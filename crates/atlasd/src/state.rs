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
}
