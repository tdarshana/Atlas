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
use std::path::Path;

use crate::db::Db;
use crate::models::{McpCheckResult, McpServerEntry, McpServerList, NewMcpServer, Project};
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
    db: &Db,
    home: &Path,
    project: Option<&Project>,
    id: &str,
    enabled: bool,
    actor: &str,
) -> Result<McpServerEntry> {
    let resolved = find(home, project, id)?;
    edit::set_enabled(db, home, project, &resolved.entry, enabled, actor)?;
    find(home, project, id).map(|r| r.entry)
}

/// Writes a new server into one agent's configuration. Refuses a name the target file
/// already holds rather than replacing it.
pub fn add_mcp_server(db: &Db, home: &Path, project: Option<&Project>, input: &NewMcpServer, actor: &str) -> Result<McpServerEntry> {
    let id = edit::add_server(db, home, project, input, actor)?;
    find(home, project, &id).map(|r| r.entry)
}

/// Deletes a server from the file it came from.
pub fn remove_mcp_server(db: &Db, home: &Path, project: Option<&Project>, id: &str, actor: &str) -> Result<()> {
    let resolved = find(home, project, id)?;
    edit::remove_server(db, project, &resolved.entry, actor)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{McpServerScope, McpServerSource, McpTransport, McpTransportInput};
    use crate::projects::ProjectRepo;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// The value planted in every `env` and `headers` block in the fixtures. No listing,
    /// no entry and no error may ever carry it.
    const SECRET: &str = "SECRET-DO-NOT-LEAK";

    /// Copies the fixture home and project into a temp directory, points the project
    /// block in `.claude.json` at the copied root, and registers the project.
    ///
    /// Copied rather than read in place, because every edit test writes, and the real
    /// `~/.claude.json` and `~/.codex/config.toml` must never be within reach: nothing
    /// here ever reads a path the fixture did not put there.
    fn fixture(db: &Db) -> (tempfile::TempDir, PathBuf, Project) {
        let temp = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_servers");
        let home = temp.path().join("home");
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
        (temp, home, project)
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
        let (_temp, home, _project) = fixture(&db);
        let list = list_mcp_servers(&home, None);

        assert_eq!(
            ids(&list),
            vec![
                "atlas",
                "codex:user:computer-use",
                "cursor:user:cursor-off",
                "cursor:user:cursor-one",
                "gemini:user:gemini-one",
                "codex:user:node_repl",
                "plugin:acme/tools:packaged",
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
        let (_temp, home, project) = fixture(&db);
        let list = list_mcp_servers(&home, Some(&project));

        assert_eq!(
            ids(&list),
            vec![
                "claude:project:approved",
                "atlas",
                "plugin:acme/tools:packaged",
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
        assert!(list.servers.iter().any(|s| s.id == "plugin:acme/tools:packaged"));
    }

    /// The one rule that would be worst to get wrong: a token in an agent's config never
    /// reaches a caller, in any field, in either listing.
    #[test]
    fn no_secret_value_ever_leaves_the_daemon() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, project) = fixture(&db);
        for list in [list_mcp_servers(&home, None), list_mcp_servers(&home, Some(&project))] {
            let json = serde_json::to_string(&list).unwrap();
            assert!(!json.contains(SECRET), "a secret reached the wire: {json}");
            // The key names do come through, which is what a client needs to show.
            assert!(json.contains("env_keys") || json.contains("header_keys"), "{json}");
        }
        let global = serde_json::to_string(&list_mcp_servers(&home, None)).unwrap();
        assert!(global.contains("NODE_TOKEN"), "the key name is what a listing shows: {global}");
    }

    /// A half-finished plugin install is not a version, and a disabled plugin's servers
    /// list switched off rather than vanishing.
    #[test]
    fn a_scratch_plugin_directory_is_skipped() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        let list = list_mcp_servers(&home, None);
        assert!(list.servers.iter().all(|s| s.name != "half-installed"), "{:?}", ids(&list));
    }

    /// An unreadable or unparsable file is one warning, not a failed listing.
    #[test]
    fn a_broken_config_is_a_warning() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        std::fs::write(home.join(".cursor/mcp.json"), "{ this is not json").unwrap();
        let list = list_mcp_servers(&home, None);
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
        let (_temp, home, project) = fixture(&db);
        let project = Some(&project);

        set_mcp_server_enabled(&db, &home, project, "claude:local:playwright", false, "t").unwrap();
        assert!(!entry(&list_mcp_servers(&home, project), "claude:local:playwright").enabled);
        set_mcp_server_enabled(&db, &home, project, "claude:local:playwright", true, "t").unwrap();
        assert!(entry(&list_mcp_servers(&home, project), "claude:local:playwright").enabled);

        set_mcp_server_enabled(&db, &home, project, "claude:project:unapproved", true, "t").unwrap();
        let list = list_mcp_servers(&home, project);
        assert!(entry(&list, "claude:project:unapproved").enabled, "approving it is what turns it on");
        assert!(entry(&list, "claude:project:approved").enabled, "the other one is untouched");

        // The rest of `~/.claude.json` survived four rewrites.
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(config["numStartups"], 7);
        assert_eq!(config["mcpServers"]["svelte"]["headers"]["Authorization"], format!("Bearer {SECRET}"));
    }

    /// Cursor's switch is the entry's own `disabled`, and an enabled server carries no
    /// key at all rather than `"disabled": false`.
    #[test]
    fn cursor_enable_and_disable_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        set_mcp_server_enabled(&db, &home, None, "cursor:user:cursor-one", false, "t").unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join(".cursor/mcp.json")).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["cursor-one"]["disabled"], true);

        set_mcp_server_enabled(&db, &home, None, "cursor:user:cursor-one", true, "t").unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join(".cursor/mcp.json")).unwrap()).unwrap();
        assert!(config["mcpServers"]["cursor-one"].get("disabled").is_none(), "{config}");
        assert_eq!(config["mcpServers"]["cursor-one"]["env"]["CURSOR_TOKEN"], SECRET, "the value stayed in its own file");
    }

    /// Where the agent has no switch, the toggle refuses with the one message every
    /// caller says, and nothing is written.
    #[test]
    fn an_agent_with_no_switch_refuses_the_toggle() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        for id in ["gemini:user:gemini-one", "windsurf:user:windsurf-one", "plugin:acme/tools:packaged", "atlas", "claude:user:svelte"] {
            let err = set_mcp_server_enabled(&db, &home, None, id, false, "t").unwrap_err();
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
        let (_temp, home, _project) = fixture(&db);
        let before = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();

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
        let added = add_mcp_server(&db, &home, None, &input, "t").unwrap();
        assert_eq!(added.id, "codex:user:added");
        assert_eq!(added.transport, McpTransport::Stdio { command: "added-server".into(), args: vec!["--go".into()], env_keys: vec!["ADDED_TOKEN".into()] });

        let after = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(after.starts_with(&before), "everything before the addition is unchanged:\n{after}");
        assert!(after.contains("# Codex's own file, comments and all."), "{after}");
        assert!(after.contains("[mcp_servers.added]"), "{after}");
        assert!(after.contains("ADDED_TOKEN"), "the value is written into the file it belongs in");

        // A second add of the same name is a conflict, not a replacement.
        let err = add_mcp_server(&db, &home, None, &input, "t").unwrap_err();
        assert!(matches!(err, AtlasError::Conflict(_)), "{err}");
    }

    /// A JSON add keeps the existing keys and their order, and appends the new server.
    #[test]
    fn adding_a_json_server_keeps_the_existing_keys() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
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
        let added = add_mcp_server(&db, &home, None, &input, "t").unwrap();
        assert_eq!(added.id, "claude:user:added");
        assert_eq!(added.transport, McpTransport::Http { url: "https://added.example/mcp".into(), header_keys: vec!["Authorization".into()] });

        let text = std::fs::read_to_string(home.join(".claude.json")).unwrap();
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
        let (_temp, home, project) = fixture(&db);
        std::fs::remove_file(Path::new(&project.root_path).join(".cursor/mcp.json")).unwrap();
        std::fs::remove_dir_all(home.join(".gemini")).unwrap();

        let created = add_mcp_server(
            &db,
            &home,
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
            &db,
            &home,
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
        assert!(!home.join(".gemini").exists(), "nothing was created");
    }

    /// A plugin's and Atlas's rows are never removable, and a removal takes the entry out
    /// of the file it came from without touching its neighbours.
    #[test]
    fn removing_a_server_edits_only_its_own_file() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        for id in ["plugin:acme/tools:packaged", "atlas"] {
            let err = remove_mcp_server(&db, &home, None, id, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(ref m) if m.contains("not a server Atlas can remove")), "{id}: {err}");
        }

        remove_mcp_server(&db, &home, None, "codex:user:playwright", "t").unwrap();
        let toml = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(!toml.contains("[mcp_servers.playwright]"), "{toml}");
        assert!(toml.contains("[mcp_servers.node_repl]") && toml.contains("model = \"gpt-5\""), "{toml}");

        remove_mcp_server(&db, &home, None, "claude:user:user-stdio", "t").unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
        assert!(config["mcpServers"].get("user-stdio").is_none(), "{config}");
        assert!(config["mcpServers"].get("svelte").is_some(), "{config}");

        let missing = remove_mcp_server(&db, &home, None, "codex:user:playwright", "t").unwrap_err();
        assert!(matches!(missing, AtlasError::NotFound(_)), "{missing}");
    }

    /// Every write leaves an audit row carrying the file's previous text, so an edit made
    /// from Atlas can be undone by hand.
    #[test]
    fn every_write_records_the_previous_text() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        let before = std::fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
        set_mcp_server_enabled(&db, &home, None, "cursor:user:cursor-one", false, "desktop").unwrap();

        let (actor, entity, detail): (String, String, String) = db
            .with_conn(|c| {
                Ok(c.query_row("select actor, entity, detail::varchar from audit where action = 'mcp_config_edit'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?)
            })
            .unwrap();
        assert_eq!(actor, "desktop");
        assert_eq!(entity, "mcp_server");
        let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(detail["action"], "set_enabled");
        assert_eq!(detail["id"], "cursor:user:cursor-one");
        assert!(detail["path"].as_str().unwrap().ends_with(".cursor/mcp.json"), "{detail}");
        assert_eq!(detail["previous"].as_str().unwrap(), before, "the whole previous file is kept");
        assert_eq!(detail["previous_truncated"], false);
    }

    /// A name that could split an id, or an unmanageable one, is refused before anything
    /// is written.
    #[test]
    fn a_name_that_would_break_an_id_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let (_temp, home, _project) = fixture(&db);
        for name in ["", "with:colon", "with/slash", "with\nnewline"] {
            let err = add_mcp_server(
                &db,
                &home,
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
        let (_temp, home, _project) = fixture(&db);
        for id in ["nope", "claude:user:../../etc/passwd", "cursor:user:cursor-one/../x"] {
            assert!(matches!(find(&home, None, id), Err(AtlasError::NotFound(_))), "{id}");
        }
    }

    /// A check runs the user's own command. This one starts a shell that says nothing and
    /// never exits, so the check has to end on its own timeout rather than hanging, and
    /// the failure has to be an answer rather than an error.
    #[tokio::test]
    async fn a_server_that_never_answers_times_out() {
        let resolved = Resolved {
            entry: McpServerEntry {
                id: "test".into(),
                name: "test".into(),
                source: McpServerSource::Cursor,
                scope: McpServerScope::User,
                transport: McpTransport::Stdio { command: "sleep".into(), args: vec!["60".into()], env_keys: vec![] },
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
        };
        // The real cap is 15 s and this test would sit through it; the same path is
        // exercised at a length a test suite can afford.
        let result = tokio::time::timeout(std::time::Duration::from_millis(400), check::run(&resolved)).await;
        assert!(result.is_err(), "a silent server must not answer early: {result:?}");
    }

    /// A command that is not there is a failed check with a reason, not an error.
    #[tokio::test]
    async fn a_command_that_does_not_exist_is_a_failed_check() {
        let resolved = Resolved {
            entry: McpServerEntry {
                id: "test".into(),
                name: "test".into(),
                source: McpServerSource::Cursor,
                scope: McpServerScope::User,
                transport: McpTransport::Stdio {
                    command: "atlas-no-such-command-exists".into(),
                    args: vec![],
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
        };
        let result = check::run(&resolved).await;
        assert!(!result.ok, "{result:?}");
        assert!(result.error.unwrap().contains("atlas-no-such-command-exists"));
        assert!(result.tools.is_empty());
    }
}
