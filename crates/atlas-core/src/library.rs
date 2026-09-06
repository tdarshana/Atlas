use crate::db::Db;
use crate::models::*;
use crate::{AtlasError, Result};
use duckdb::types::Type;
use duckdb::{params, Row};
use uuid::Uuid;

/// Validates doc names: lowercase letters, digits, `-` or `_`, starting
/// with a letter or digit, max 64 chars. Implemented without the `regex`
/// crate since the alphabet is tiny.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().next().map(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AtlasError::Invalid(format!(
            "invalid name '{name}': use lowercase letters, digits, - or _, max 64 chars, starting with a letter or digit"
        )))
    }
}

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default. Mirrors `memories::conv_err`.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

fn parse_uuid_col(col: usize, s: String) -> duckdb::Result<Uuid> {
    Uuid::parse_str(&s).map_err(|e| conv_err(col, Type::Text, e))
}

fn parse_list_col(col: usize, json: &str) -> duckdb::Result<Vec<String>> {
    serde_json::from_str(json).map_err(|e| conv_err(col, Type::Text, e))
}

fn list_literal(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|t| format!("'{}'", t.replace('\'', "''"))).collect::<Vec<_>>().join(","))
}

// ---- practices / workflows ----

pub struct DocRepo<'a> {
    db: &'a Db,
    kind: DocKind,
}

fn table_for(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Practice => "practices",
        // Migration 6 gave `workflows` to the real workflow table and moved the
        // Markdown documents to `workflow_docs`, where they wait for
        // `workflow::migrate_docs` to turn each into a single-action workflow.
        DocKind::Workflow => "workflow_docs",
    }
}

const DOC_SEL: &str = "id::text, name, body, to_json(tags)::text, project_id::text, created_at::text, updated_at::text";

fn row_to_doc(kind: DocKind, r: &Row) -> duckdb::Result<Doc> {
    let tags_json: String = r.get(3)?;
    let project_id: Option<String> = r.get(4)?;
    Ok(Doc {
        id: parse_uuid_col(0, r.get::<_, String>(0)?)?,
        kind,
        name: r.get(1)?,
        body: r.get(2)?,
        tags: parse_list_col(3, &tags_json)?,
        project_id: project_id.map(|s| parse_uuid_col(4, s)).transpose()?,
        created_at: crate::memories::parse_ts_pub(r.get::<_, String>(5)?)?,
        updated_at: crate::memories::parse_ts_pub(r.get::<_, String>(6)?)?,
    })
}

impl<'a> DocRepo<'a> {
    pub fn new(db: &'a Db, kind: DocKind) -> Self {
        Self { db, kind }
    }

