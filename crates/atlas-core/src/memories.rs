use crate::db::Db;
use crate::models::*;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use duckdb::{params, Row};
use uuid::Uuid;

pub struct MemoryRepo<'a> { db: &'a Db }

const COLS: &str = "id, scope, project_id, kind, text, tags, source_agent, source_tool, confidence, status, superseded_by, created_at, updated_at";

fn parse_uuid(s: Option<String>) -> Option<Uuid> { s.and_then(|v| Uuid::parse_str(&v).ok()) }
fn parse_ts(s: String) -> DateTime<Utc> {
    // DuckDB timestamp text: "2026-09-02 10:11:12.123456"
    chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f").or_else(|_| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")).map(|n| n.and_utc()).unwrap_or_else(|_| Utc::now())
}

/// Public wrapper around `parse_ts` for reuse in Phase 2.
pub fn parse_ts_pub(s: String) -> DateTime<Utc> { parse_ts(s) }

fn row_to_memory(r: &Row) -> duckdb::Result<Memory> {
    let tags_json: String = r.get::<_, String>(5)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Memory {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_default(),
        scope: r.get::<_, String>(1)?.parse().unwrap_or(MemoryScope::Global),
        project_id: parse_uuid(r.get::<_, Option<String>>(2)?),
        kind: r.get::<_, String>(3)?.parse().unwrap_or(MemoryKind::Fact),
        text: r.get(4)?,
        tags,
        source_agent: r.get(6)?,
        source_tool: r.get(7)?,
        confidence: r.get(8)?,
        status: r.get::<_, String>(9)?.parse().unwrap_or(MemoryStatus::Active),
        superseded_by: parse_uuid(r.get::<_, Option<String>>(10)?),
        created_at: parse_ts(r.get::<_, String>(11)?),
        updated_at: parse_ts(r.get::<_, String>(12)?),
    })
}

// Select list that casts to text so row mapping is uniform across DuckDB types.
fn select_cols() -> String {
    "id::text, scope, project_id::text, kind, text, to_json(tags)::text, source_agent, source_tool, confidence, status, superseded_by::text, created_at::text, updated_at::text".to_string()
}

impl<'a> MemoryRepo<'a> {
    pub fn new(db: &'a Db) -> Self { Self { db } }

    pub fn insert(&self, m: &NewMemory, actor: &str) -> Result<Memory> {
        if m.text.trim().is_empty() { return Err(AtlasError::Invalid("memory text is empty".into())); }
        let id = Uuid::new_v4();
        let tags_list = format!("[{}]", m.tags.iter().map(|t| format!("'{}'", t.replace('\'', "''"))).collect::<Vec<_>>().join(","));
        self.db.with_conn(|c| {
            c.execute(&format!("insert into memories ({COLS}) values (?, ?, ?, ?, ?, {tags_list}::text[], ?, ?, ?, ?, null, now(), now())"),
                params![id.to_string(), m.scope.as_str(), m.project_id.map(|p| p.to_string()), m.kind.as_str(), m.text, m.source_agent, m.source_tool, m.confidence, m.status.as_str()])?;
            Ok(())
        })?;
        self.audit(actor, "insert", "memory", Some(id), serde_json::json!({"kind": m.kind.as_str(), "scope": m.scope.as_str()}))?;
        self.get(id)
    }

    pub fn get(&self, id: Uuid) -> Result<Memory> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {} from memories where id = ?", select_cols()))?;
            let mut rows = st.query(params![id.to_string()])?;
            match rows.next()? { Some(r) => Ok(row_to_memory(r)?), None => Err(AtlasError::NotFound(format!("memory {id}"))) }
        })
    }

    pub fn list_active(&self, scope: Option<MemoryScope>, project_id: Option<Uuid>) -> Result<Vec<Memory>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {} from memories where status = 'active'", select_cols());
            let mut args: Vec<String> = vec![];
            if let Some(s) = scope { sql.push_str(" and scope = ?"); args.push(s.as_str().to_string()); }
            if let Some(p) = project_id { sql.push_str(" and (project_id = ? or scope = 'global')"); args.push(p.to_string()); }
            sql.push_str(" order by created_at desc");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(duckdb::params_from_iter(args.iter()), row_to_memory)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn set_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory> {
        self.get(id)?;
        self.db.with_conn(|c| { c.execute("update memories set status = ?, updated_at = now() where id = ?", params![status.as_str(), id.to_string()])?; Ok(()) })?;
        self.audit(actor, "set_status", "memory", Some(id), serde_json::json!({"status": status.as_str()}))?;
        self.get(id)
    }

    pub fn supersede(&self, id: Uuid, by: Option<Uuid>, actor: &str) -> Result<Memory> {
        self.get(id)?;
        self.db.with_conn(|c| { c.execute("update memories set status = 'superseded', superseded_by = ?, updated_at = now() where id = ?", params![by.map(|b| b.to_string()), id.to_string()])?; Ok(()) })?;
        self.audit(actor, "supersede", "memory", Some(id), serde_json::json!({"by": by}))?;
        self.get(id)
    }

    pub fn count_active(&self) -> Result<i64> {
        self.db.with_conn(|c| Ok(c.query_row("select count(*) from memories where status='active'", [], |r| r.get(0))?))
    }

    pub fn audit(&self, actor: &str, action: &str, entity: &str, entity_id: Option<Uuid>, detail: serde_json::Value) -> Result<()> {
        self.db.with_conn(|c| {
            c.execute("insert into audit (id, actor, action, entity, entity_id, detail) values (?, ?, ?, ?, ?, ?::json)",
                params![Uuid::new_v4().to_string(), actor, action, entity, entity_id.map(|e| e.to_string()), detail.to_string()])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::*;
    fn mem(text: &str, scope: MemoryScope) -> NewMemory {
        NewMemory { scope, project_id: None, kind: MemoryKind::Fact, text: text.into(), tags: vec!["t1".into()],
            source_agent: Some("test".into()), source_tool: None, confidence: 1.0, status: MemoryStatus::Active }
    }
    #[test]
    fn insert_get_list_supersede() {
        let db = Db::open_in_memory().unwrap();
        let repo = MemoryRepo::new(&db);
        let a = repo.insert(&mem("use bun not pnpm", MemoryScope::Global), "test").unwrap();
        let b = repo.insert(&mem("tailwind 4 config lives in css", MemoryScope::Global), "test").unwrap();
        assert_eq!(repo.get(a.id).unwrap().text, "use bun not pnpm");
        assert_eq!(repo.get(a.id).unwrap().tags, vec!["t1".to_string()]);
        assert_eq!(repo.list_active(Some(MemoryScope::Global), None).unwrap().len(), 2);
        let s = repo.supersede(a.id, Some(b.id), "test").unwrap();
        assert_eq!(s.status, MemoryStatus::Superseded);
        assert_eq!(s.superseded_by, Some(b.id));
        assert_eq!(repo.list_active(None, None).unwrap().len(), 1);
        assert_eq!(repo.count_active().unwrap(), 1);
        let audits: i64 = db.with_conn(|c| Ok(c.query_row("select count(*) from audit", [], |r| r.get(0))?)).unwrap();
        assert_eq!(audits, 3);
    }
    #[test]
    fn get_missing_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let repo = MemoryRepo::new(&db);
        assert!(matches!(repo.get(uuid::Uuid::new_v4()), Err(crate::AtlasError::NotFound(_))));
    }
}
