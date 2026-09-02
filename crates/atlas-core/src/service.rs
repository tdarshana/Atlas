use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use uuid::Uuid;
use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::*;
use crate::search::{cosine, Bm25Index, Embedder};
use crate::{AtlasError, Result};

pub struct MemoryService {
    db: Arc<Db>,
    embedder: Arc<dyn Embedder>,
    index: RwLock<Bm25Index>,
    vectors: RwLock<HashMap<Uuid, Vec<f32>>>,
    embed_error: RwLock<Option<String>>,
}

impl MemoryService {
    pub fn new(db: Arc<Db>, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let svc = Self { db, embedder, index: RwLock::new(Bm25Index::new()), vectors: RwLock::new(HashMap::new()), embed_error: RwLock::new(None) };
        svc.reload()?;
        Ok(svc)
    }

    fn repo(&self) -> MemoryRepo<'_> { MemoryRepo::new(&self.db) }

    // Poison-tolerant lock accessors: a panic elsewhere while a lock is held must not
    // make every later read/write panic too, so we recover the guard from a poisoned lock.
    fn idx_read(&self) -> RwLockReadGuard<'_, Bm25Index> { self.index.read().unwrap_or_else(|e| e.into_inner()) }
    fn idx_write(&self) -> RwLockWriteGuard<'_, Bm25Index> { self.index.write().unwrap_or_else(|e| e.into_inner()) }
    fn vec_read(&self) -> RwLockReadGuard<'_, HashMap<Uuid, Vec<f32>>> { self.vectors.read().unwrap_or_else(|e| e.into_inner()) }
    fn vec_write(&self) -> RwLockWriteGuard<'_, HashMap<Uuid, Vec<f32>>> { self.vectors.write().unwrap_or_else(|e| e.into_inner()) }
    fn err_read(&self) -> RwLockReadGuard<'_, Option<String>> { self.embed_error.read().unwrap_or_else(|e| e.into_inner()) }
    fn err_write(&self) -> RwLockWriteGuard<'_, Option<String>> { self.embed_error.write().unwrap_or_else(|e| e.into_inner()) }

