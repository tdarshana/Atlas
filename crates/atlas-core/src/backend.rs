use std::sync::Arc;
use uuid::Uuid;
use crate::db::Db;
use crate::models::*;
use crate::paths::AtlasPaths;
use crate::search::{FastEmbedder, NoopEmbedder};
use crate::service::MemoryService;
use crate::Result;

#[async_trait::async_trait]
pub trait Backend: Send + Sync + 'static {
    async fn status(&self) -> Result<StatusReport>;
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory>;
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>>;
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory>;
    async fn get_memory(&self, id: Uuid) -> Result<Memory>;
}

pub struct LocalBackend { pub memories: Arc<MemoryService>, pub paths: AtlasPaths, pub port: Option<u16> }

impl LocalBackend {
    pub fn open(paths: &AtlasPaths, port: Option<u16>, load_embedder: bool) -> Result<Self> {
        paths.ensure()?;
        let db = Arc::new(Db::open(&paths.db_path())?);
        let memories = Arc::new(MemoryService::new(db, Arc::new(NoopEmbedder))?);
        if load_embedder {
            memories.set_loading(true);
            let models_dir = paths.models_dir();
            let bg = memories.clone();
            std::thread::spawn(move || {
                match FastEmbedder::try_new(&models_dir) {
                    Ok(e) => {
                        if let Err(err) = bg.set_embedder(Arc::new(e)) {
                            tracing::warn!("failed to activate embedding model: {err}");
                            bg.set_embed_error(err.to_string());
                        }
                    }
                    Err(e) => {
                        tracing::warn!("embedding model unavailable, keyword-only search: {e}");
                        bg.set_embed_error(e.to_string());
                    }
                }
                bg.set_loading(false);
            });
        }
        Ok(Self { memories, paths: paths.clone(), port })
    }
}

#[async_trait::async_trait]
impl Backend for LocalBackend {
    async fn status(&self) -> Result<StatusReport> {
        let mut s = self.memories.status(self.port)?;
        s.db_path = self.paths.db_path().display().to_string();
        Ok(s)
    }
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> { self.memories.remember(m, actor) }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> { self.memories.recall(&q) }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> { self.memories.forget(id, reason, actor) }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> { self.memories.get(id) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn local_backend_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::AtlasPaths::at(dir.path());
        let b = LocalBackend::open(&paths, Some(1), false).unwrap();
        let m = b.remember(crate::models::NewMemory { scope: crate::models::MemoryScope::Global, project_id: None, kind: crate::models::MemoryKind::Fact, text: "bun is the runtime".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: crate::models::MemoryStatus::Active }, "t").await.unwrap();
        let hits = b.recall(crate::models::RecallQuery { query: "runtime".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] }).await.unwrap();
        assert_eq!(hits[0].memory.id, m.id);
        let st = b.status().await.unwrap();
        assert!(st.db_path.ends_with("atlas.duckdb"));
        assert_eq!(st.port, Some(1));
    }

    #[tokio::test]
    async fn open_returns_before_embedder_loads() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::AtlasPaths::at(dir.path());
        // load_embedder = false must not spawn the background download thread, so status
        // should reflect the (permanent) unavailable state, never the transient "loading" one.
        let b = LocalBackend::open(&paths, None, false).unwrap();
        let st = b.status().await.unwrap();
        assert!(st.embedding.starts_with("unavailable"), "expected unavailable, got {}", st.embedding);
        assert_ne!(st.embedding, "loading");
    }
}
