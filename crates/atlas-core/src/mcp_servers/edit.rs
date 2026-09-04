//! Writing back into the agents' own configuration files.
//!
//! Three rules hold for every write here. The file is replaced through a temp file in the
//! same directory and a rename, so a failure leaves the old text intact rather than a
//! truncated one, and the target's own permissions are carried over. The previous text is
//! kept in an `mcp_config_edit` audit row (bounded to [`MAX_AUDIT_PREVIOUS`]) beside the
//! path, so an edit made from Atlas can always be undone by hand. And Atlas never creates
//! a config file for an agent the user has not set up: the one exception is a project
//! scope, where `.mcp.json`, `.codex/config.toml` and `.cursor/mcp.json` are exactly the
//! files a user adding a server to a repository means to create.
//!
//! Plugin and Atlas entries are never editable: a plugin's files belong to whoever
//! published it, and Atlas does not edit itself out of its own list.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item};
use uuid::Uuid;

use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::{
    McpServerEntry, McpServerScope, McpServerSource, McpTransportInput, NewMcpServer, Project,
};
use crate::{AtlasError, Result};

use super::generic_json::server_id;
use super::{claude, codex};

/// How much of a replaced file's text an audit row keeps.
pub const MAX_AUDIT_PREVIOUS: usize = 64 * 1024;

/// The message a source with no native switch answers with, so every caller says the
/// same thing.
pub const NO_SWITCH: &str = "this agent has no enable switch; remove the server instead";

/// Flips the agent's own switch for one server.
///
/// Claude Code keeps its switches in `~/.claude.json` under the project's root:
/// `disabledMcpServers` for the servers configured in that block, and the
/// `enabledMcpjsonServers` / `disabledMcpjsonServers` pair for the repository's own
/// `.mcp.json` entries, which it will not start until they are approved. Codex and Cursor
/// keep theirs in the server's own entry (`enabled = false`, `"disabled": true`).
pub fn set_enabled(
    db: &Db,
    home: &Path,
    project: Option<&Project>,
    entry: &McpServerEntry,
    enabled: bool,
    actor: &str,
) -> Result<()> {
    if !entry.can_toggle {
        return Err(AtlasError::Invalid(NO_SWITCH.into()));
    }
    match (entry.source, entry.scope) {
        (McpServerSource::Claude, McpServerScope::Local) => {
            let root = require_project(project)?.root_path.clone();
            claude_list_edit(db, home, &root, entry, actor, |block, name| {
                set_membership(block, "disabledMcpServers", name, !enabled);
            })
        }
        (McpServerSource::Claude, McpServerScope::Project) => {
            let root = require_project(project)?.root_path.clone();
            claude_list_edit(db, home, &root, entry, actor, |block, name| {
                set_membership(block, "enabledMcpjsonServers", name, enabled);
                set_membership(block, "disabledMcpjsonServers", name, !enabled);
            })
        }
        (McpServerSource::Codex, _) => {
            let path = entry_file(entry)?;
            let mut doc = read_toml(&path)?;
            table_for(&mut doc)?
                .get_mut(&entry.name)
                .and_then(Item::as_table_like_mut)
                .ok_or_else(|| AtlasError::NotFound(format!("mcp server {}", entry.id)))?
                .insert("enabled", toml_edit::value(enabled));
            write(db, &path, doc.to_string(), "set_enabled", entry.id.clone(), actor)
        }
        (McpServerSource::Cursor, _) => {
            let path = entry_file(entry)?;
            let mut config = read_json(&path)?;
            let server = config
                .get_mut("mcpServers")
                .and_then(Value::as_object_mut)
                .and_then(|m| m.get_mut(&entry.name))
                .and_then(Value::as_object_mut)
                .ok_or_else(|| AtlasError::NotFound(format!("mcp server {}", entry.id)))?;
            // Cursor's switch is the negative one, so an enabled server carries no key
            // rather than `"disabled": false`, which is what its own UI writes.
            if enabled {
                server.remove("disabled");
            } else {
                server.insert("disabled".into(), Value::Bool(true));
            }
            write(db, &path, pretty(&config), "set_enabled", entry.id.clone(), actor)
        }
        _ => Err(AtlasError::Invalid(NO_SWITCH.into())),
    }
}

