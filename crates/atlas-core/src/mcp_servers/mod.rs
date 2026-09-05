//! The MCP servers the user's agents are wired to, read from the agents' own
//! configuration files.
//!
//! [`discover`] walks Claude Code's `~/.claude.json` and `.mcp.json` files, Codex's
//! `config.toml`, Cursor's, Gemini CLI's and Windsurf's `mcpServers` blocks and the
//! Claude Code plugin cache, and normalises every one into an [`McpServerEntry`] with a
//! stable id. [`check`] starts a server and asks it for its tools. [`edit`] writes back
//! to the file an entry came from.
//!
//! Two rules hold everywhere below. Secret values (`env` and `headers`) stay inside this
//! module: an entry carries key names only, and the values are re-read from the file when
//! a check actually needs them. And an id is only ever resolved against a listing taken
//! right now, so no caller-supplied string becomes a path.

pub mod check;
pub mod claude;
pub mod codex;
pub mod discover;
pub mod edit;
pub mod generic_json;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::db::Db;
use crate::models::{McpCheckResult, McpServerEntry, McpServerList, NewMcpServer, Project};
use crate::paths::AtlasPaths;
use crate::{AtlasError, Result};

/// No agent configuration file is read past this. A file larger than it is reported as a
/// warning rather than parsed from a truncated prefix, which would only ever be a
/// confusing syntax error.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// A discovered server plus the secret values its file holds. The values never leave this
/// module: [`McpServerEntry`] carries key names, and only [`check`] reads them back.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub entry: McpServerEntry,
    pub env: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    /// The working directory the agent would start the server in, for the one agent that
    /// configures one (Codex's `cwd`). A relative command depends on it, so a check that
    /// ignored it would fail on a server the agent itself starts.
    pub cwd: Option<PathBuf>,
}

/// Everything discovery found, plus what it could not read.
#[derive(Debug, Default)]
pub struct Found {
    pub servers: Vec<Resolved>,
    pub warnings: Vec<String>,
}

/// Every MCP server that applies, sorted by name then id so two servers of the same name
/// from different agents keep a stable order.
///
/// Without a project this is every agent's user-level configuration, the installed
/// plugins' servers and Atlas. With one it is that project's own scopes (Claude Code's
/// `.mcp.json` and its `~/.claude.json` block, Codex's and Cursor's project files) plus
/// the plugin servers and Atlas, which apply wherever the user works. The desktop decides
/// how to group them.
pub fn list_mcp_servers(home: &Path, project: Option<&Project>) -> McpServerList {
    let found = discover::discover(home, project);
    McpServerList { servers: found.servers.into_iter().map(|r| r.entry).collect(), warnings: found.warnings }
}

/// Starts the server `id` names and asks it for its tools. See [`check::run`] for the
/// timeout and the process handling.
pub async fn check_mcp_server(home: &Path, project: Option<&Project>, id: &str) -> Result<McpCheckResult> {
    let (home, project, id) = (home.to_path_buf(), project.cloned(), id.to_string());
    // Discovery is a handful of small file reads, but it is blocking work all the same,
    // and this is the one entry point that is already async.
    let resolved = tokio::task::spawn_blocking(move || find(&home, project.as_ref(), &id))
        .await
        .map_err(|e| AtlasError::Internal(format!("blocking task failed: {e}")))??;
    Ok(check::run(&resolved).await)
}

/// Flips the agent's own enable switch for one server. `Invalid` where the agent has
/// none: see [`edit::set_enabled`].
pub fn set_mcp_server_enabled(
    paths: &AtlasPaths,
    db: &Db,
    project: Option<&Project>,
    id: &str,
    enabled: bool,
    actor: &str,
) -> Result<McpServerEntry> {
    let resolved = find(paths.agent_home(), project, id)?;
    edit::set_enabled(paths, db, project, &resolved.entry, enabled, actor)?;
    find(paths.agent_home(), project, id).map(|r| r.entry)
}

/// Writes a new server into one agent's configuration. Refuses a name the target file
/// already holds rather than replacing it.
pub fn add_mcp_server(paths: &AtlasPaths, db: &Db, project: Option<&Project>, input: &NewMcpServer, actor: &str) -> Result<McpServerEntry> {
    let id = edit::add_server(paths, db, project, input, actor)?;
    find(paths.agent_home(), project, &id).map(|r| r.entry)
}

/// Deletes a server from the file it came from.
pub fn remove_mcp_server(paths: &AtlasPaths, db: &Db, project: Option<&Project>, id: &str, actor: &str) -> Result<()> {
    let resolved = find(paths.agent_home(), project, id)?;
    edit::remove_server(paths, db, project, &resolved.entry, actor)
}

/// The one entry whose id matches, resolved against a listing taken right now.
pub(crate) fn find(home: &Path, project: Option<&Project>, id: &str) -> Result<Resolved> {
    discover::discover(home, project)
        .servers
        .into_iter()
        .find(|r| r.entry.id == id)
        .ok_or_else(|| AtlasError::NotFound(format!("mcp server {id}")))
}

