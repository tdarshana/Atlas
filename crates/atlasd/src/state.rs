use std::sync::Arc;
use atlas_core::backend::LocalBackend;

#[derive(Clone)]
pub struct AppState { pub backend: Arc<LocalBackend> }