/// Writes a new server into one agent's configuration and answers with its id. A name the
/// target file already holds is a `Conflict`: replacing a server the user configured
/// elsewhere is never what an Add button meant.
pub fn add_server(db: &Db, home: &Path, project: Option<&Project>, input: &NewMcpServer, actor: &str) -> Result<String> {
    validate_name(&input.name)?;
    let target = target_file(home, project, input.source, input.scope)?;
    match input.source {
        McpServerSource::Codex => {
            let mut doc = match read_toml_if_present(&target)? {
                Some(doc) => doc,
                None => {
                    ensure_creatable(input.scope, &target)?;
                    DocumentMut::new()
                }
            };
            let table = table_for(&mut doc)?;
            if table.get(&input.name).is_some() {
                return Err(AtlasError::Conflict(format!("{} already has a server called '{}'", target.display(), input.name)));
            }
            table.insert(&input.name, Item::Table(toml_entry(&input.transport)));
            write(db, &target, doc.to_string(), "add", server_id(input.source, input.scope.as_str(), &input.name), actor)?;
        }
        _ => {
            let mut config = match read_json_if_present(&target)? {
                Some(config) => config,
                None => {
                    ensure_creatable(input.scope, &target)?;
                    Value::Object(Map::new())
                }
            };
            let servers = json_servers_mut(&mut config, project, input.source, input.scope)?;
            if servers.contains_key(&input.name) {
                return Err(AtlasError::Conflict(format!("{} already has a server called '{}'", target.display(), input.name)));
            }
            servers.insert(input.name.clone(), json_entry(&input.transport));
            write(db, &target, pretty(&config), "add", server_id(input.source, input.scope.as_str(), &input.name), actor)?;
        }
    }
    Ok(server_id(input.source, input.scope.as_str(), &input.name))
}

/// Deletes a server from the file it came from. The agent's own switches are left alone:
/// a stale name in `disabledMcpServers` gates nothing, and clearing it would mean editing
/// a second file for one removal.
pub fn remove_server(db: &Db, project: Option<&Project>, entry: &McpServerEntry, actor: &str) -> Result<()> {
    if !entry.can_remove {
        return Err(AtlasError::Invalid(format!("'{}' is not a server Atlas can remove", entry.id)));
    }
    let path = entry_file(entry)?;
    if entry.source == McpServerSource::Codex {
        let mut doc = read_toml(&path)?;
        table_for(&mut doc)?
            .remove(&entry.name)
            .ok_or_else(|| AtlasError::NotFound(format!("mcp server {}", entry.id)))?;
        return write(db, &path, doc.to_string(), "remove", entry.id.clone(), actor);
    }
    let mut config = read_json(&path)?;
    let servers = json_servers_mut(&mut config, project, entry.source, entry.scope)?;
    servers.remove(&entry.name).ok_or_else(|| AtlasError::NotFound(format!("mcp server {}", entry.id)))?;
    write(db, &path, pretty(&config), "remove", entry.id.clone(), actor)
}

// ---------------------------------------------------------------------------
// Where a server lives
// ---------------------------------------------------------------------------

/// The file a new server of this source and scope belongs in.
fn target_file(home: &Path, project: Option<&Project>, source: McpServerSource, scope: McpServerScope) -> Result<PathBuf> {
    let root = |p: Option<&Project>| require_project(p).map(|p| PathBuf::from(&p.root_path));
    Ok(match (source, scope) {
        (McpServerSource::Claude, McpServerScope::User) => claude::config_path(home),
        (McpServerSource::Claude, McpServerScope::Local) => claude::config_path(home),
        (McpServerSource::Claude, McpServerScope::Project) => root(project)?.join(".mcp.json"),
        (McpServerSource::Codex, McpServerScope::User) => home.join(".codex/config.toml"),
        (McpServerSource::Codex, McpServerScope::Project) => root(project)?.join(".codex/config.toml"),
        (McpServerSource::Cursor, McpServerScope::User) => home.join(".cursor/mcp.json"),
        (McpServerSource::Cursor, McpServerScope::Project) => root(project)?.join(".cursor/mcp.json"),
        (McpServerSource::Gemini, McpServerScope::User) => home.join(".gemini/settings.json"),
        (McpServerSource::Windsurf, McpServerScope::User) => home.join(".codeium/windsurf/mcp_config.json"),
        (source, scope) => {
            return Err(AtlasError::Invalid(format!("Atlas cannot add a {} server in the {scope} scope", source.as_str())))
        }
    })
}