/// A configuration file's text, `None` when it is simply not there. Most agents are not
/// installed on most machines, so an absent file is silent; anything else, including a
/// file over [`MAX_CONFIG_BYTES`], is a warning naming the path.
pub(crate) fn read_config(path: &Path, warnings: &mut Vec<String>) -> Option<String> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warnings.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    match file.metadata().map(|m| m.len()) {
        Ok(len) if len > MAX_CONFIG_BYTES => {
            warnings.push(format!("{}: larger than {MAX_CONFIG_BYTES} bytes, not read", path.display()));
            return None;
        }
        _ => {}
    }
    let mut buf = Vec::new();
    if let Err(e) = (&file).take(MAX_CONFIG_BYTES).read_to_end(&mut buf) {
        warnings.push(format!("{}: {e}", path.display()));
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The same read for a path an edit is about to write: an unreadable file is an error
/// here, not a warning, since the caller is about to replace it.
pub(crate) fn read_config_for_edit(path: &Path) -> Result<Option<String>> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(m) if m.len() > MAX_CONFIG_BYTES => {
            return Err(AtlasError::TooLarge(format!("{} is larger than {MAX_CONFIG_BYTES} bytes", path.display())))
        }
        Ok(_) => {}
    }
    Ok(Some(std::fs::read_to_string(path)?))
}

/// The absolute path of `path`, as a string, without requiring it to exist: a project
/// scope's file may be the one an add is about to create.
pub(crate) fn display(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).display().to_string()
}

/// A TOML parse failure named by position alone.
///
/// `toml_edit`'s own `Display` prints the offending source line under the message, and
/// these files are full of tokens: a secret sitting on a line that fails to parse would
/// travel into a listing's `warnings`, into an HTTP error body and onto the desktop. Only
/// the path and the line and column ever come back.
pub(crate) fn toml_error(path: &Path, text: &str, error: &toml_edit::TomlError) -> String {
    let (line, column) = error.span().map(|s| position(text, s.start)).unwrap_or((1, 1));
    format!("{}: not valid TOML at line {line}, column {column}", path.display())
}

/// The same for JSON. `serde_json`'s own `Display` is already position only; this is here
/// so both agents' messages read the same and neither can start quoting a line.
pub(crate) fn json_error(path: &Path, error: &serde_json::Error) -> String {
    format!("{}: not valid JSON at line {}, column {}", path.display(), error.line(), error.column())
}

