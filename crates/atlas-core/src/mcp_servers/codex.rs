//! Codex's `config.toml`, at `~/.codex/config.toml` for the user and
//! `<root>/.codex/config.toml` for one repository.
//!
//! Each server is a `[mcp_servers.<name>]` table with `command` and `args`, an
//! `[mcp_servers.<name>.env]` sub-table, or a `url` for an HTTP server. Codex carries its
//! own switch in the table, `enabled = false`, which is why a Codex row can be toggled
//! rather than only removed.
//!
//! Read through `toml_edit` rather than `toml`, because every write below has to preserve
//! the user's comments and layout and there is no reason for two TOML parsers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Value as TomlValue};
use uuid::Uuid;

use crate::models::{McpServerEntry, McpServerScope, McpServerSource, McpTransport};

use super::generic_json::server_id;
use super::{Found, Resolved};

/// The table every server hangs off.
pub const TABLE: &str = "mcp_servers";

/// Every `[mcp_servers.*]` table in one `config.toml`.
pub fn servers(path: &Path, scope: McpServerScope, project_id: Option<Uuid>, found: &mut Found) {
    let Some(doc) = read_document(path, found) else { return };
    let Some(table) = doc.get(TABLE).and_then(Item::as_table_like) else { return };
    let display = super::display(path);
    for (name, item) in table.iter() {
        let Some(entry) = item.as_table_like() else {
            found.warnings.push(format!("{display}: server '{name}' is not a table"));
            continue;
        };
        let env: BTreeMap<String, String> = entry
            .get("env")
            .and_then(Item::as_table_like)
            .map(|e| e.iter().filter_map(|(k, v)| string(v).map(|s| (k.to_string(), s))).collect())
            .unwrap_or_default();
        let transport = match entry.get("url").and_then(string) {
            Some(url) => McpTransport::Http { url, header_keys: Vec::new() },
            None => match entry.get("command").and_then(string) {
                Some(command) => McpTransport::Stdio {
                    command,
                    args: entry
                        .get("args")
                        .and_then(Item::as_array)
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                    env_keys: env.keys().cloned().collect(),
                },
                None => {
                    found.warnings.push(format!("{display}: server '{name}' has neither a command nor a url"));
                    continue;
                }
            },
        };
        found.servers.push(Resolved {
            entry: McpServerEntry {
                id: server_id(McpServerSource::Codex, scope.as_str(), name),
                name: name.to_string(),
                source: McpServerSource::Codex,
                scope,
                transport,
                file: Some(display.clone()),
                plugin: None,
                enabled: entry.get("enabled").and_then(|v| v.as_bool()) != Some(false),
                can_toggle: true,
                can_remove: true,
                is_atlas: false,
                project_id,
            },
            env,
            headers: BTreeMap::new(),
            // Codex starts a server here, so a relative `command` only resolves with it.
            cwd: entry.get("cwd").and_then(string).map(PathBuf::from),
        });
    }
}

/// One `config.toml` parsed with its formatting intact. A file that is not TOML is a
/// warning, never a failed listing.
pub fn read_document(path: &Path, found: &mut Found) -> Option<DocumentMut> {
    let text = super::read_config(path, &mut found.warnings)?;
    match text.parse::<DocumentMut>() {
        Ok(doc) => Some(doc),
        Err(e) => {
            found.warnings.push(super::toml_error(path, &text, &e));
            None
        }
    }
}

fn string(item: &Item) -> Option<String> {
    match item.as_value() {
        Some(TomlValue::String(s)) => Some(s.value().to_string()),
        _ => None,
    }
}