/// The `mcpServers` object a JSON-configured agent keeps its servers in, created when the
/// file has none yet. Claude Code's `local` scope nests it under the project's own block,
/// which is created the same way.
fn json_servers_mut<'a>(
    config: &'a mut Value,
    project: Option<&Project>,
    source: McpServerSource,
    scope: McpServerScope,
) -> Result<&'a mut Map<String, Value>> {
    let object = config.as_object_mut().ok_or_else(|| AtlasError::Invalid("the configuration file is not a JSON object".into()))?;
    let container = if source == McpServerSource::Claude && scope == McpServerScope::Local {
        let root = require_project(project)?.root_path.clone();
        object
            .entry("projects")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| AtlasError::Invalid("'projects' is not an object".into()))?
            .entry(root)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| AtlasError::Invalid("the project's block is not an object".into()))?
    } else {
        object
    };
    container
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| AtlasError::Invalid("'mcpServers' is not an object".into()))
}

/// The file an existing entry was read from. Every entry a caller can reach here carries
/// one; Atlas's own does not, and it is refused long before this.
fn entry_file(entry: &McpServerEntry) -> Result<PathBuf> {
    entry
        .file
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| AtlasError::Invalid(format!("'{}' comes from no file Atlas can edit", entry.id)))
}

fn require_project(project: Option<&Project>) -> Result<&Project> {
    project.ok_or_else(|| AtlasError::Invalid("this scope needs a project".into()))
}

// ---------------------------------------------------------------------------
// Claude Code's per-project switch lists
// ---------------------------------------------------------------------------

/// Applies `edit` to the project's block in `~/.claude.json` and writes the file back.
/// The block is created when the project has none: Claude Code writes one the first time
/// it is used in a repository, and a user switching a server off before then still means
/// it off.
fn claude_list_edit(
    db: &Db,
    home: &Path,
    root: &str,
    entry: &McpServerEntry,
    actor: &str,
    edit: impl FnOnce(&mut Map<String, Value>, &str),
) -> Result<()> {
    let path = claude::config_path(home);
    let mut config = read_json(&path)?;
    let object = config.as_object_mut().ok_or_else(|| AtlasError::Invalid(format!("{} is not a JSON object", path.display())))?;
    let block = object
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| AtlasError::Invalid("'projects' is not an object".into()))?
        .entry(root.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| AtlasError::Invalid("the project's block is not an object".into()))?;
    edit(block, &entry.name);
    write(db, &path, pretty(&config), "set_enabled", entry.id.clone(), actor)
}

/// Puts `name` in `key`'s array, or takes it out, leaving the rest of the list in the
/// order the agent wrote it.
fn set_membership(block: &mut Map<String, Value>, key: &str, name: &str, present: bool) {
    let list = block.entry(key.to_string()).or_insert_with(|| Value::Array(Vec::new()));
    if !list.is_array() {
        *list = Value::Array(Vec::new());
    }
    let array = list.as_array_mut().expect("just made an array");
    array.retain(|v| v.as_str() != Some(name));
    if present {
        array.push(Value::String(name.to_string()));
    }
}

// ---------------------------------------------------------------------------
// Building an entry
// ---------------------------------------------------------------------------

fn json_entry(transport: &McpTransportInput) -> Value {
    let mut object = Map::new();
    match transport {
        McpTransportInput::Stdio { command, args, env } => {
            object.insert("command".into(), Value::String(command.clone()));
            object.insert("args".into(), Value::Array(args.iter().map(|a| Value::String(a.clone())).collect()));
            if !env.is_empty() {
                object.insert("env".into(), Value::Object(env.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()));
            }
        }
        McpTransportInput::Http { url, headers } => {
            object.insert("type".into(), Value::String("http".into()));
            object.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                object.insert(
                    "headers".into(),
                    Value::Object(headers.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()),
                );
            }
        }
    }
    Value::Object(object)
}

fn toml_entry(transport: &McpTransportInput) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    match transport {
        McpTransportInput::Stdio { command, args, env } => {
            table.insert("command", toml_edit::value(command.as_str()));
            let mut array = toml_edit::Array::new();
            for arg in args {
                array.push(arg.as_str());
            }
            table.insert("args", toml_edit::value(array));
            if !env.is_empty() {
                let mut sub = toml_edit::Table::new();
                for (key, value) in env {
                    sub.insert(key, toml_edit::value(value.as_str()));
                }
                table.insert("env", Item::Table(sub));
            }
        }
        McpTransportInput::Http { url, .. } => {
            // Codex has no headers of its own for an HTTP server; anything a caller sent
            // would silently vanish, so it is not written and not claimed.
            table.insert("url", toml_edit::value(url.as_str()));
        }
    }
    table
}

