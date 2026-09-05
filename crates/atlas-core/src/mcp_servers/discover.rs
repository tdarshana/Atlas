//! Walks every agent's configuration and returns one normalised list.
//!
//! Read-only. Nothing here creates a file, and every path it opens is built from the home
//! it was handed or from a project's own recorded root, never from a caller's string. An
//! agent that is not installed contributes nothing and says nothing; a file that exists
//! but cannot be read or parsed becomes a warning, so an unreadable config never looks
//! identical to an absent one.

use std::collections::BTreeMap;
use std::path::Path;

use crate::models::{McpServerEntry, McpServerScope, McpServerSource, McpTransport, Project};

use super::{claude, codex, generic_json, Found, Resolved};

/// Atlas's own entry, which no file declares.
pub const ATLAS_ID: &str = "atlas";

/// Every server that applies. Without a project this is the user level of every agent,
/// the installed plugins and Atlas. With one it is that project's own files, plus the
/// plugins and Atlas, which apply wherever the user works.
pub fn discover(home: &Path, project: Option<&Project>) -> Found {
    let mut found = Found::default();
    match project {
        None => {
            claude::user_servers(home, &mut found);
            codex::servers(&home.join(".codex/config.toml"), McpServerScope::User, None, &mut found);
            json_agent(
                &home.join(".cursor/mcp.json"),
                McpServerSource::Cursor,
                McpServerScope::User,
                None,
                true,
                &mut found,
            );
            json_agent(
                &home.join(".gemini/settings.json"),
                McpServerSource::Gemini,
                McpServerScope::User,
                None,
                false,
                &mut found,
            );
            json_agent(
                &home.join(".codeium/windsurf/mcp_config.json"),
                McpServerSource::Windsurf,
                McpServerScope::User,
                None,
                false,
                &mut found,
            );
        }
        Some(p) => {
            let root = Path::new(&p.root_path);
            claude::project_servers(home, p, &mut found);
            claude::local_servers(home, p, &mut found);
            codex::servers(&root.join(".codex/config.toml"), McpServerScope::Project, Some(p.id), &mut found);
            json_agent(
                &root.join(".cursor/mcp.json"),
                McpServerSource::Cursor,
                McpServerScope::Project,
                Some(p.id),
                true,
                &mut found,
            );
        }
    }
    claude::plugin_servers(home, project, &mut found);
    found.servers.push(atlas_entry());
    found.servers.sort_by(|a, b| a.entry.name.cmp(&b.entry.name).then_with(|| a.entry.id.cmp(&b.entry.id)));
    found
}

/// Cursor, Gemini CLI and Windsurf all keep a plain `mcpServers` object. Cursor is the
/// only one of the three with a switch of its own (`disabled: true` inside the entry),
/// which [`generic_json::parse_entry`] reads; for the other two the toggle is absent and
/// Remove is the way to stop a server.
fn json_agent(
    path: &Path,
    source: McpServerSource,
    scope: McpServerScope,
    project_id: Option<uuid::Uuid>,
    can_toggle: bool,
    found: &mut Found,
) {
    let Some(file) = generic_json::read_file(path, found) else { return };
    generic_json::collect(
        &file["mcpServers"],
        source,
        scope,
        scope.as_str(),
        path,
        project_id,
        None,
        can_toggle,
        true,
        &|_| true,
        found,
    );
}

/// Atlas's own MCP server, as agents launch it. Synthesised rather than read, because the
/// entry that matters is the one the user's agents run (`atlas mcp`), not whichever of
/// their config files happens to name it; it is listed everywhere, and Atlas never edits
/// itself out of its own list.
fn atlas_entry() -> Resolved {
    Resolved {
        entry: McpServerEntry {
            id: ATLAS_ID.to_string(),
            name: "atlas".to_string(),
            source: McpServerSource::Atlas,
            scope: McpServerScope::User,
            transport: McpTransport::Stdio { command: "atlas".into(), args: vec!["mcp".into()], env_keys: Vec::new() },
            file: None,
            plugin: None,
            enabled: true,
            can_toggle: false,
            can_remove: false,
            is_atlas: true,
            project_id: None,
        },
        env: BTreeMap::new(),
        headers: BTreeMap::new(),
        cwd: None,
    }
}
