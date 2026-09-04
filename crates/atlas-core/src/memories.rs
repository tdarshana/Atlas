use crate::db::Db;
use crate::models::*;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use duckdb::types::Type;
use duckdb::{params, Row};
use uuid::Uuid;

pub struct MemoryRepo<'a> { db: &'a Db }

const COLS: &str = "id, scope, project_id, kind, text, tags, source_agent, source_tool, confidence, status, superseded_by, created_at, updated_at";

fn parse_uuid(s: Option<String>) -> Option<Uuid> { s.and_then(|v| Uuid::parse_str(&v).ok()) }

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

fn parse_ts(col: usize, s: &str) -> duckdb::Result<DateTime<Utc>> {
    // DuckDB timestamp text: "2026-09-02 10:11:12.123456"
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .map(|n| n.and_utc())
        .map_err(|e| conv_err(col, Type::Timestamp, e))
}

/// Public wrapper around `parse_ts` for reuse in Phase 2. Returns `duckdb::Result`
/// instead of `DateTime<Utc>` so a malformed timestamp fails loudly instead of
/// silently defaulting to `Utc::now()`; column index 0 is a placeholder since this
/// entry point has no row context.
pub fn parse_ts_pub(s: String) -> duckdb::Result<DateTime<Utc>> { parse_ts(0, &s) }

fn row_to_memory(r: &Row) -> duckdb::Result<Memory> {
    let tags_json: String = r.get::<_, String>(5)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).map_err(|e| conv_err(5, Type::Text, e))?;
    let id: Uuid = Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?;
    let scope: MemoryScope = r.get::<_, String>(1)?.parse().map_err(|e: AtlasError| conv_err(1, Type::Text, e))?;
    let kind: MemoryKind = r.get::<_, String>(3)?.parse().map_err(|e: AtlasError| conv_err(3, Type::Text, e))?;
    let status: MemoryStatus = r.get::<_, String>(9)?.parse().map_err(|e: AtlasError| conv_err(9, Type::Text, e))?;
    Ok(Memory {
        id,
        scope,
        project_id: parse_uuid(r.get::<_, Option<String>>(2)?),
        kind,
        text: r.get(4)?,
        tags,
        source_agent: r.get(6)?,
        source_tool: r.get(7)?,
        confidence: r.get(8)?,
        status,
        superseded_by: parse_uuid(r.get::<_, Option<String>>(10)?),
        created_at: parse_ts(11, &r.get::<_, String>(11)?)?,
        updated_at: parse_ts(12, &r.get::<_, String>(12)?)?,
    })
}

// Select list that casts to text so row mapping is uniform across DuckDB types.
fn select_cols() -> String {
    "id::text, scope, project_id::text, kind, text, to_json(tags)::text, source_agent, source_tool, confidence, status, superseded_by::text, created_at::text, updated_at::text".to_string()
}

/// One row of the `audit` table. Feeds the daemon's global search event group
/// alongside `board::TaskEvent`; `audit` itself carries no `project_id` column, so
/// this has no per-project scope to offer a caller.
pub struct AuditEntry {
    pub id: Uuid,
    pub actor: String,
    pub action: String,
    pub entity: String,
    pub entity_id: Option<Uuid>,
    pub detail: Option<serde_json::Value>,
    pub at: DateTime<Utc>,
}

