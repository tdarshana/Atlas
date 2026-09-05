//! The registry of MCP clients currently connected to this daemon, either directly
//! (stdio shims registering themselves over the API) or through the HTTP transport
//! (whose sessions are picked up on their first tool call; see `atlas_mcp::OnToolCall`).
//! Served by `GET /api/v1/mcp/status`.
use std::collections::HashMap;
use std::sync::Mutex;
use chrono::Utc;
use uuid::Uuid;

/// The row and its transport are `atlas_core` models, so the desktop's generated types
/// cover `GET /api/v1/mcp/status`. `last_project_id` is the project this client's last
/// tool call resolved, best effort: only the HTTP transport (`record_http_call`) reports
/// one, since it comes from the router's own project resolution on that call.
pub use atlas_core::models::{McpClient, McpClientTransport as Transport};

/// An entry with no heartbeat in this long is dropped from `ClientRegistry::live`.
const STALE_AFTER_MINUTES: i64 = 10;

#[derive(Default)]
pub struct ClientRegistry {
    clients: Mutex<HashMap<String, McpClient>>,
}

impl ClientRegistry {
    pub fn new() -> Self { Self::default() }

    /// The stdio shim's explicit registration (`POST /api/v1/mcp/clients`) on start.
    /// Upserts on `id`, so a caller that registers twice with the same id (a shim
    /// that races its own retry) ends up with one entry, not two.
    pub fn register(&self, id: String, transport: Transport, client_name: String, client_version: Option<String>) -> McpClient {
        let now = Utc::now();
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        let entry = clients.entry(id.clone()).or_insert_with(|| McpClient {
            id,
            transport,
            client_name: client_name.clone(),
            client_version: client_version.clone(),
            first_seen: now,
            last_seen: now,
            tool_calls: 0,
            last_project_id: None,
        });
        entry.last_seen = now;
        entry.transport = transport;
        entry.client_name = client_name;
        entry.client_version = client_version;
        entry.clone()
    }

    /// The stdio shim's periodic heartbeat (`PUT /api/v1/mcp/clients/{id}`, every
    /// 60 s): bumps `last_seen` and sets `tool_calls` to the total the shim reports,
    /// which it tracks itself since a daemon restart must not lose the running count.
    /// `None` for an id nobody registered (a stale id from a daemon that restarted
    /// since, most likely), which the route reports as 404.
    pub fn heartbeat(&self, id: &str, tool_calls: u64) -> Option<McpClient> {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        let entry = clients.get_mut(id)?;
        entry.last_seen = Utc::now();
        entry.tool_calls = tool_calls;
        Some(entry.clone())
    }

    /// An HTTP MCP session's tool call: registers the session on its first call and
    /// bumps `tool_calls` and `last_seen` on every one after. There is no `initialize`
    /// hook to register from instead; see `atlas_mcp::OnToolCall`. `project_id` is
    /// whatever the router resolved this call's project to (or `None`), and always
    /// overwrites `last_project_id`: the field tracks the *last* call, not the first.
    pub fn record_http_call(&self, session_id: String, client_name: String, client_version: Option<String>, project_id: Option<Uuid>) {
        let now = Utc::now();
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        let entry = clients.entry(session_id.clone()).or_insert_with(|| McpClient {
            id: session_id,
            transport: Transport::Http,
            client_name: client_name.clone(),
            client_version: client_version.clone(),
            first_seen: now,
            last_seen: now,
            tool_calls: 0,
            last_project_id: None,
        });
        entry.last_seen = now;
        entry.tool_calls += 1;
        entry.last_project_id = project_id;
        if !client_name.is_empty() {
            entry.client_name = client_name;
        }
        if client_version.is_some() {
            entry.client_version = client_version;
        }
    }

    /// The stdio shim's best-effort unregister on exit (`DELETE
    /// /api/v1/mcp/clients/{id}`). Idempotent: removing an id nobody holds is not an
    /// error, since the caller cannot tell the difference from here.
    pub fn unregister(&self, id: &str) {
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    /// The clients to show in `GET /api/v1/mcp/status`: every entry heartbeated (or
    /// called, for an HTTP session) within `STALE_AFTER_MINUTES`, oldest first. Also
    /// prunes anything older from the map, so a client that vanished without
    /// unregistering does not linger forever.
    pub fn live(&self) -> Vec<McpClient> {
        let cutoff = Utc::now() - chrono::Duration::minutes(STALE_AFTER_MINUTES);
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        clients.retain(|_, c| c.last_seen >= cutoff);
        let mut out: Vec<_> = clients.values().cloned().collect();
        out.sort_by(|a, b| a.first_seen.cmp(&b.first_seen));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_upserts_and_heartbeat_sets_the_reported_call_count() {
        let reg = ClientRegistry::new();
        let created = reg.register("a".into(), Transport::Stdio, "claude-code".into(), None);
        assert_eq!(created.tool_calls, 0);
        let updated = reg.heartbeat("a", 3).unwrap();
        assert_eq!(updated.tool_calls, 3);
        assert_eq!(updated.client_name, "claude-code");
        let live = reg.live();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].tool_calls, 3);
    }

    #[test]
    fn heartbeat_of_an_unknown_id_is_none() {
        let reg = ClientRegistry::new();
        assert!(reg.heartbeat("nobody", 1).is_none());
    }

    #[test]
    fn record_http_call_registers_on_first_call_and_counts_every_one() {
        let reg = ClientRegistry::new();
        reg.record_http_call("sess-1".into(), "codex".into(), Some("1.0".into()), None);
        reg.record_http_call("sess-1".into(), "codex".into(), Some("1.0".into()), None);
        reg.record_http_call("sess-1".into(), "codex".into(), Some("1.0".into()), None);
        let live = reg.live();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].tool_calls, 3);
        assert_eq!(live[0].transport, Transport::Http);
        assert_eq!(live[0].client_name, "codex");
    }

    /// The registry's `last_project_id` tracks the *last* call, not the first: a call
    /// with no project resolves clears it, matching `Task MCP-A`'s "last call touched
    /// this project" contract for `GET /api/v1/projects/{id}/mcp`.
    #[test]
    fn record_http_call_tracks_the_last_resolved_project() {
        let reg = ClientRegistry::new();
        let p = Uuid::new_v4();
        reg.record_http_call("sess-1".into(), "codex".into(), None, Some(p));
        assert_eq!(reg.live()[0].last_project_id, Some(p));
        reg.record_http_call("sess-1".into(), "codex".into(), None, None);
        assert_eq!(reg.live()[0].last_project_id, None);
    }

    #[test]
    fn stale_entries_drop_from_live_but_fresh_ones_stay() {
        let reg = ClientRegistry::new();
        reg.register("fresh".into(), Transport::Stdio, "cli".into(), None);
        reg.register("stale".into(), Transport::Stdio, "cli".into(), None);
        {
            // Real time cannot be rewound in a unit test, so the staleness is forced
            // directly on the entry rather than by waiting ten minutes.
            let mut clients = reg.clients.lock().unwrap();
            clients.get_mut("stale").unwrap().last_seen = Utc::now() - chrono::Duration::minutes(STALE_AFTER_MINUTES + 1);
        }
        let live = reg.live();
        assert_eq!(live.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["fresh"]);
    }

    #[test]
    fn unregister_removes_immediately() {
        let reg = ClientRegistry::new();
        reg.register("a".into(), Transport::Stdio, "cli".into(), None);
        reg.unregister("a");
        assert!(reg.live().is_empty());
        // Removing an id nobody holds is not an error.
        reg.unregister("a");
    }
}