    /// Inserts a new doc or, when `name` already exists (within this kind's
    /// table), updates its body/tags/project_id.
    pub fn save(&self, d: &NewDoc, actor: &str) -> Result<Doc> {
        validate_name(&d.name)?;
        let table = table_for(self.kind);
        let tags_list = list_literal(&d.tags);
        let existing: Option<Uuid> = self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select id::text from {table} where name = ?"))?;
            let mut rows = st.query(params![d.name])?;
            match rows.next()? {
                Some(r) => Ok(Some(parse_uuid_col(0, r.get::<_, String>(0)?)?)),
                None => Ok(None),
            }
        })?;
        let (id, action) = match existing {
            Some(id) => {
                self.db.with_conn(|c| {
                    c.execute(
                        &format!("update {table} set body = ?, tags = {tags_list}::text[], project_id = ?, updated_at = now() where id = ?"),
                        params![d.body, d.project_id.map(|p| p.to_string()), id.to_string()],
                    )?;
                    Ok(())
                })?;
                (id, "update")
            }
            None => {
                let id = Uuid::new_v4();
                self.db.with_conn(|c| {
                    c.execute(
                        &format!("insert into {table} (id, name, body, tags, project_id) values (?, ?, ?, {tags_list}::text[], ?)"),
                        params![id.to_string(), d.name, d.body, d.project_id.map(|p| p.to_string())],
                    )?;
                    Ok(())
                })?;
                (id, "insert")
            }
        };
        crate::memories::MemoryRepo::new(self.db).audit(actor, action, self.kind.as_str(), Some(id), serde_json::json!({"name": d.name}))?;
        self.get(&d.name)
    }

    pub fn get(&self, name: &str) -> Result<Doc> {
        let table = table_for(self.kind);
        let kind = self.kind;
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {DOC_SEL} from {table} where name = ?"))?;
            let mut rows = st.query(params![name])?;
            match rows.next()? {
                Some(r) => Ok(row_to_doc(kind, r)?),
                None => Err(AtlasError::NotFound(format!("{kind} {name}"))),
            }
        })
    }

    /// `project_id` given: docs scoped to that project OR global (null
    /// `project_id`). `None`: every doc of this kind.
    pub fn list(&self, project_id: Option<Uuid>) -> Result<Vec<Doc>> {
        let table = table_for(self.kind);
        let kind = self.kind;
        self.db.with_conn(|c| {
            let mut sql = format!("select {DOC_SEL} from {table}");
            let mut args: Vec<String> = vec![];
            if let Some(p) = project_id {
                sql.push_str(" where project_id = ? or project_id is null");
                args.push(p.to_string());
            }
            sql.push_str(" order by name");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(duckdb::params_from_iter(args.iter()), |r| row_to_doc(kind, r))?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    /// Docs of this kind whose name or body case-insensitively contain `pattern` (a
    /// caller-built `LIKE`-escaped substring, wrapped in `%...%`), scoped like `list`
    /// (a project given also matches every global doc), newest-updated first, capped
    /// at 500. A SQL-level prefilter for global search.
    pub fn search_candidates(&self, project_id: Option<Uuid>, pattern: &str) -> Result<Vec<Doc>> {
        let table = table_for(self.kind);
        let kind = self.kind;
        self.db.with_conn(|c| {
            let mut sql = format!("select {DOC_SEL} from {table} where (lower(name) like ? escape '\\' or lower(body) like ? escape '\\')");
            let mut args: Vec<String> = vec![pattern.to_string(), pattern.to_string()];
            if let Some(p) = project_id {
                sql.push_str(" and (project_id = ? or project_id is null)");
                args.push(p.to_string());
            }
            sql.push_str(" order by updated_at desc limit 500");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(duckdb::params_from_iter(args.iter()), |r| row_to_doc(kind, r))?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn delete(&self, name: &str, actor: &str) -> Result<()> {
        let d = self.get(name)?;
        let table = table_for(self.kind);
        self.db.with_conn(|c| {
            c.execute(&format!("delete from {table} where id = ?"), params![d.id.to_string()])?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "delete", self.kind.as_str(), Some(d.id), serde_json::json!({"name": name}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

        /// A newline in an exported field would break the Claude frontmatter, so it is
    /// refused at the door rather than escaped in each exporter.
        #[test]
    fn names_are_validated() {
        assert!(validate_name("ok-name_1").is_ok());
        for bad in ["", "Bad", "has space", "-lead", &"a".repeat(65)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn docs_scope_to_project_plus_global() {
        let db = Db::open_in_memory().unwrap();
        let r = DocRepo::new(&db, DocKind::Practice);
        let p = uuid::Uuid::new_v4();
        r.save(&NewDoc { name: "commits".into(), body: "imperative mood".into(), tags: vec![], project_id: None }, "t").unwrap();
        r.save(&NewDoc { name: "deploy".into(), body: "fly deploy".into(), tags: vec![], project_id: Some(p) }, "t").unwrap();
        assert_eq!(r.list(None).unwrap().len(), 2);
        assert_eq!(r.list(Some(p)).unwrap().len(), 2);
        assert_eq!(r.list(Some(uuid::Uuid::new_v4())).unwrap().len(), 1);
        assert_eq!(DocRepo::new(&db, DocKind::Workflow).list(None).unwrap().len(), 0);
    }
}