/// The 1-based line and column of a byte offset into `text`.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let (mut line, mut column) = (1, 1);
    for (index, ch) in text.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{McpServerScope, McpServerSource, McpTransport, McpTransportInput};
    use crate::projects::ProjectRepo;
    use std::collections::BTreeMap;

    /// The value planted in every `env` and `headers` block in the fixtures. No listing,
    /// no entry and no error may ever carry it.
    const SECRET: &str = "SECRET-DO-NOT-LEAK";

    /// Copies the fixture home and project into a temp directory, points the project
    /// block in `.claude.json` at the copied root, and registers the project.
    ///
    /// Copied rather than read in place, because every edit test writes, and the real
    /// `~/.claude.json` and `~/.codex/config.toml` must never be within reach: nothing
    /// here ever reads a path the fixture did not put there.
    fn fixture(db: &Db) -> (tempfile::TempDir, AtlasPaths, Project) {
        let temp = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_servers");
        let home = temp.path().join("home");
        let temp_atlas = temp.path().join("atlas-home");
        let root = temp.path().join("project");
        copy_tree(&fixtures.join("home"), &home);
        copy_tree(&fixtures.join("project"), &root);

        // The fixture's project block is keyed by a placeholder, since the real key is an
        // absolute path only this run knows.
        let claude = home.join(".claude.json");
        let text = std::fs::read_to_string(&claude).unwrap().replace("__PROJECT_ROOT__", root.to_str().unwrap());
        std::fs::write(&claude, text).unwrap();

        // The plugin cache picks the version directory with the newest modification time,
        // and a copy gives every directory the same one.
        let cache = home.join(".claude/plugins/cache/acme/tools");
        let now = std::time::SystemTime::now();
        set_mtime(&cache.join("aaaa1111"), now - std::time::Duration::from_secs(600));
        set_mtime(&cache.join("bbbb2222"), now);

        let detected = crate::projects::Detected { root: root.clone(), remote: None };
        let project = ProjectRepo::new(db).upsert(&detected, None, "t").unwrap();
        // Atlas's own home and the agent home are two different directories here, which is
        // what a real daemon under `ATLAS_SYNC_HOME` looks like and what keeps the backups
        // out of the tree being edited.
        (temp, AtlasPaths::at(temp_atlas).with_skills_home(&home), project)
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    fn set_mtime(dir: &Path, when: std::time::SystemTime) {
        std::fs::File::open(dir).unwrap().set_modified(when).unwrap();
    }

    fn ids(list: &McpServerList) -> Vec<&str> {
        list.servers.iter().map(|s| s.id.as_str()).collect()
    }

    fn entry<'a>(list: &'a McpServerList, id: &str) -> &'a McpServerEntry {
        list.servers.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no {id} in {:?}", ids(list)))
    }

    /// Without a project: every agent's user level, the newest version of each installed
    /// plugin, and Atlas.
    #[test]
    fn the_global_listing_covers_every_agent() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let list = list_mcp_servers(paths.agent_home(), None);

        assert_eq!(
            ids(&list),
            vec![
                "atlas",
                "codex:user:computer-use",
                "cursor:user:cursor-off",
                "cursor:user:cursor-one",
                "gemini:user:gemini-http",
                "gemini:user:gemini-one",
                "codex:user:node_repl",
                "plugin:acme/tools:packaged",
                "plugin:acme/tools:packaged-off",
                "codex:user:playwright",
                "claude:user:svelte",
                "claude:user:user-stdio",
                "windsurf:user:windsurf-one",
            ],
            "{:?}",
            ids(&list)
        );
        assert!(list.warnings.is_empty(), "{:?}", list.warnings);

        // Claude Code's HTTP server keeps its header names and loses the token.
        let svelte = entry(&list, "claude:user:svelte");
        assert_eq!(
            svelte.transport,
            McpTransport::Http { url: "https://svelte.example/mcp".into(), header_keys: vec!["Authorization".into()] }
        );
        assert!(svelte.file.as_ref().unwrap().ends_with(".claude.json"), "{svelte:?}");
        assert!(svelte.enabled && !svelte.can_toggle && svelte.can_remove, "no global switch, but removable: {svelte:?}");

        // Codex carries its own switch, so its rows can be toggled and one is off.
        assert!(!entry(&list, "codex:user:computer-use").enabled, "enabled = false in the file");
        assert!(entry(&list, "codex:user:playwright").enabled);
        assert!(entry(&list, "codex:user:node_repl").can_toggle);
        assert_eq!(
            entry(&list, "codex:user:node_repl").transport,
            McpTransport::Stdio { command: "node_repl".into(), args: vec![], env_keys: vec!["NODE_TOKEN".into()] }
        );

        // Cursor's switch lives in the entry itself.
        assert!(!entry(&list, "cursor:user:cursor-off").enabled);
        assert!(entry(&list, "cursor:user:cursor-one").enabled && entry(&list, "cursor:user:cursor-one").can_toggle);

        // Gemini CLI and Windsurf have no switch of their own, so Remove is the only way.
        assert!(!entry(&list, "gemini:user:gemini-one").can_toggle);
        // Gemini spells the HTTP key `httpUrl`; reading it as a URL is what keeps a valid
        // server off the warnings list.
        assert_eq!(
            entry(&list, "gemini:user:gemini-http").transport,
            McpTransport::Http { url: "https://gemini.example/mcp".into(), header_keys: vec!["Authorization".into()] }
        );
        assert!(entry(&list, "windsurf:user:windsurf-one").can_remove);

        // The newest plugin version wins, its placeholders come through verbatim, and
        // nothing about it is editable.
        let packaged = entry(&list, "plugin:acme/tools:packaged");
        assert_eq!(
            packaged.transport,
            McpTransport::Stdio {
                command: "${CLAUDE_PLUGIN_ROOT}/bin/server".into(),
                args: vec!["--serve".into()],
                env_keys: vec!["PLUGIN_TOKEN".into()],
            }
        );
        assert_eq!(packaged.plugin.as_deref(), Some("acme/tools"));
        assert_eq!(packaged.scope, McpServerScope::Plugin);
        assert!(packaged.enabled, "enabledPlugins lists tools@acme");
        assert!(!packaged.can_toggle && !packaged.can_remove, "{packaged:?}");

        let atlas = entry(&list, "atlas");
        assert!(atlas.is_atlas && atlas.enabled && !atlas.can_toggle && !atlas.can_remove);
        assert_eq!(atlas.transport, McpTransport::Stdio { command: "atlas".into(), args: vec!["mcp".into()], env_keys: vec![] });
        assert_eq!(atlas.file, None);
    }

    /// A project swaps the user scopes for its own, and keeps the plugins and Atlas,
    /// which apply wherever the user works.
    #[test]
    fn a_project_listing_is_that_project_plus_the_plugins_and_atlas() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        let list = list_mcp_servers(paths.agent_home(), Some(&project));

        assert_eq!(
            ids(&list),
            vec![
                "claude:project:approved",
                "atlas",
                "plugin:acme/tools:packaged",
                "plugin:acme/tools:packaged-off",
                "claude:local:playwright",
                "codex:project:repo-codex",
                "cursor:project:repo-cursor",
                "claude:local:switched-off",
                "claude:project:unapproved",
            ],
            "{:?}",
            ids(&list)
        );
        assert!(list.warnings.is_empty(), "{:?}", list.warnings);

        // Claude Code's per-project `disabledMcpServers` gates the local entries.
        assert!(entry(&list, "claude:local:playwright").enabled);
        assert!(!entry(&list, "claude:local:switched-off").enabled);
        assert!(entry(&list, "claude:local:playwright").can_toggle);
        assert_eq!(entry(&list, "claude:local:playwright").project_id, Some(project.id));

        // A `.mcp.json` server is off until the user approves it.
        assert!(entry(&list, "claude:project:approved").enabled, "listed in enabledMcpjsonServers");
        assert!(!entry(&list, "claude:project:unapproved").enabled, "never approved");
        assert!(entry(&list, "claude:project:approved").file.as_ref().unwrap().ends_with(".mcp.json"));

        assert!(entry(&list, "codex:project:repo-codex").enabled);
        assert!(entry(&list, "cursor:project:repo-cursor").enabled);

        // The same `disabledMcpServers` gates a plugin's servers within the project, under
        // `plugin:<plugin>:<server>`, and every server of an enabled plugin has a switch here.
        assert!(entry(&list, "plugin:acme/tools:packaged").enabled);
        assert!(!entry(&list, "plugin:acme/tools:packaged-off").enabled, "listed as plugin:tools:packaged-off");
        assert!(entry(&list, "plugin:acme/tools:packaged").can_toggle);
        assert!(entry(&list, "plugin:acme/tools:packaged-off").can_toggle);
        assert!(!entry(&list, "plugin:acme/tools:packaged").can_remove, "the plugin's own file is never edited");

        // Without a project there is no list to flip, so the rows only follow their plugin.
        let global = list_mcp_servers(paths.agent_home(), None);
        assert!(entry(&global, "plugin:acme/tools:packaged-off").enabled);
        assert!(!entry(&global, "plugin:acme/tools:packaged-off").can_toggle);
    }

    /// A plugin server's per-project switch is Claude Code's `disabledMcpServers`, written
    /// under the `plugin:<plugin>:<server>` key Claude Code itself uses (no marketplace),
    /// next to the local servers' bare names.
    #[test]
    fn a_plugin_server_switches_per_project_through_disabled_mcp_servers() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        let root = project.root_path.clone();
        let project = Some(&project);
        let disabled_list = || -> Vec<String> {
            let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(paths.agent_home().join(".claude.json")).unwrap()).unwrap();
            let block = &config["projects"][&root];
            block["disabledMcpServers"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
        };

        set_mcp_server_enabled(&paths, &db, project, "plugin:acme/tools:packaged", false, "t").unwrap();
        assert!(!entry(&list_mcp_servers(paths.agent_home(), project), "plugin:acme/tools:packaged").enabled);
        assert_eq!(disabled_list(), vec!["switched-off", "plugin:tools:packaged-off", "plugin:tools:packaged"]);

        set_mcp_server_enabled(&paths, &db, project, "plugin:acme/tools:packaged-off", true, "t").unwrap();
        let list = list_mcp_servers(paths.agent_home(), project);
        assert!(entry(&list, "plugin:acme/tools:packaged-off").enabled);
        assert!(!entry(&list, "plugin:acme/tools:packaged").enabled, "the other one is untouched");
        assert_eq!(disabled_list(), vec!["switched-off", "plugin:tools:packaged"]);

        // The plugin's own `.mcp.json` was never written.
        let cache = paths.agent_home().join(".claude/plugins/cache/acme/tools/bbbb2222/.mcp.json");
        assert!(std::fs::read_to_string(cache).unwrap().contains("packaged-off"));
        let audited: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'mcp_config_edit'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(audited, 2);
    }

    /// The one rule that would be worst to get wrong: a token in an agent's config never
    /// reaches a caller, in any field, in either listing.
    #[test]
    fn no_secret_value_ever_leaves_the_daemon() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        for list in [list_mcp_servers(paths.agent_home(), None), list_mcp_servers(paths.agent_home(), Some(&project))] {
            let json = serde_json::to_string(&list).unwrap();
            assert!(!json.contains(SECRET), "a secret reached the wire: {json}");
            // The key names do come through, which is what a client needs to show.
            assert!(json.contains("env_keys") || json.contains("header_keys"), "{json}");
        }
        let global = serde_json::to_string(&list_mcp_servers(paths.agent_home(), None)).unwrap();
        assert!(global.contains("NODE_TOKEN"), "the key name is what a listing shows: {global}");
    }

    /// A half-finished plugin install is not a version, and a disabled plugin's servers
    /// list switched off rather than vanishing.
    #[test]
    fn a_scratch_plugin_directory_is_skipped() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let list = list_mcp_servers(paths.agent_home(), None);
        assert!(list.servers.iter().all(|s| s.name != "half-installed"), "{:?}", ids(&list));
    }

    /// An unreadable or unparsable file is one warning, not a failed listing.
    #[test]
    fn a_broken_config_is_a_warning() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        std::fs::write(paths.agent_home().join(".cursor/mcp.json"), "{ this is not json").unwrap();
        let list = list_mcp_servers(paths.agent_home(), None);
        assert!(list.servers.iter().all(|s| s.source != McpServerSource::Cursor), "{:?}", ids(&list));
        assert_eq!(list.warnings.len(), 1, "{:?}", list.warnings);
        assert!(list.warnings[0].contains(".cursor/mcp.json"), "{:?}", list.warnings);
        // Every other agent still lists.
        assert!(list.servers.iter().any(|s| s.id == "codex:user:playwright"));
    }

    /// Claude Code's two switch shapes round trip through `~/.claude.json` without
    /// disturbing the rest of the file.
    #[test]
    fn claude_code_enable_and_disable_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        let project = Some(&project);

        set_mcp_server_enabled(&paths, &db, project, "claude:local:playwright", false, "t").unwrap();
        assert!(!entry(&list_mcp_servers(paths.agent_home(), project), "claude:local:playwright").enabled);
        set_mcp_server_enabled(&paths, &db, project, "claude:local:playwright", true, "t").unwrap();
        assert!(entry(&list_mcp_servers(paths.agent_home(), project), "claude:local:playwright").enabled);

        set_mcp_server_enabled(&paths, &db, project, "claude:project:unapproved", true, "t").unwrap();
        let list = list_mcp_servers(paths.agent_home(), project);
        assert!(entry(&list, "claude:project:unapproved").enabled, "approving it is what turns it on");
        assert!(entry(&list, "claude:project:approved").enabled, "the other one is untouched");

        // The rest of `~/.claude.json` survived four rewrites.
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(paths.agent_home().join(".claude.json")).unwrap()).unwrap();
        assert_eq!(config["numStartups"], 7);
        assert_eq!(config["mcpServers"]["svelte"]["headers"]["Authorization"], format!("Bearer {SECRET}"));
    }

    /// Cursor's switch is the entry's own `disabled`, and an enabled server carries no
    /// key at all rather than `"disabled": false`.
    #[test]
    fn cursor_enable_and_disable_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", false, "t").unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(paths.agent_home().join(".cursor/mcp.json")).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["cursor-one"]["disabled"], true);

        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", true, "t").unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(paths.agent_home().join(".cursor/mcp.json")).unwrap()).unwrap();
        assert!(config["mcpServers"]["cursor-one"].get("disabled").is_none(), "{config}");
        assert_eq!(config["mcpServers"]["cursor-one"]["env"]["CURSOR_TOKEN"], SECRET, "the value stayed in its own file");
    }

    /// Where the agent has no switch, the toggle refuses with the one message every
    /// caller says, and nothing is written.
    #[test]
    fn an_agent_with_no_switch_refuses_the_toggle() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        for id in ["gemini:user:gemini-one", "windsurf:user:windsurf-one", "plugin:acme/tools:packaged", "atlas", "claude:user:svelte"] {
            let err = set_mcp_server_enabled(&paths, &db, None, id, false, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(ref m) if m == edit::NO_SWITCH), "{id}: {err}");
        }
        let audited: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'mcp_config_edit'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(audited, 0, "a refusal writes nothing");
    }

    /// A TOML add keeps the rest of the document byte for byte, comments included.
    #[test]
    fn adding_a_codex_server_preserves_the_document() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let before = std::fs::read_to_string(paths.agent_home().join(".codex/config.toml")).unwrap();

        let input = NewMcpServer {
            source: McpServerSource::Codex,
            scope: McpServerScope::User,
            project_id: None,
            name: "added".into(),
            transport: McpTransportInput::Stdio {
                command: "added-server".into(),
                args: vec!["--go".into()],
                env: BTreeMap::from([("ADDED_TOKEN".to_string(), SECRET.to_string())]),
            },
        };
        let added = add_mcp_server(&paths, &db, None, &input, "t").unwrap();
        assert_eq!(added.id, "codex:user:added");
        assert_eq!(added.transport, McpTransport::Stdio { command: "added-server".into(), args: vec!["--go".into()], env_keys: vec!["ADDED_TOKEN".into()] });

        let after = std::fs::read_to_string(paths.agent_home().join(".codex/config.toml")).unwrap();
        assert!(after.starts_with(&before), "everything before the addition is unchanged:\n{after}");
        assert!(after.contains("# Codex's own file, comments and all."), "{after}");
        assert!(after.contains("[mcp_servers.added]"), "{after}");
        assert!(after.contains("ADDED_TOKEN"), "the value is written into the file it belongs in");

        // A second add of the same name is a conflict, not a replacement.
        let err = add_mcp_server(&paths, &db, None, &input, "t").unwrap_err();
        assert!(matches!(err, AtlasError::Conflict(_)), "{err}");
    }

    /// A JSON add keeps the existing keys and their order, and appends the new server.
    #[test]
    fn adding_a_json_server_keeps_the_existing_keys() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let input = NewMcpServer {
            source: McpServerSource::Claude,
            scope: McpServerScope::User,
            project_id: None,
            name: "added".into(),
            transport: McpTransportInput::Http {
                url: "https://added.example/mcp".into(),
                headers: BTreeMap::from([("Authorization".to_string(), SECRET.to_string())]),
            },
        };
        let added = add_mcp_server(&paths, &db, None, &input, "t").unwrap();
        assert_eq!(added.id, "claude:user:added");
        assert_eq!(added.transport, McpTransport::Http { url: "https://added.example/mcp".into(), header_keys: vec!["Authorization".into()] });

        let text = std::fs::read_to_string(paths.agent_home().join(".claude.json")).unwrap();
        let config: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<&str> = config.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["numStartups", "mcpServers", "projects"], "the file's own key order survived");
        let servers: Vec<&str> = config["mcpServers"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(servers, vec!["svelte", "user-stdio", "added"], "the new server is appended");
        assert_eq!(config["projects"].as_object().unwrap().len(), 1, "the project block is intact");
        assert!(text.contains("\n  \"mcpServers\""), "two-space pretty JSON: {}", &text[..80.min(text.len())]);
    }

    /// A project scope is the one place Atlas creates a file: `.mcp.json` and the two
    /// project files an agent has not been used in yet. A user scope that is not set up
    /// is refused instead.
    #[test]
    fn only_a_project_scope_may_create_a_file() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        std::fs::remove_file(Path::new(&project.root_path).join(".cursor/mcp.json")).unwrap();
        std::fs::remove_dir_all(paths.agent_home().join(".gemini")).unwrap();

        let created = add_mcp_server(
            &paths,
            &db,
            Some(&project),
            &NewMcpServer {
                source: McpServerSource::Cursor,
                scope: McpServerScope::Project,
                project_id: Some(project.id),
                name: "fresh".into(),
                transport: McpTransportInput::Stdio { command: "fresh-server".into(), args: vec![], env: BTreeMap::new() },
            },
            "t",
        )
        .unwrap();
        assert_eq!(created.id, "cursor:project:fresh");

        let err = add_mcp_server(
            &paths,
            &db,
            None,
            &NewMcpServer {
                source: McpServerSource::Gemini,
                scope: McpServerScope::User,
                project_id: None,
                name: "nope".into(),
                transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
            },
            "t",
        )
        .unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(ref m) if m.contains("does not create a configuration file")), "{err}");
        assert!(!paths.agent_home().join(".gemini").exists(), "nothing was created");
    }

    /// A plugin's and Atlas's rows are never removable, and a removal takes the entry out
    /// of the file it came from without touching its neighbours.
    #[test]
    fn removing_a_server_edits_only_its_own_file() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        for id in ["plugin:acme/tools:packaged", "atlas"] {
            let err = remove_mcp_server(&paths, &db, None, id, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(ref m) if m.contains("not a server Atlas can remove")), "{id}: {err}");
        }

        remove_mcp_server(&paths, &db, None, "codex:user:playwright", "t").unwrap();
        let toml = std::fs::read_to_string(paths.agent_home().join(".codex/config.toml")).unwrap();
        assert!(!toml.contains("[mcp_servers.playwright]"), "{toml}");
        assert!(toml.contains("[mcp_servers.node_repl]") && toml.contains("model = \"gpt-5\""), "{toml}");

        remove_mcp_server(&paths, &db, None, "claude:user:user-stdio", "t").unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(paths.agent_home().join(".claude.json")).unwrap()).unwrap();
        assert!(config["mcpServers"].get("user-stdio").is_none(), "{config}");
        assert!(config["mcpServers"].get("svelte").is_some(), "{config}");

        let missing = remove_mcp_server(&paths, &db, None, "codex:user:playwright", "t").unwrap_err();
        assert!(matches!(missing, AtlasError::NotFound(_)), "{missing}");
    }

    /// Every write copies the file it is about to replace into Atlas's own home and names
    /// that backup in the audit row, so an edit made from Atlas can be undone by hand.
    /// The row itself carries what Atlas did, to which server, the path, the backup and a
    /// byte count and nothing else: an agent's config is full of tokens, and global search
    /// renders a matching row's whole detail as the hit's title.
    #[test]
    fn every_write_backs_the_file_up_and_names_it_in_the_audit_row() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let before = std::fs::read_to_string(paths.agent_home().join(".cursor/mcp.json")).unwrap();
        assert!(before.contains(SECRET), "the fixture is what makes this test mean something");
        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", false, "desktop").unwrap();

        let (actor, entity, detail): (String, String, String) = db
            .with_conn(|c| {
                Ok(c.query_row("select actor, entity, detail::varchar from audit where action = 'mcp_config_edit'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?)
            })
            .unwrap();
        assert_eq!(actor, "desktop");
        assert_eq!(entity, "mcp_server");
        assert!(!detail.contains(SECRET), "the audit row quoted the file: {detail}");
        let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(detail.as_object().unwrap().len(), 5, "only action, id, path, backup and bytes: {detail}");
        assert_eq!(detail["action"], "disable", "{detail}");
        assert_eq!(detail["id"], "cursor:user:cursor-one", "{detail}");
        assert!(detail["path"].as_str().unwrap().ends_with(".cursor/mcp.json"), "{detail}");
        assert_eq!(detail["bytes"].as_u64().unwrap(), before.len() as u64);

        // The backup is the undo: it is the file, byte for byte, under Atlas's own home.
        let backup = std::path::PathBuf::from(detail["backup"].as_str().unwrap());
        assert!(backup.starts_with(&paths.home), "{backup:?} is not under {:?}", paths.home);
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), before);
        assert!(backup.file_name().unwrap().to_string_lossy().ends_with("-cursor-mcp.json"), "{backup:?}");

        // The other two verbs name themselves too, so the trail says what Atlas did to
        // which server without anyone having to diff two backups to find out.
        add_mcp_server(
            &paths,
            &db,
            None,
            &NewMcpServer {
                source: McpServerSource::Cursor,
                scope: McpServerScope::User,
                project_id: None,
                name: "added".into(),
                transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
            },
            "desktop",
        )
        .unwrap();
        remove_mcp_server(&paths, &db, None, "cursor:user:added", "desktop").unwrap();
        let verbs: Vec<(String, String)> = db
            .with_conn(|c| {
                let mut stmt = c.prepare(
                    "select detail->>'action', detail->>'id' from audit where action = 'mcp_config_edit' order by \"at\", rowid",
                )?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            verbs,
            vec![
                ("disable".to_string(), "cursor:user:cursor-one".to_string()),
                ("add".to_string(), "cursor:user:added".to_string()),
                ("remove".to_string(), "cursor:user:added".to_string()),
            ]
        );
    }

    /// The backup directory is capped per file. `~/.claude.json` runs to hundreds of
    /// kilobytes and every toggle copies it whole, so an uncapped directory would grow
    /// with each click.
    #[test]
    fn only_the_newest_backups_of_one_file_are_kept() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        for round in 0..22 {
            set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", round % 2 == 1, "t").unwrap();
            // A backup is named by the millisecond it was taken in, and this loop is far
            // faster than that; the pause is what keeps the names apart.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let directory = paths.home.join(edit::BACKUP_DIR);
        let mut names: Vec<String> =
            std::fs::read_dir(&directory).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), edit::BACKUP_KEEP, "22 edits, the newest {} kept: {names:?}", edit::BACKUP_KEEP);
        assert!(names.iter().all(|n| n.ends_with("-cursor-mcp.json")), "{names:?}");
        // What was kept is the tail: the newest backup is the last write's.
        let newest = names.last().unwrap();
        assert!(std::fs::read_to_string(directory.join(newest)).unwrap().contains(SECRET), "the backups are still the file");
    }

    /// A configuration file that fails to parse is named by position and nothing else.
    /// `toml_edit`'s own message prints the source line under it, so a token on a broken
    /// line would otherwise reach a listing's warnings and an edit's error.
    #[test]
    fn a_parse_error_never_quotes_the_line_it_failed_on() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let codex = paths.agent_home().join(".codex/config.toml");
        std::fs::write(&codex, format!("[mcp_servers.one]\ncommand = \"x\"\ntoken = \"{SECRET}\" and then some\n")).unwrap();
        std::fs::write(
            paths.agent_home().join(".cursor/mcp.json"),
            format!("{{\"mcpServers\": {{\"one\": {{\"command\": \"x\", \"env\": {{\"TOKEN\": \"{SECRET}\"}}}}}}}} trailing\n"),
        )
        .unwrap();

        let list = list_mcp_servers(paths.agent_home(), None);
        assert_eq!(list.warnings.len(), 2, "{:?}", list.warnings);
        for warning in &list.warnings {
            assert!(!warning.contains(SECRET), "a warning quoted the broken line: {warning}");
            assert!(warning.contains("line") && warning.contains("column"), "{warning}");
        }
        assert!(list.warnings.iter().any(|w| w.contains("config.toml") && w.contains("not valid TOML")), "{:?}", list.warnings);
        assert!(list.warnings.iter().any(|w| w.contains("mcp.json") && w.contains("not valid JSON")), "{:?}", list.warnings);

        // The same file read for an edit is an error rather than a warning, and it is the
        // same sentence.
        for source in [McpServerSource::Codex, McpServerSource::Cursor] {
            let err = add_mcp_server(
                &paths,
                &db,
                None,
                &NewMcpServer {
                    source,
                    scope: McpServerScope::User,
                    project_id: None,
                    name: "nope".into(),
                    transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
                },
                "t",
            )
            .unwrap_err();
            let message = err.to_string();
            assert!(!message.contains(SECRET), "{source:?}: an error quoted the broken line: {message}");
            assert!(message.contains("line") && message.contains("column"), "{source:?}: {message}");
        }
    }

    /// A repository does not get to say where a project scope write lands. A `.mcp.json`
    /// symlinked out of the tree is refused, both for the listing that would edit it and
    /// for an add that would create one.
    #[cfg(unix)]
    #[test]
    fn a_project_config_that_resolves_outside_the_project_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let (temp, paths, project) = fixture(&db);
        let outside = temp.path().join("outside-mcp.json");
        let planted = format!("{{\"mcpServers\": {{\"outside\": {{\"command\": \"x\", \"env\": {{\"T\": \"{SECRET}\"}}}}}}}}\n");
        std::fs::write(&outside, &planted).unwrap();
        let inside = Path::new(&project.root_path).join(".mcp.json");
        std::fs::remove_file(&inside).unwrap();
        std::os::unix::fs::symlink(&outside, &inside).unwrap();

        let added = add_mcp_server(
            &paths,
            &db,
            Some(&project),
            &NewMcpServer {
                source: McpServerSource::Claude,
                scope: McpServerScope::Project,
                project_id: Some(project.id),
                name: "fresh".into(),
                transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
            },
            "t",
        )
        .unwrap_err();
        assert!(matches!(added, AtlasError::Invalid(ref m) if m.contains("resolves outside the project")), "{added}");

        // The link is followed by discovery, so the row is there to be asked for; the
        // write is where it is stopped.
        let removed = remove_mcp_server(&paths, &db, Some(&project), "claude:project:outside", "t").unwrap_err();
        assert!(matches!(removed, AtlasError::Invalid(ref m) if m.contains("resolves outside the project")), "{removed}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), planted, "nothing outside the project was written");

        // A user scope file symlinked into a dotfiles repository still writes through: it
        // is Atlas that names that path, not a checkout.
        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", false, "t").unwrap();
    }

    /// Adding to a repository that has none of an agent's project files creates the one it
    /// needs, private from its first byte, and the answer names it so a client can say
    /// where the server went.
    #[test]
    fn an_add_creates_a_missing_project_file_and_names_it() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        let root = Path::new(&project.root_path);
        std::fs::remove_file(root.join(".mcp.json")).unwrap();
        std::fs::remove_dir_all(root.join(".codex")).unwrap();
        std::fs::remove_dir_all(root.join(".cursor")).unwrap();

        for (source, relative) in [
            (McpServerSource::Claude, ".mcp.json"),
            (McpServerSource::Codex, ".codex/config.toml"),
            (McpServerSource::Cursor, ".cursor/mcp.json"),
        ] {
            let created = add_mcp_server(
                &paths,
                &db,
                Some(&project),
                &NewMcpServer {
                    source,
                    scope: McpServerScope::Project,
                    project_id: Some(project.id),
                    name: "fresh".into(),
                    transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
                },
                "t",
            )
            .unwrap();
            let path = root.join(relative);
            assert!(path.is_file(), "{relative} was not created");
            assert_eq!(created.file.as_deref(), Some(display(&path).as_str()), "{relative} is not named in the answer");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "{relative}");
            }
        }
    }

    /// Neither the backup nor the directory holding it may be readable by another local
    /// account: they are verbatim copies of the user's agent configs.
    #[cfg(unix)]
    #[test]
    fn a_backup_and_its_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", false, "t").unwrap();

        let directory = paths.home.join(edit::BACKUP_DIR);
        assert_eq!(std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777, 0o700);
        let backup = std::fs::read_dir(&directory).unwrap().flatten().next().unwrap().path();
        assert_eq!(std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777, 0o600);
    }

    /// A replaced config keeps its own mode, and never widens even for an instant: the
    /// temp file is created with that mode rather than created and then narrowed.
    #[cfg(unix)]
    #[test]
    fn a_rewritten_config_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, project) = fixture(&db);
        let cursor = paths.agent_home().join(".cursor/mcp.json");
        std::fs::set_permissions(&cursor, std::fs::Permissions::from_mode(0o600)).unwrap();
        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", false, "t").unwrap();
        assert_eq!(std::fs::metadata(&cursor).unwrap().permissions().mode() & 0o777, 0o600);

        // A file Atlas creates for a project starts private too, since it will hold that
        // agent's secrets from the first write.
        std::fs::remove_file(Path::new(&project.root_path).join(".cursor/mcp.json")).unwrap();
        add_mcp_server(
            &paths,
            &db,
            Some(&project),
            &NewMcpServer {
                source: McpServerSource::Cursor,
                scope: McpServerScope::Project,
                project_id: Some(project.id),
                name: "fresh".into(),
                transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
            },
            "t",
        )
        .unwrap();
        let created = Path::new(&project.root_path).join(".cursor/mcp.json");
        assert_eq!(std::fs::metadata(&created).unwrap().permissions().mode() & 0o777, 0o600);
    }

    /// A config the user symlinked into a dotfiles repository is written through, not
    /// replaced: the link survives and the file it points at is the one that changes.
    #[cfg(unix)]
    #[test]
    fn an_edit_writes_through_a_symlinked_config() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        let cursor = paths.agent_home().join(".cursor/mcp.json");
        let dotfiles = paths.agent_home().join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        let real = dotfiles.join("cursor-mcp.json");
        std::fs::rename(&cursor, &real).unwrap();
        std::os::unix::fs::symlink(&real, &cursor).unwrap();

        set_mcp_server_enabled(&paths, &db, None, "cursor:user:cursor-one", false, "t").unwrap();
        assert!(std::fs::symlink_metadata(&cursor).unwrap().file_type().is_symlink(), "the link was replaced");
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["cursor-one"]["disabled"], true, "the real file is what changed");
    }

    /// A name that could split an id, or an unmanageable one, is refused before anything
    /// is written.
    #[test]
    fn a_name_that_would_break_an_id_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        for name in ["", "with:colon", "with/slash", "with\nnewline"] {
            let err = add_mcp_server(
                &paths,
                &db,
                None,
                &NewMcpServer {
                    source: McpServerSource::Cursor,
                    scope: McpServerScope::User,
                    project_id: None,
                    name: name.into(),
                    transport: McpTransportInput::Stdio { command: "x".into(), args: vec![], env: BTreeMap::new() },
                },
                "t",
            )
            .unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(_)), "{name:?}: {err}");
        }
    }

    /// An id nobody listed resolves to nothing rather than to a path.
    #[test]
    fn an_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, paths, _project) = fixture(&db);
        for id in ["nope", "claude:user:../../etc/passwd", "cursor:user:cursor-one/../x"] {
            assert!(matches!(find(paths.agent_home(), None, id), Err(AtlasError::NotFound(_))), "{id}");
        }
    }

    /// A `Resolved` for a command a check test wants to run, with nothing else filled in.
    fn stdio_probe(command: &str, args: &[&str]) -> Resolved {
        Resolved {
            entry: McpServerEntry {
                id: "test".into(),
                name: "test".into(),
                source: McpServerSource::Cursor,
                scope: McpServerScope::User,
                transport: McpTransport::Stdio {
                    command: command.into(),
                    args: args.iter().map(|a| a.to_string()).collect(),
                    env_keys: vec![],
                },
                file: None,
                plugin: None,
                enabled: true,
                can_toggle: false,
                can_remove: false,
                is_atlas: false,
                project_id: None,
            },
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            cwd: None,
        }
    }

    /// The cap the real check runs under. Only the value differs from a live check, so a
    /// test can prove the timeout path without sitting through fifteen seconds.
    #[test]
    fn the_check_cap_is_fifteen_seconds() {
        assert_eq!(check::CHECK_TIMEOUT, std::time::Duration::from_secs(15));
    }

    /// A server that starts and then says nothing ends on the cap with an answer, not an
    /// error, and does not outlive the check: the process is gone afterwards.
    ///
    /// Multi-threaded on purpose. The kill is spawned as a task by the transport's `Drop`,
    /// and on a current-thread runtime a test that then waits for the process would be
    /// holding the only thread the kill could run on.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_server_that_never_answers_times_out_and_its_process_is_killed() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("pid");
        // `exec` so the recorded pid is the child Atlas spawned, not a shell that forked
        // it: the kill under test is of that process.
        let script = format!("echo $$ > {}; exec sleep 60", pid_file.display());
        let resolved = stdio_probe("sh", &["-c", &script]);

        let result = check::run_with_timeout(&resolved, std::time::Duration::from_millis(600)).await;
        assert!(!result.ok, "{result:?}");
        assert!(result.error.as_deref().unwrap().contains("did not answer within"), "{result:?}");
        assert!(result.elapsed_ms >= 500, "it ended on the cap, not early: {result:?}");

        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        // The kill is spawned on its own task by the transport's `Drop`, so it is polled
        // for rather than asserted the instant the check returns.
        let mut gone = false;
        for _ in 0..50 {
            if !process_is_alive(pid) {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(gone, "the child outlived its check: pid {pid}");
    }

    /// Whether `pid` is still a running process. A killed child lingers as a zombie until
    /// it is reaped, which is not "alive" for this purpose.
    #[cfg(unix)]
    fn process_is_alive(pid: i32) -> bool {
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output();
        match out {
            Ok(out) => {
                let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
                !stat.is_empty() && !stat.starts_with('Z')
            }
            Err(_) => false,
        }
    }

    /// A command that is not there is a failed check with a reason, not an error.
    #[tokio::test]
    async fn a_command_that_does_not_exist_is_a_failed_check() {
        let resolved = stdio_probe("atlas-no-such-command-exists", &[]);
        let result = check::run(&resolved).await;
        assert!(!result.ok, "{result:?}");
        assert!(result.error.unwrap().contains("atlas-no-such-command-exists"));
        assert!(result.tools.is_empty());
    }
}
