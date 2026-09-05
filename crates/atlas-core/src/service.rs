use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use uuid::Uuid;
use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::*;
use crate::search::{cosine, Bm25Index, Embedder};
use crate::{AtlasError, Result};

pub struct MemoryService {
    db: Arc<Db>,
    embedder: RwLock<Arc<dyn Embedder>>,
    index: RwLock<Bm25Index>,
    vectors: RwLock<HashMap<Uuid, Vec<f32>>>,
    embed_error: RwLock<Option<String>>,
    loading: RwLock<bool>,
    /// Serializes everything that mutates the derived state (`index`, `vectors`) against
    /// everything that rebuilds it from the DB. Without it a `remember` whose DB insert
    /// lands after `reload`'s read but whose `upsert` lands before `reload`'s wholesale
    /// replacement is silently dropped from the index and stays unrecallable until restart.
    write_gate: Arc<Mutex<()>>,
}

impl MemoryService {
    pub fn new(db: Arc<Db>, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let svc = Self { db, embedder: RwLock::new(embedder), index: RwLock::new(Bm25Index::new()), vectors: RwLock::new(HashMap::new()), embed_error: RwLock::new(None), loading: RwLock::new(false), write_gate: Arc::new(Mutex::new(())) };
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
    fn loading_read(&self) -> RwLockReadGuard<'_, bool> { self.loading.read().unwrap_or_else(|e| e.into_inner()) }
    fn loading_write(&self) -> RwLockWriteGuard<'_, bool> { self.loading.write().unwrap_or_else(|e| e.into_inner()) }
    fn gate(&self) -> MutexGuard<'_, ()> { self.write_gate.lock().unwrap_or_else(|e| e.into_inner()) }

    /// The same gate, for a caller outside this type with its own read-modify-write to
    /// serialize: the project profile, written by both `refresh_project` and the
    /// summary job. Holding it also excludes `remember` and friends, which is heavier
    /// than those callers need, but it keeps one lock ordering in the process rather
    /// than two. The guard is a blocking, non-reentrant mutex: never hold it across an
    /// await, and never call another `MemoryService` method while holding it.
    pub fn write_gate(&self) -> MutexGuard<'_, ()> { self.gate() }

    /// The gate itself, for a type that owns its own writes and must take it in the
    /// same order the memory path does: `board::TaskRepo`. Handing out the `Arc`
    /// rather than a guard lets that type keep the mutex for its own lifetime
    /// without borrowing the service.
    pub fn gate_handle(&self) -> Arc<Mutex<()>> { self.write_gate.clone() }

    /// Clone of the current embedder's `Arc`, so callers don't hold the lock while embedding.
    fn emb(&self) -> Arc<dyn Embedder> { self.embedder.read().unwrap_or_else(|e| e.into_inner()).clone() }

    /// Swap in a new embedder, drop any stale error, reload persisted vectors for it, and
    /// backfill a vector for every active memory that doesn't have one under the new model.
    pub fn set_embedder(&self, e: Arc<dyn Embedder>) -> Result<()> {
        // `loading` spans the swap, reload and backfill so `embedding_status` doesn't
        // announce "ready" while memories still have no vector under the new model.
        self.set_loading(true);
        let result = self.set_embedder_inner(e);
        self.set_loading(false);
        result
    }

    /// How many texts one backfill `embed` call carries.
    const BACKFILL_BATCH: usize = 32;

    fn set_embedder_inner(&self, e: Arc<dyn Embedder>) -> Result<()> {
        // The swap and rebuild are gated; the backfill is not. Holding the gate across
        // every missing memory's inference blocked every memory, task and workflow
        // write for the whole backfill, which is minutes on a large store.
        let missing: Vec<(Uuid, String)> = {
            let _gate = self.gate();
            *self.embedder.write().unwrap_or_else(|e| e.into_inner()) = e.clone();
            *self.err_write() = None;
            self.reload_gated()?;
            let vectors = self.vec_read();
            self.repo().list_active(None, None)?.into_iter().filter(|m| !vectors.contains_key(&m.id)).map(|m| (m.id, m.text)).collect()
        };
        for batch in missing.chunks(Self::BACKFILL_BATCH) {
            let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
            match e.embed(&texts) {
                Ok(vecs) => {
                    let _gate = self.gate();
                    for ((id, _), vec) in batch.iter().zip(vecs) { self.store_vector_gated(*id, e.name(), vec); }
                }
                Err(err) => self.record_embed_error(&e, err.to_string()),
            }
        }
        Ok(())
    }

    pub fn set_loading(&self, v: bool) { *self.loading_write() = v; }

    pub fn set_embed_error(&self, msg: String) { *self.err_write() = Some(msg); }

    /// Rebuild the keyword index and load stored vectors for active memories.
    pub fn reload(&self) -> Result<()> {
        let _gate = self.gate();
        self.reload_gated()
    }

    /// The body of `reload`, for callers that already hold `write_gate`.
    fn reload_gated(&self) -> Result<()> {
        let mems = self.repo().list_active(None, None)?;
        let mut idx = Bm25Index::new();
        for m in &mems { idx.upsert(m.id, &m.text); }
        *self.idx_write() = idx;
        let emb = self.emb();
        let vecs: Vec<(Uuid, Vec<f32>)> = self.db.with_conn(|c| {
            let mut st = c.prepare("select e.memory_id::text, to_json(e.vector)::text from memory_embeddings e join memories m on m.id = e.memory_id where m.status='active' and e.model = ?")?;
            let rows = st.query_map([emb.name()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = vec![];
            for row in rows {
                let (id, v) = row?;
                match (Uuid::parse_str(&id), serde_json::from_str::<Vec<f32>>(&v)) {
                    (Ok(id), Ok(v)) => out.push((id, v)),
                    (Ok(id), Err(e)) => tracing::warn!("skipping unparseable stored vector for memory {id}: {e}"),
                    (Err(e), _) => tracing::warn!("skipping stored vector with unparseable memory id {id:?}: {e}"),
                }
            }
            Ok(out)
        })?;
        *self.vec_write() = vecs.into_iter().collect();
        Ok(())
    }

    /// Embed `text` with no gate held, then take the gate only to store the result.
    /// Inference is the slow part of a write, and holding `write_gate` across it made
    /// every memory, task and workflow write wait on the model.
    fn embed_and_store(&self, id: Uuid, text: &str) {
        let emb = self.emb();
        match emb.embed(&[text.to_string()]) {
            Ok(mut v) if !v.is_empty() => {
                let _gate = self.gate();
                self.store_vector_gated(id, emb.name(), v.remove(0));
            }
            Ok(_) => {}
            Err(e) => self.record_embed_error(&emb, e.to_string()),
        }
    }

    /// Records an embed failure, unless `emb` has already been replaced: a stale
    /// embedder's error must not mask the status of the one now in use.
    fn record_embed_error(&self, emb: &Arc<dyn Embedder>, msg: String) {
        if self.emb().name() == emb.name() { *self.err_write() = Some(msg); }
    }

    /// Persist a vector computed by the embedder named `model` and, only once it is
    /// durably stored, make it visible in the in-memory `vectors` map. A DB write
    /// failure must not leave the in-memory index claiming a vector that isn't stored.
    ///
    /// The vector was computed with the gate released, so the world may have moved on:
    /// if the embedder changed, this vector belongs to the wrong model and the new
    /// model's backfill covers the memory; if the memory left the active set, `reload`
    /// would drop the vector anyway. Both cases skip the store. Callers must hold `write_gate`.
    fn store_vector_gated(&self, id: Uuid, model: &str, vec: Vec<f32>) {
        if self.emb().name() != model { return; }
        match self.repo().get(id) {
            Ok(m) if m.status == MemoryStatus::Active => {}
            _ => return,
        }
        let json = serde_json::to_string(&vec).unwrap_or_default();
        let write_result = self.db.with_conn(|c| {
            c.execute("delete from memory_embeddings where memory_id = ?", [id.to_string()])?;
            c.execute(&format!("insert into memory_embeddings values (?, ?, {json}::float[])"), duckdb::params![id.to_string(), model])?;
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

    pub fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> {
        if m.scope == MemoryScope::Project && m.project_id.is_none() { return Err(AtlasError::Invalid("project scope requires project_id".into())); }
        let saved = {
            let _gate = self.gate();
            let saved = self.repo().insert(&m, actor)?;
            // Only active memories belong in the derived state: `reload` rebuilds it from the
            // active rows alone, so indexing a pending one here would not survive a restart.
            if saved.status == MemoryStatus::Active { self.idx_write().upsert(saved.id, &saved.text); }
            saved
        };
        if saved.status == MemoryStatus::Active { self.embed_and_store(saved.id, &saved.text); }
        Ok(saved)
    }

    pub fn get(&self, id: Uuid) -> Result<Memory> { self.repo().get(id) }

    pub fn list(&self, status: MemoryStatus, scope: Option<MemoryScope>, project_id: Option<Uuid>) -> Result<Vec<Memory>> {
        self.repo().list_by_status(status, scope, project_id)
    }

    /// [`list`](Self::list) with a say in whether a `project_id` widens to the global
    /// memories or narrows to the project's own.
    pub fn list_scoped(
        &self,
        status: MemoryStatus,
        scope: Option<MemoryScope>,
        project_id: Option<Uuid>,
        only: MemoryScopeFilter,
    ) -> Result<Vec<Memory>> {
        self.repo().list_by_status_scoped(status, scope, project_id, only)
    }

    /// Kind and tag counts, plus the total, over active memories; `project_id` read the
    /// same narrowing [`list_scoped`](Self::list_scoped) takes.
    pub fn facets(&self, project_id: Option<Uuid>, only: MemoryScopeFilter) -> Result<MemoryFacets> {
        self.repo().facets(project_id, only)
    }

    /// Moves a memory between statuses, keeping the search index in step: becoming
    /// active makes it searchable, leaving active takes it back out.
    pub fn set_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory> {
        let m = {
            let _gate = self.gate();
            let m = self.repo().set_status(id, status, actor)?;
            if status == MemoryStatus::Active {
                self.idx_write().upsert(m.id, &m.text);
            } else {
                self.idx_write().remove(id);
                self.vec_write().remove(&id);
            }
            m
        };
        if status == MemoryStatus::Active { self.embed_and_store(m.id, &m.text); }
        Ok(m)
    }

    pub fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> {
        let _gate = self.gate();
        let m = self.repo().supersede(id, None, actor)?;
        if let Some(r) = reason { self.repo().audit(actor, "forget_reason", "memory", Some(id), serde_json::json!({"reason": r}))?; }
        self.idx_write().remove(id);
        self.vec_write().remove(&id);
        Ok(m)
    }

    pub fn recall(&self, q: &RecallQuery) -> Result<Vec<RecallHit>> {
        let candidates = self.repo().list_by_status_scoped(MemoryStatus::Active, q.scope, q.project_id, q.list_scope)?;
        let candidates: Vec<Memory> = candidates.into_iter().filter(|m| {
            (q.kinds.is_empty() || q.kinds.contains(&m.kind)) && (q.tags.is_empty() || q.tags.iter().any(|t| m.tags.contains(t)))
        }).collect();
        if candidates.is_empty() { return Ok(vec![]); }
        let allowed: HashMap<Uuid, &Memory> = candidates.iter().map(|m| (m.id, m)).collect();
        let kw: HashMap<Uuid, f64> = self.idx_read().query(&q.query, usize::MAX).into_iter().filter(|(id, _)| allowed.contains_key(id)).collect();
        // Only probe the embedder when it can actually produce vectors; a failure here is
        // recorded so `status()` surfaces it, but recall still falls back to keyword-only.
        let emb = self.emb();
        let qvec = if emb.dims() > 0 {
            match emb.embed(std::slice::from_ref(&q.query)) {
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

    /// The active memory closest to `text` and its cosine similarity, for the
    /// extraction worker's duplicate check. `None` when there is nothing to
    /// compare against: no embedding model, an embedder that fails on this text,
    /// or no active memory in scope with a stored vector. A caller that gets
    /// `None` has to fall back to comparing the text itself.
    ///
    /// `project_id` bounds the comparison, because the daemon serves every project
    /// at once and `vectors` spans all of them. `Some(p)` compares against that
    /// project's memories plus every global one; `None` compares against global
    /// memories only, so a project's wording never suppresses a global candidate.
    pub fn nearest_active(&self, text: &str, project_id: Option<Uuid>) -> Result<Option<(Uuid, f64)>> {
        let emb = self.emb();
        if emb.dims() == 0 { return Ok(None); }
        // Which ids are in scope comes first: with nothing to compare against there is
        // no reason to spend an embedding on the candidate. `list_active` with a project
        // widens to that project plus every global memory, which is the rule wanted here.
        let scope = project_id.is_none().then_some(MemoryScope::Global);
        let allowed: std::collections::HashSet<Uuid> =
            self.repo().list_active(scope, project_id)?.into_iter().map(|m| m.id).collect();
        if allowed.is_empty() { return Ok(None); }
        let qvec = match emb.embed(&[text.to_string()]) {
            Ok(mut v) if !v.is_empty() => v.remove(0),
            Ok(_) => return Ok(None),
            // A failure here is recorded so `status()` surfaces it, exactly as in `recall`,
            // but it must not fail the ingest: the caller falls back to text comparison.
            Err(e) => { *self.err_write() = Some(e.to_string()); return Ok(None); }
        };
        let vectors = self.vec_read();
        Ok(vectors
            .iter()
            .filter(|(id, _)| allowed.contains(*id))
            .map(|(id, v)| (*id, cosine(&qvec, v)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)))
    }

    /// Appends an audit row under the write gate, so a caller outside this type
    /// records one in the same lock order as every other write on its path.
    pub fn audit(&self, actor: &str, action: &str, entity: &str, entity_id: Option<Uuid>, detail: serde_json::Value) -> Result<()> {
        let _gate = self.gate();
        self.repo().audit(actor, action, entity, entity_id, detail)
    }

    pub fn embedding_status(&self) -> String {
        // "loading" wins over everything: the model may already be swapped in while the
        // backfill is still running, and reporting "ready" then would be a lie.
        if *self.loading_read() { return "loading".into(); }
        if self.emb().dims() == 0 { return "unavailable: no embedding model loaded".into(); }
        match &*self.err_read() { Some(e) => format!("unavailable: {e}"), None => "ready".into() }
    }

    pub fn status(&self, port: Option<u16>) -> Result<StatusReport> {
        Ok(StatusReport {
            version: env!("CARGO_PKG_VERSION").to_string(),
            db_path: String::new(),
            memories_active: self.repo().count_active()?,
            memories_pending: self.repo().count_pending()?,
            embedding: self.embedding_status(),
            port,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::search::NoopEmbedder;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

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
        let hits = s.recall(&RecallQuery { query: "package manager bun".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].memory.text.contains("bun"));
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn forget_removes_from_recall() {
        let s = svc();
        let m = s.remember(nm("convex listens on 3210"), "t").unwrap();
        s.forget(m.id, Some("moved".into()), "t").unwrap();
        assert!(s.recall(&RecallQuery { query: "convex port".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] }).unwrap().is_empty());
        assert_eq!(s.get(m.id).unwrap().status, MemoryStatus::Superseded);
    }

    #[test]
    fn filters_by_kind_and_tag_and_project() {
        let s = svc();
        let p = uuid::Uuid::new_v4();
        let mut a = nm("prefer tabs"); a.kind = MemoryKind::Preference; a.tags = vec!["style".into()];
        let mut b = nm("prefer spaces in yaml"); b.scope = MemoryScope::Project; b.project_id = Some(p);
        s.remember(a, "t").unwrap(); s.remember(b, "t").unwrap();
        let q = |kinds: Vec<MemoryKind>, tags: Vec<String>, project: Option<uuid::Uuid>| s.recall(&RecallQuery { query: "prefer".into(), limit: 10, scope: None, list_scope: MemoryScopeFilter::All, project_id: project, kinds, tags }).unwrap().len();
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
        let hits = s.recall(&RecallQuery { query: "which runtime do we use".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
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
        let q = RecallQuery { query: "which runtime do we use".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] };
        let hits1 = s1.recall(&q).unwrap();
        let hits2 = s2.recall(&q).unwrap();
        assert!(!hits1.is_empty());
        assert_eq!(hits1[0].memory.id, hits2[0].memory.id);
        let count: i64 = db.with_conn(|c| Ok(c.query_row("select count(*) from memory_embeddings", [], |r| r.get(0))?)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn set_embedder_backfills_existing_memories() {
        let s = svc();
        s.remember(nm("bun is the javascript runtime here"), "t").unwrap();
        s.remember(nm("the api listens on port 3210"), "t").unwrap();
        assert!(s.status(None).unwrap().embedding.starts_with("unavailable"));
        s.set_embedder(Arc::new(FakeEmbedder)).unwrap();
        let count: i64 = s.db.with_conn(|c| Ok(c.query_row("select count(*) from memory_embeddings", [], |r| r.get(0))?)).unwrap();
        assert_eq!(count, 2);
        let hits = s.recall(&RecallQuery { query: "which runtime do we use".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].memory.text.contains("bun"));
        assert_eq!(s.status(None).unwrap().embedding, "ready");
    }

    /// A `set_embedder` (which rebuilds the index from the DB) running concurrently with
    /// writers must not erase memories whose insert raced the rebuild.
    #[test]
    fn concurrent_remember_survives_set_embedder() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let s = Arc::new(MemoryService::new(Arc::new(Db::open_in_memory().unwrap()), Arc::new(NoopEmbedder)).unwrap());
        // Pre-existing bulk so each rebuild spends real time between reading the DB and
        // swapping in the fresh index; that is the window a racing write falls into.
        let filler = "alpha beta gamma delta epsilon zeta eta theta iota kappa ".repeat(40);
        for i in 0..100 { s.remember(nm(&format!("filler {i} {filler}")), "t").unwrap(); }

        let done = Arc::new(AtomicBool::new(false));
        let writer = { let (s, done) = (s.clone(), done.clone()); std::thread::spawn(move || {
            for i in 0..50 { s.remember(nm(&format!("concurrent marker zqx{i} recorded")), "t").unwrap(); }
            done.store(true, Ordering::SeqCst);
        })};
        for _ in 0..25 {
            if done.load(Ordering::SeqCst) { break; }
            s.set_embedder(Arc::new(FakeEmbedder)).unwrap();
        }
        writer.join().unwrap();

        let active = s.repo().count_active().unwrap() as usize;
        assert_eq!(active, 150);
        assert_eq!(s.idx_read().len(), active, "index lost memories written during the rebuild");
        assert_eq!(s.vec_read().len(), active, "vectors lost memories written during the rebuild");
        for i in 0..50 {
            let token = format!("zqx{i}");
            assert!(!s.idx_read().query(&token, 5).is_empty(), "memory {token} is unrecallable");
        }
    }

    /// `FakeEmbedder` that parks every `embed` call until the test releases it, and
    /// records how many texts each call carried. `started` fires once per call.
    struct BlockingEmbedder { calls: Mutex<Vec<usize>>, started: mpsc::Sender<()>, release: Mutex<mpsc::Receiver<()>> }
    impl BlockingEmbedder {
        fn new() -> (Arc<Self>, mpsc::Receiver<()>, mpsc::Sender<()>) {
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            (Arc::new(Self { calls: Mutex::new(vec![]), started: started_tx, release: Mutex::new(release_rx) }), started_rx, release_tx)
        }
        fn calls(&self) -> Vec<usize> { self.calls.lock().unwrap().clone() }
    }
    impl Embedder for BlockingEmbedder {
        fn name(&self) -> &str { "fake" }
        fn dims(&self) -> usize { 8 }
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            self.calls.lock().unwrap().push(texts.len());
            self.started.send(()).ok();
            self.release.lock().unwrap().recv().ok();
            FakeEmbedder.embed(texts)
        }
    }

    const WAIT: Duration = Duration::from_secs(5);

    /// Acquires and drops `write_gate` on another thread; `true` if that finished
    /// within `WAIT`, which it only does when no other thread is holding the gate.
    fn gate_is_free(s: &Arc<MemoryService>) -> bool {
        let (tx, rx) = mpsc::channel();
        let s = s.clone();
        std::thread::spawn(move || { drop(s.write_gate()); tx.send(()).ok(); });
        rx.recv_timeout(WAIT).is_ok()
    }

    fn stored_vectors(s: &MemoryService) -> i64 {
        s.db.with_conn(|c| Ok(c.query_row("select count(*) from memory_embeddings", [], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn remember_does_not_hold_gate_while_embedding() {
        let (emb, started, release) = BlockingEmbedder::new();
        let s = Arc::new(MemoryService::new(Arc::new(Db::open_in_memory().unwrap()), emb).unwrap());
        let writer = { let s = s.clone(); std::thread::spawn(move || s.remember(nm("bun is the runtime"), "t").unwrap()) };
        started.recv_timeout(WAIT).expect("embed never started");
        assert!(gate_is_free(&s), "write_gate is held while the embedder runs");
        // The row and its keyword index entry are already visible mid-embed.
        assert_eq!(s.list(MemoryStatus::Active, None, None).unwrap().len(), 1);
        assert_eq!(s.idx_read().len(), 1);
        assert!(s.vec_read().is_empty(), "a vector was claimed before it was computed");
        release.send(()).unwrap();
        let saved = writer.join().unwrap();
        assert!(s.vec_read().contains_key(&saved.id));
        assert_eq!(stored_vectors(&s), 1);
    }

    #[test]
    fn memory_forgotten_during_embed_gets_no_vector() {
        let (emb, started, release) = BlockingEmbedder::new();
        let s = Arc::new(MemoryService::new(Arc::new(Db::open_in_memory().unwrap()), emb).unwrap());
        let writer = { let s = s.clone(); std::thread::spawn(move || s.remember(nm("convex listens on 3210"), "t").unwrap()) };
        started.recv_timeout(WAIT).expect("embed never started");
        let id = s.list(MemoryStatus::Active, None, None).unwrap()[0].id;
        s.forget(id, None, "t").unwrap();
        release.send(()).unwrap();
        writer.join().unwrap();
        assert!(s.vec_read().is_empty(), "a forgotten memory kept a vector");
        assert_eq!(stored_vectors(&s), 0);
        assert!(s.idx_read().is_empty());
    }

    #[test]
    fn set_embedder_backfills_in_batches_outside_the_gate() {
        let s = Arc::new(svc());
        for i in 0..70 { s.remember(nm(&format!("memory number {i} about bun")), "t").unwrap(); }
        let (emb, started, release) = BlockingEmbedder::new();
        let swapper = { let (s, emb) = (s.clone(), emb.clone()); std::thread::spawn(move || s.set_embedder(emb).unwrap()) };
        for _ in 0..3 {
            started.recv_timeout(WAIT).expect("backfill batch never started");
            assert_eq!(s.embedding_status(), "loading");
            assert!(gate_is_free(&s), "write_gate is held during the backfill");
            release.send(()).unwrap();
        }
        swapper.join().unwrap();
        assert_eq!(emb.calls(), vec![32, 32, 6]);
        assert_eq!(s.vec_read().len(), 70);
        assert_eq!(stored_vectors(&s), 70);
        assert_eq!(s.embedding_status(), "ready");
    }

    #[test]
    fn memory_without_vector_still_recalled_by_keyword() {
        let db = Db::open_in_memory().unwrap();
        MemoryRepo::new(&db).insert(&nm("legacy memory about redis cache"), "t").unwrap();
        let s = MemoryService::new(Arc::new(db), Arc::new(FakeEmbedder)).unwrap();
        let hits = s.recall(&RecallQuery { query: "redis cache".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] }).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].memory.text.contains("redis"));
    }
}
