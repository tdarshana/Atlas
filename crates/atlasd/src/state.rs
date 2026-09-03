use std::sync::Arc;
use atlas_core::backend::LocalBackend;
use crate::mcp_clients::ClientRegistry;

#[derive(Clone)]
pub struct AppState { pub backend: Arc<LocalBackend>, pub mcp_clients: Arc<ClientRegistry> }