    /// Rebuild the keyword index and load stored vectors for active memories.
    pub fn reload(&self) -> Result<()> {
        let mems = self.repo().list_active(None, None)?;
        let mut idx = Bm25Index::new();
        for m in &mems { idx.upsert(m.id, &m.text); }
        *self.idx_write() = idx;
        let vecs: Vec<(Uuid, Vec<f32>)> = self.db.with_conn(|c| {
            let mut st = c.prepare("select e.memory_id::text, to_json(e.vector)::text from memory_embeddings e join memories m on m.id = e.memory_id where m.status='active' and e.model = ?")?;
            let rows = st.query_map([self.embedder.name()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = vec![];
            for row in rows { let (id, v) = row?; if let (Ok(id), Ok(v)) = (Uuid::parse_str(&id), serde_json::from_str::<Vec<f32>>(&v)) { out.push((id, v)); } }
            Ok(out)
        })?;
        *self.vec_write() = vecs.into_iter().collect();
        Ok(())
    }

    /// Embed `text` and, only if the embedding is both computed and durably persisted,
    /// make it visible in the in-memory `vectors` map. A DB write failure must not leave
    /// the in-memory index claiming a vector exists that isn't actually stored.
    fn try_embed(&self, id: Uuid, text: &str) {
        match self.embedder.embed(&[text.to_string()]) {
            Ok(mut v) if !v.is_empty() => {
                let vec = v.remove(0);
                let json = serde_json::to_string(&vec).unwrap_or_default();
                let write_result = self.db.with_conn(|c| {
                    c.execute("delete from memory_embeddings where memory_id = ?", [id.to_string()])?;
                    c.execute(&format!("insert into memory_embeddings values (?, ?, {json}::float[])"), duckdb::params![id.to_string(), self.embedder.name()])?;
                    Ok(())
                });
                match write_result {
                    Ok(()) => {
                        self.vec_write().insert(id, vec);
                        *self.err_write() = None;
                    }
                    Err(e) => {
                        let msg = format!("failed to persist embedding: {e}");
                        tracing::warn!("{msg}");
                        *self.err_write() = Some(msg);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => { *self.err_write() = Some(e.to_string()); }
        }
    }

    pub fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> {
        if m.scope == MemoryScope::Project && m.project_id.is_none() { return Err(AtlasError::Invalid("project scope requires project_id".into())); }
        let saved = self.repo().insert(&m, actor)?;
        self.idx_write().upsert(saved.id, &saved.text);
        self.try_embed(saved.id, &saved.text);
        Ok(saved)
    }

    pub fn get(&self, id: Uuid) -> Result<Memory> { self.repo().get(id) }

    pub fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> {
        let m = self.repo().supersede(id, None, actor)?;
        if let Some(r) = reason { self.repo().audit(actor, "forget_reason", "memory", Some(id), serde_json::json!({"reason": r}))?; }
        self.idx_write().remove(id);
        self.vec_write().remove(&id);
        Ok(m)
    }

    pub fn recall(&self, q: &RecallQuery) -> Result<Vec<RecallHit>> {
        let candidates = self.repo().list_active(q.scope, q.project_id)?;
        let candidates: Vec<Memory> = candidates.into_iter().filter(|m| {
            (q.kinds.is_empty() || q.kinds.contains(&m.kind)) && (q.tags.is_empty() || q.tags.iter().any(|t| m.tags.contains(t)))
        }).collect();
        if candidates.is_empty() { return Ok(vec![]); }
        let allowed: HashMap<Uuid, &Memory> = candidates.iter().map(|m| (m.id, m)).collect();
        let kw: HashMap<Uuid, f64> = self.idx_read().query(&q.query, usize::MAX).into_iter().filter(|(id, _)| allowed.contains_key(id)).collect();
        // Only probe the embedder when it can actually produce vectors; a failure here is
        // recorded so `status()` surfaces it, but recall still falls back to keyword-only.
        let qvec = if self.embedder.dims() > 0 {
            match self.embedder.embed(&[q.query.clone()]) {
                Ok(mut v) if !v.is_empty() => Some(v.remove(0)),
                Ok(_) => None,
                Err(e) => { *self.err_write() = Some(e.to_string()); None }
            }
        } else { None };
        let vectors = self.vec_read();
        let mut hits: Vec<RecallHit> = candidates.iter().filter_map(|m| {
            let k = kw.get(&m.id).copied().unwrap_or(0.0);
            // A memory only gets the hybrid treatment when both the query and the memory
            // itself have a vector; otherwise fall back to pure keyword scoring so memories
            // written before an embedding model was available aren't penalized.
            let score = match (&qvec, vectors.get(&m.id)) {
                (Some(qv), Some(mv)) => {
                    let c = cosine(qv, mv).max(0.0);
                    let s = 0.6 * c + 0.4 * k;
                    (s > 0.35).then_some(s)
                }
                _ => (k > 0.0).then_some(k),
            };
            score.map(|score| RecallHit { memory: m.clone(), score })
        }).collect();
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(q.limit);
        Ok(hits)
    }

    pub fn embedding_status(&self) -> String {
        if self.embedder.dims() == 0 { return "unavailable: no embedding model loaded".into(); }
        match &*self.err_read() { Some(e) => format!("unavailable: {e}"), None => "ready".into() }
    }

    pub fn status(&self, port: Option<u16>) -> Result<StatusReport> {
        Ok(StatusReport { version: env!("CARGO_PKG_VERSION").to_string(), db_path: String::new(), memories_active: self.repo().count_active()?, embedding: self.embedding_status(), port })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::search::NoopEmbedder;
    use std::sync::Arc;

    fn svc() -> MemoryService { MemoryService::new(Arc::new(Db::open_in_memory().unwrap()), Arc::new(NoopEmbedder)).unwrap() }
    fn nm(text: &str) -> NewMemory { NewMemory { scope: MemoryScope::Global, project_id: None, kind: MemoryKind::Fact, text: text.into(), tags: vec![], source_agent: None, source_tool: Some("test".into()), confidence: 1.0, status: MemoryStatus::Active } }

    /// Deterministic bag-of-words embedder for tests: hashes each token from
    /// `crate::search::tokenize` into one of 8 buckets, counts, and L2-normalizes.
    struct FakeEmbedder;
    impl Embedder for FakeEmbedder {
        fn name(&self) -> &str { "fake" }
        fn dims(&self) -> usize { 8 }
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|t| {
                let mut buckets = [0f32; 8];
                for tok in crate::search::tokenize(t) {
                    let h = tok.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                    buckets[(h % 8) as usize] += 1.0;
                }
                let norm = buckets.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 { for b in buckets.iter_mut() { *b /= norm; } }
                buckets.to_vec()
            }).collect())
        }
    }

    #[test]
    fn remember_then_recall_keyword_only() {
        let s = svc();
        s.remember(nm("the repo uses bun instead of pnpm"), "t").unwrap();
        s.remember(nm("dark mode is a class on html"), "t").unwrap();
        let hits = s.recall(&RecallQuery { query: "package manager bun".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].memory.text.contains("bun"));
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn forget_removes_from_recall() {
        let s = svc();
        let m = s.remember(nm("convex listens on 3210"), "t").unwrap();
        s.forget(m.id, Some("moved".into()), "t").unwrap();
        assert!(s.recall(&RecallQuery { query: "convex port".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] }).unwrap().is_empty());
        assert_eq!(s.get(m.id).unwrap().status, MemoryStatus::Superseded);
    }

    #[test]
    fn filters_by_kind_and_tag_and_project() {
        let s = svc();
        let p = uuid::Uuid::new_v4();
        let mut a = nm("prefer tabs"); a.kind = MemoryKind::Preference; a.tags = vec!["style".into()];
        let mut b = nm("prefer spaces in yaml"); b.scope = MemoryScope::Project; b.project_id = Some(p);
        s.remember(a, "t").unwrap(); s.remember(b, "t").unwrap();
        let q = |kinds: Vec<MemoryKind>, tags: Vec<String>, project: Option<uuid::Uuid>| s.recall(&RecallQuery { query: "prefer".into(), limit: 10, scope: None, project_id: project, kinds, tags }).unwrap().len();
        assert_eq!(q(vec![MemoryKind::Preference], vec![], None), 1);
        assert_eq!(q(vec![], vec!["style".into()], None), 1);
        assert_eq!(q(vec![], vec![], Some(p)), 2);        // project + global
        assert_eq!(q(vec![], vec![], Some(uuid::Uuid::new_v4())), 1); // other project sees only global
    }

    #[test]
    fn status_reports_counts() {
        let s = svc();
        s.remember(nm("x y z"), "t").unwrap();
        let st = s.status(Some(7433)).unwrap();
        assert_eq!(st.memories_active, 1);
        assert!(st.embedding.starts_with("unavailable"));
        assert_eq!(st.port, Some(7433));
    }

    #[test]
    fn hybrid_recall_prefers_semantic_match() {
        let s = MemoryService::new(Arc::new(Db::open_in_memory().unwrap()), Arc::new(FakeEmbedder)).unwrap();
        s.remember(nm("bun is the javascript runtime here"), "t").unwrap();
        s.remember(nm("the api listens on port 3210"), "t").unwrap();
        let hits = s.recall(&RecallQuery { query: "which runtime do we use".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].memory.text.contains("bun"));
        assert!(hits[0].score > 0.35);
        assert_eq!(s.status(None).unwrap().embedding, "ready");
    }

    #[test]
    fn embeddings_persist_across_reload() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let s1 = MemoryService::new(db.clone(), Arc::new(FakeEmbedder)).unwrap();
        s1.remember(nm("bun is the javascript runtime here"), "t").unwrap();
        s1.remember(nm("the api listens on port 3210"), "t").unwrap();
        let s2 = MemoryService::new(db.clone(), Arc::new(FakeEmbedder)).unwrap();
        let q = RecallQuery { query: "which runtime do we use".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] };
        let hits1 = s1.recall(&q).unwrap();
        let hits2 = s2.recall(&q).unwrap();
        assert!(!hits1.is_empty());
        assert_eq!(hits1[0].memory.id, hits2[0].memory.id);
        let count: i64 = db.with_conn(|c| Ok(c.query_row("select count(*) from memory_embeddings", [], |r| r.get(0))?)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn memory_without_vector_still_recalled_by_keyword() {
        let db = Db::open_in_memory().unwrap();
        MemoryRepo::new(&db).insert(&nm("legacy memory about redis cache"), "t").unwrap();
        let s = MemoryService::new(Arc::new(db), Arc::new(FakeEmbedder)).unwrap();
        let hits = s.recall(&RecallQuery { query: "redis cache".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].memory.text.contains("redis"));
    }
}
