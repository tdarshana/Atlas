use std::sync::Arc;
use atlas_core::backend::LocalBackend;
use crate::mcp_clients::ClientRegistry;
use crate::plugin_tools::PluginToolChannel;

#[derive(Clone)]
pub struct AppState {
    pub backend: Arc<LocalBackend>,
    pub mcp_clients: Arc<ClientRegistry>,
    /// The same channel the backend's `PluginToolHost` points at, so the registration
    /// routes and the WebSocket upgrade reach it without going through `Backend`.
    pub plugin_tools: Arc<PluginToolChannel>,
    /// Flips to true when the daemon is stopping, so every long-lived response (the
    /// change stream, the plugin channel) ends and the graceful shutdown can finish.
    pub shutdown: tokio::sync::watch::Receiver<bool>,
    /// The secret this daemon and its clients share (SEC-5): 32 random bytes as hex,
    /// minted at start and written to `daemon.json`. Every `/api/v1` request presents it
    /// and `status` answers with its SHA-256 so a client can tell atlasd from anything
    /// else on the port.
    pub token: Arc<str>,
}

#[cfg(test)]
impl AppState {
    /// A state around an already-open backend with no MCP clients, no plugin channel
    /// traffic and no shutdown pending: what the in-process handler tests need to run
    /// the router with `oneshot`.
    pub fn in_process(backend: Arc<LocalBackend>, token: Arc<str>) -> Self {
        let (_tx, shutdown) = tokio::sync::watch::channel(false);
        Self { backend, mcp_clients: Arc::new(ClientRegistry::new()), plugin_tools: Arc::new(PluginToolChannel::new()), shutdown, token }
    }
}