/// A server name Atlas will write. Bounded, printable and without the separators an id is
/// built from, so a name can never split an id into a different one.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AtlasError::Invalid("an MCP server name must be 1..64 characters".into()));
    }
    if name.chars().any(|c| c.is_control() || c == ':' || c == '/') {
        return Err(AtlasError::Invalid("an MCP server name cannot contain ':' or '/' or control characters".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading and writing files
// ---------------------------------------------------------------------------

fn read_json(path: &Path) -> Result<Value> {
    read_json_if_present(path)?.ok_or_else(|| AtlasError::NotFound(format!("{}", path.display())))
}

fn read_json_if_present(path: &Path) -> Result<Option<Value>> {
    let Some(text) = super::read_config_for_edit(path)? else { return Ok(None) };
    if text.trim().is_empty() {
        return Ok(Some(Value::Object(Map::new())));
    }
    serde_json::from_str(&text).map(Some).map_err(|e| AtlasError::Invalid(format!("{}: {e}", path.display())))
}

fn read_toml(path: &Path) -> Result<DocumentMut> {
    read_toml_if_present(path)?.ok_or_else(|| AtlasError::NotFound(format!("{}", path.display())))
}

fn read_toml_if_present(path: &Path) -> Result<Option<DocumentMut>> {
    let Some(text) = super::read_config_for_edit(path)? else { return Ok(None) };
    text.parse::<DocumentMut>().map(Some).map_err(|e| AtlasError::Invalid(format!("{}: {e}", path.display())))
}

/// `[mcp_servers]` in a Codex document, created implicit so it renders as
/// `[mcp_servers.<name>]` headers rather than an empty parent table.
fn table_for(doc: &mut DocumentMut) -> Result<&mut dyn toml_edit::TableLike> {
    if doc.get(codex::TABLE).is_none() {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc.insert(codex::TABLE, Item::Table(table));
    }
    doc.get_mut(codex::TABLE)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| AtlasError::Invalid(format!("'{}' is not a table", codex::TABLE)))
}

/// Two-space pretty JSON with a trailing newline, which is what every agent writing one of
/// these files produces. `serde_json`'s `preserve_order` keeps the existing keys in the
/// order they were read, with a new one appended.
fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    text.push('\n');
    text
}

/// Refuses to create a file for an agent the user has not set up. A project scope is the
/// exception: `.mcp.json`, `<root>/.codex/config.toml` and `<root>/.cursor/mcp.json` are
/// the files an add to a repository is meant to create, so their parent directory is made
/// here and the caller writes into an empty document.
fn ensure_creatable(scope: McpServerScope, path: &Path) -> Result<()> {
    if scope != McpServerScope::Project {
        return Err(AtlasError::Invalid(format!(
            "{} does not exist; Atlas does not create a configuration file for an agent that is not set up",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Replaces `path` atomically and records the previous text.
fn write(db: &Db, path: &Path, text: String, action: &str, id: String, actor: &str) -> Result<()> {
    let previous = std::fs::read_to_string(path).unwrap_or_default();
    let directory = path.parent().ok_or_else(|| AtlasError::Invalid(format!("{} has no directory", path.display())))?;
    let mode = std::fs::metadata(path).map(|m| m.permissions()).ok();
    let temp = directory.join(format!(".atlas-{}.tmp", Uuid::new_v4()));
    let written = (|| -> std::io::Result<()> {
        std::fs::write(&temp, &text)?;
        if let Some(mode) = mode {
            std::fs::set_permissions(&temp, mode)?;
        }
        std::fs::rename(&temp, path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    // The whole point of `previous` is that an edit Atlas made can be undone by hand, so
    // it is stored verbatim up to the cap rather than summarised.
    let kept: String = previous.chars().take(MAX_AUDIT_PREVIOUS).collect();
    MemoryRepo::new(db).audit(
        actor,
        "mcp_config_edit",
        "mcp_server",
        None,
        serde_json::json!({
            "action": action,
            "id": id,
            "path": path.display().to_string(),
            "previous": kept,
            "previous_truncated": previous.chars().count() > MAX_AUDIT_PREVIOUS,
        }),
    )
}
