use std::collections::HashMap;
use std::sync::{Arc, RwLock};
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

    /// Rebuild the keyword index and load stored vectors for active memories.
    pub fn reload(&self) -> Result<()> {
        let mems = self.repo().list_active(None, None)?;
        let mut idx = Bm25Index::new();
        for m in &mems { idx.upsert(m.id, &m.text); }
        *self.index.write().unwrap() = idx;
        let vecs: Vec<(Uuid, Vec<f32>)> = self.db.with_conn(|c| {
            let mut st = c.prepare("select e.memory_id::text, to_json(e.vector)::text from memory_embeddings e join memories m on m.id = e.memory_id where m.status='active' and e.model = ?")?;
            let rows = st.query_map([self.embedder.name()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = vec![];
            for row in rows { let (id, v) = row?; if let (Ok(id), Ok(v)) = (Uuid::parse_str(&id), serde_json::from_str::<Vec<f32>>(&v)) { out.push((id, v)); } }
            Ok(out)
        })?;
        *self.vectors.write().unwrap() = vecs.into_iter().collect();
        Ok(())
    }

    fn try_embed(&self, id: Uuid, text: &str) {
        match self.embedder.embed(&[text.to_string()]) {
            Ok(mut v) if !v.is_empty() => {
                let vec = v.remove(0);
                let json = serde_json::to_string(&vec).unwrap_or_default();
                let _ = self.db.with_conn(|c| {
                    c.execute("delete from memory_embeddings where memory_id = ?", [id.to_string()])?;
                    c.execute(&format!("insert into memory_embeddings values (?, ?, {json}::float[])"), duckdb::params![id.to_string(), self.embedder.name()])?;
                    Ok(())
                });
                self.vectors.write().unwrap().insert(id, vec);
                *self.embed_error.write().unwrap() = None;
            }
            Ok(_) => {}
            Err(e) => { *self.embed_error.write().unwrap() = Some(e.to_string()); }
        }
    }

    pub fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> {
        if m.scope == MemoryScope::Project && m.project_id.is_none() { return Err(AtlasError::Invalid("project scope requires project_id".into())); }
        let saved = self.repo().insert(&m, actor)?;
        self.index.write().unwrap().upsert(saved.id, &saved.text);
        self.try_embed(saved.id, &saved.text);
        Ok(saved)
    }

    pub fn get(&self, id: Uuid) -> Result<Memory> { self.repo().get(id) }

    pub fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> {
        let m = self.repo().supersede(id, None, actor)?;
        if let Some(r) = reason { self.repo().audit(actor, "forget_reason", "memory", Some(id), serde_json::json!({"reason": r}))?; }
        self.index.write().unwrap().remove(id);
        self.vectors.write().unwrap().remove(&id);
        Ok(m)
    }

    pub fn recall(&self, q: &RecallQuery) -> Result<Vec<RecallHit>> {
        let candidates = self.repo().list_active(q.scope, q.project_id)?;
        let candidates: Vec<Memory> = candidates.into_iter().filter(|m| {
            (q.kinds.is_empty() || q.kinds.contains(&m.kind)) && (q.tags.is_empty() || q.tags.iter().any(|t| m.tags.contains(t)))
        }).collect();
        if candidates.is_empty() { return Ok(vec![]); }
        let allowed: HashMap<Uuid, &Memory> = candidates.iter().map(|m| (m.id, m)).collect();
        let kw: HashMap<Uuid, f64> = self.index.read().unwrap().query(&q.query, usize::MAX).into_iter().filter(|(id, _)| allowed.contains_key(id)).collect();
        let qvec = self.embedder.embed(&[q.query.clone()]).ok().and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });
        let vectors = self.vectors.read().unwrap();
        let mut hits: Vec<RecallHit> = candidates.iter().filter_map(|m| {
            let k = kw.get(&m.id).copied().unwrap_or(0.0);
            let c = match (&qvec, vectors.get(&m.id)) { (Some(qv), Some(mv)) => cosine(qv, mv).max(0.0), _ => 0.0 };
            let score = if qvec.is_some() { 0.6 * c + 0.4 * k } else { k };
            let floor = if qvec.is_some() { 0.35 } else { 0.0 };
            (score > floor).then(|| RecallHit { memory: m.clone(), score })
        }).collect();
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(q.limit.max(1));
        Ok(hits)
    }

    pub fn embedding_status(&self) -> String {
        if self.embedder.dims() == 0 { return "unavailable: no embedding model loaded".into(); }
        match &*self.embed_error.read().unwrap() { Some(e) => format!("unavailable: {e}"), None => "ready".into() }
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
}