fn row_to_audit(r: &Row) -> duckdb::Result<AuditEntry> {
    let entity_id: Option<String> = r.get(4)?;
    let detail: Option<String> = r.get(5)?;
    Ok(AuditEntry {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?,
        actor: r.get(1)?,
        action: r.get(2)?,
        entity: r.get(3)?,
        entity_id: entity_id.and_then(|s| Uuid::parse_str(&s).ok()),
        detail: detail.map(|d| serde_json::from_str(&d).map_err(|e| conv_err(5, Type::Text, e))).transpose()?,
        at: parse_ts(6, &r.get::<_, String>(6)?)?,
    })
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
        self.list_by_status(MemoryStatus::Active, scope, project_id)
    }

    /// Newest first. `project_id` widens rather than narrows: it matches that
    /// project's memories plus every global one, as `list_active` does.
    pub fn list_by_status(&self, status: MemoryStatus, scope: Option<MemoryScope>, project_id: Option<Uuid>) -> Result<Vec<Memory>> {
        self.list_by_status_scoped(status, scope, project_id, MemoryScopeFilter::All)
    }

    /// [`list_by_status`](Self::list_by_status) with a say in how `project_id` is read:
    /// `All` widens to that project plus the global memories, `ProjectOnly` keeps just
    /// the rows that belong to the project. With no `project_id` the two agree, since
    /// there is no project to narrow to.
    pub fn list_by_status_scoped(
        &self,
        status: MemoryStatus,
        scope: Option<MemoryScope>,
        project_id: Option<Uuid>,
        only: MemoryScopeFilter,
    ) -> Result<Vec<Memory>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {} from memories where status = ?", select_cols());
            let mut args: Vec<String> = vec![status.as_str().to_string()];
            if let Some(s) = scope { sql.push_str(" and scope = ?"); args.push(s.as_str().to_string()); }
            if only == MemoryScopeFilter::GlobalOnly {
                sql.push_str(" and scope = 'global'");
            } else if let Some(p) = project_id {
                match only {
                    MemoryScopeFilter::All => sql.push_str(" and (project_id = ? or scope = 'global')"),
                    MemoryScopeFilter::ProjectOnly => sql.push_str(" and project_id = ?"),
                    MemoryScopeFilter::GlobalOnly => unreachable!("handled above"),
                }
                args.push(p.to_string());
            }
            sql.push_str(" order by created_at desc");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(duckdb::params_from_iter(args.iter()), row_to_memory)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    /// Active memories whose text case-insensitively contains `pattern` (a
    /// caller-built `LIKE`-escaped substring, wrapped in `%...%`), scoped like
    /// `list_by_status` (a project given also matches every global memory), newest
    /// first, capped at 500. A SQL-level prefilter for global search.
    pub fn search_candidates(&self, project_id: Option<Uuid>, pattern: &str) -> Result<Vec<Memory>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {} from memories where status = 'active' and lower(text) like ? escape '\\'", select_cols());
            let mut args: Vec<String> = vec![pattern.to_string()];
            if let Some(p) = project_id {
                sql.push_str(" and (project_id = ? or scope = 'global')");
                args.push(p.to_string());
            }
            sql.push_str(" order by updated_at desc limit 500");
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

    /// Kind and tag counts, plus the total, over active memories, `project_id` read
    /// the same way [`list_by_status_scoped`](Self::list_by_status_scoped) reads it.
    /// Tags are a list column, so counting them unnests it in the `from` clause rather
    /// than loading every memory to count client-side.
    pub fn facets(&self, project_id: Option<Uuid>, only: MemoryScopeFilter) -> Result<MemoryFacets> {
        self.db.with_conn(|c| {
            let mut project_clause = String::new();
            let mut args: Vec<String> = Vec::new();
            if only == MemoryScopeFilter::GlobalOnly {
                project_clause.push_str(" and scope = 'global'");
            } else if let Some(p) = project_id {
                match only {
                    MemoryScopeFilter::All => project_clause.push_str(" and (project_id = ? or scope = 'global')"),
                    MemoryScopeFilter::ProjectOnly => project_clause.push_str(" and project_id = ?"),
                    MemoryScopeFilter::GlobalOnly => unreachable!("handled above"),
                }
                args.push(p.to_string());
            }
            let total: i64 = c.query_row(
                &format!("select count(*) from memories where status = 'active'{project_clause}"),
                duckdb::params_from_iter(args.iter()),
                |r| r.get(0),
            )?;
            let mut kinds = std::collections::HashMap::new();
            {
                let mut st = c.prepare(&format!(
                    "select kind, count(*) from memories where status = 'active'{project_clause} group by kind"
                ))?;
                let rows = st.query_map(duckdb::params_from_iter(args.iter()), |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
                for row in rows { let (k, n) = row?; kinds.insert(k, n); }
            }
            let mut tags = std::collections::HashMap::new();
            {
                let mut st = c.prepare(&format!(
                    "select t.tag, count(*) from memories, unnest(memories.tags) as t(tag) \
                     where status = 'active'{project_clause} group by t.tag"
                ))?;
                let rows = st.query_map(duckdb::params_from_iter(args.iter()), |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
                for row in rows { let (t, n) = row?; tags.insert(t, n); }
            }
            Ok(MemoryFacets { kinds, tags, total })
        })
    }

    pub fn count_active(&self) -> Result<i64> {
        self.db.with_conn(|c| Ok(c.query_row("select count(*) from memories where status='active'", [], |r| r.get(0))?))
    }

    pub fn count_pending(&self) -> Result<i64> {
        self.db.with_conn(|c| Ok(c.query_row("select count(*) from memories where status='pending'", [], |r| r.get(0))?))
    }

    /// Audit rows whose action or detail (cast to text) case-insensitively contains
    /// `pattern` (a caller-built `LIKE`-escaped substring, wrapped in `%...%`), newest
    /// first, capped at 500. A SQL-level prefilter for global search, so a table that
    /// only ever grows (nothing is hard-deleted from `audit`) still answers in bounded
    /// time. `"at"` is quoted because it's a reserved word.
    ///
    /// `mcp_config_edit` rows are left out on purpose. Global search renders a matching
    /// row's whole `detail` as the hit's title, and that action describes an edit to one
    /// of the user's own agent configuration files. Those rows carry only a path, a
    /// backup file name and a byte count today, but the file they describe is full of
    /// tokens, and one careless field added to that detail later would put every one of
    /// them into a search response. The exclusion is the belt to
    /// `mcp_servers::edit`'s braces.
    pub fn list_audit_for_search(&self, pattern: &str) -> Result<Vec<AuditEntry>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(
                "select id::text, actor, action, entity, entity_id::text, detail::text, \"at\"::text from audit \
                 where action <> 'mcp_config_edit' \
                 and (lower(action) like ? escape '\\' or lower(detail::text) like ? escape '\\') \
                 order by \"at\" desc limit 500",
            )?;
            let rows = st.query_map(params![pattern, pattern], row_to_audit)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
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
    fn facets_count_kinds_tags_and_total_over_active_memories() {
        let db = Db::open_in_memory().unwrap();
        let repo = MemoryRepo::new(&db);
        let mut a = mem("use bun not pnpm", MemoryScope::Global);
        a.tags = vec!["tooling".into(), "runtime".into()];
        let mut b = mem("tailwind lives in css", MemoryScope::Global);
        b.kind = MemoryKind::Decision;
        b.tags = vec!["tooling".into()];
        let c = mem("no tags here", MemoryScope::Global);
        repo.insert(&a, "test").unwrap();
        let inserted_b = repo.insert(&b, "test").unwrap();
        repo.insert(&c, "test").unwrap();
        // Rejected memories are not active, so they must not inflate any count.
        let mut d = mem("rejected one", MemoryScope::Global);
        d.status = MemoryStatus::Rejected;
        repo.insert(&d, "test").unwrap();

        let facets = repo.facets(None, MemoryScopeFilter::All).unwrap();
        assert_eq!(facets.total, 3);
        assert_eq!(facets.kinds.get("fact").copied(), Some(2));
        assert_eq!(facets.kinds.get("decision").copied(), Some(1));
        assert_eq!(facets.tags.get("tooling").copied(), Some(2));
        assert_eq!(facets.tags.get("runtime").copied(), Some(1));
        // `c` kept `mem()`'s default tag.
        assert_eq!(facets.tags.get("t1").copied(), Some(1));

        repo.set_status(inserted_b.id, MemoryStatus::Superseded, "test").unwrap();
        let after = repo.facets(None, MemoryScopeFilter::All).unwrap();
        assert_eq!(after.total, 2);
        assert!(!after.kinds.contains_key("decision"), "the superseded decision drops out");
    }

    #[test]
    fn get_missing_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let repo = MemoryRepo::new(&db);
        assert!(matches!(repo.get(uuid::Uuid::new_v4()), Err(crate::AtlasError::NotFound(_))));
    }
    #[test]
    fn parse_ts_pub_rejects_garbage() {
        assert!(parse_ts_pub("garbage".into()).is_err());
    }
    #[test]
    fn malformed_uuid_row_is_rejected() {
        // CHECK constraints on the `memories` table make it impossible to insert an
        // invalid uuid/enum through the schema, so exercise row_to_memory directly
        // against a hand-built result row with a malformed id column.
        let db = Db::open_in_memory().unwrap();
        let result: Result<Memory> = db.with_conn(|c| {
            let mut st = c.prepare(
                "select 'not-a-uuid' as id, 'global' as scope, null as project_id, 'fact' as kind, \
                 'x' as text, '[]' as tags, null as source_agent, null as source_tool, 1.0 as confidence, \
                 'active' as status, null as superseded_by, '2026-01-01 00:00:00' as created_at, \
                 '2026-01-01 00:00:00' as updated_at",
            )?;
            let mut rows = st.query([])?;
            let r = rows.next()?.ok_or_else(|| AtlasError::NotFound("no row".into()))?;
            Ok(row_to_memory(r)?)
        });
        assert!(result.is_err());
    }
}
