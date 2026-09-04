//! The `mcpServers` JSON block, which Claude Code, Cursor, Gemini CLI and Windsurf all
//! share, and which Claude Code plugins ship in their own `.mcp.json`.
//!
//! One entry is either `{ "command": ..., "args": [...], "env": {...} }` or
//! `{ "type": "http" | "sse", "url": ..., "headers": {...} }`; Gemini CLI spells the
//! second one's key `httpUrl`, which is read as the same thing so a valid Gemini server
//! is listed rather than warned about. Cursor adds `"disabled": true`. Anything else is
//! skipped with a warning naming the server, since an entry Atlas cannot describe is
//! better said out loud than silently missing.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use uuid::Uuid;

use crate::models::{McpServerEntry, McpServerScope, McpServerSource, McpTransport};

use super::{Found, Resolved};

/// One parsed entry: how to reach the server, the secret values its file holds, and
/// whether the entry switches itself off.
pub struct ParsedEntry {
    pub transport: McpTransport,
    pub env: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    /// Cursor's own `disabled: true`.
    pub disabled: bool,
}

/// `"<source>:<scope>:<name>"`, with `scope_part` carrying `"<marketplace>/<plugin>"`
/// instead of the scope word for a plugin server.
pub fn server_id(source: McpServerSource, scope_part: &str, name: &str) -> String {
    format!("{}:{scope_part}:{name}", source.as_str())
}

/// Everything one `mcpServers` object holds, in the order the file lists it.
///
/// `enabled` is asked per server rather than passed in, so the caller can consult the
/// agent's own switch (Claude Code's per-project disabled lists, a plugin's enabled flag)
/// without this function knowing anything about it. An entry's own `disabled: true` is
/// applied on top: Cursor's switch lives in the entry itself.
#[allow(clippy::too_many_arguments)]
pub fn collect(
    servers: &Value,
    source: McpServerSource,
    scope: McpServerScope,
    scope_part: &str,
    file: &Path,
    project_id: Option<Uuid>,
    plugin: Option<&str>,
    can_toggle: bool,
    can_remove: bool,
    enabled: &dyn Fn(&str) -> bool,
    found: &mut Found,
) {
    let Some(map) = servers.as_object() else {
        if !servers.is_null() {
            found.warnings.push(format!("{}: mcpServers is not an object", file.display()));
        }
        return;
    };
    let path = super::display(file);
    for (name, value) in map {
        let Some(parsed) = parse_entry(value) else {
            found.warnings.push(format!("{path}: server '{name}' has neither a command nor a url"));
            continue;
        };
        found.servers.push(Resolved {
            entry: McpServerEntry {
                id: server_id(source, scope_part, name),
                name: name.clone(),
                source,
                scope,
                transport: parsed.transport,
                file: Some(path.clone()),
                plugin: plugin.map(str::to_string),
                enabled: enabled(name) && !parsed.disabled,
                can_toggle,
                can_remove,
                is_atlas: false,
                project_id,
            },
            env: parsed.env,
            headers: parsed.headers,
            // Only Codex configures a working directory, and it has its own reader.
            cwd: None,
        });
    }
}

/// One `mcpServers` value. `None` when it is neither an HTTP nor a stdio server, which
/// is the caller's cue to warn rather than to invent a shape.
pub fn parse_entry(value: &Value) -> Option<ParsedEntry> {
    let object = value.as_object()?;
    let disabled = object.get("disabled").and_then(Value::as_bool).unwrap_or(false);
    // The URL is checked first: an entry carrying both is an HTTP server whose author
    // left a stale command behind, and the URL is what the agent will use. `httpUrl` is
    // Gemini CLI's spelling of the same key.
    if let Some(url) = object.get("url").or_else(|| object.get("httpUrl")).and_then(Value::as_str) {
        let headers = string_map(object.get("headers"));
        return Some(ParsedEntry {
            transport: McpTransport::Http { url: url.to_string(), header_keys: headers.keys().cloned().collect() },
            env: BTreeMap::new(),
            headers,
            disabled,
        });
    }
    let command = object.get("command").and_then(Value::as_str)?;
    let args = object
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let env = string_map(object.get("env"));
    Some(ParsedEntry {
        transport: McpTransport::Stdio { command: command.to_string(), args, env_keys: env.keys().cloned().collect() },
        env,
        headers: BTreeMap::new(),
        disabled,
    })
}

/// A JSON object read as `String -> String`. Non-string values are dropped rather than
/// stringified: an agent would not pass them to a process either.
fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default()
}

/// The `mcpServers` block of one JSON file, parsed. `None` when the file is absent,
/// unreadable or not JSON; the last two are warnings.
pub fn read_file(path: &Path, found: &mut Found) -> Option<Value> {
    let text = super::read_config(path, &mut found.warnings)?;
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            found.warnings.push(super::json_error(path, &e));
            None
        }
    }
}
