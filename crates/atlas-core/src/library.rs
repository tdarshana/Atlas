use crate::db::Db;
use crate::models::*;
use crate::{AtlasError, Result};
use duckdb::types::Type;
use duckdb::{params, Row};
use uuid::Uuid;

/// Validates agent/doc names: lowercase letters, digits, `-` or `_`, starting
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

/// Rejects a newline in a field the exporters write as a single frontmatter line.
/// A value carrying one would end that line early and the rest would be read back
/// by `import` as a different key, so an agent could rewrite its own `tools` or
/// `model` through its description.
fn single_line(field: &str, value: &str) -> Result<()> {
    if value.contains('\n') || value.contains('\r') {
        return Err(AtlasError::Invalid(format!("{field} must not contain a line break")));
    }
    Ok(())
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

// ---- agents ----

pub struct AgentRepo<'a> {
    db: &'a Db,
}

const AGENT_SEL: &str =
    "id::text, name, description, instructions, model_hint, to_json(tools)::text, to_json(tags)::text, version, created_at::text, updated_at::text";

fn row_to_agent(r: &Row) -> duckdb::Result<Agent> {
    let tools_json: String = r.get(5)?;
    let tags_json: String = r.get(6)?;
    Ok(Agent {
        id: parse_uuid_col(0, r.get::<_, String>(0)?)?,
        name: r.get(1)?,
        description: r.get(2)?,
        instructions: r.get(3)?,
        model_hint: r.get(4)?,
        tools: parse_list_col(5, &tools_json)?,
        tags: parse_list_col(6, &tags_json)?,
        version: r.get(7)?,
        created_at: crate::memories::parse_ts_pub(r.get::<_, String>(8)?)?,
        updated_at: crate::memories::parse_ts_pub(r.get::<_, String>(9)?)?,
    })
}

impl<'a> AgentRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Inserts a new agent (version 1) or, when `name` already exists, updates
    /// its fields and bumps `version`.
    pub fn save(&self, a: &NewAgent, actor: &str) -> Result<Agent> {
        validate_name(&a.name)?;
        single_line("agent name", &a.name)?;
        single_line("agent description", &a.description)?;
        if let Some(hint) = &a.model_hint {
            single_line("agent model hint", hint)?;
        }
        for tool in &a.tools {
            single_line("agent tool", tool)?;
        }
        for tag in &a.tags {
            single_line("agent tag", tag)?;
        }
        let tools_list = list_literal(&a.tools);
        let tags_list = list_literal(&a.tags);
        let existing: Option<Uuid> = self.db.with_conn(|c| {
            let mut st = c.prepare("select id::text from agents where name = ?")?;
            let mut rows = st.query(params![a.name])?;
            match rows.next()? {
                Some(r) => Ok(Some(parse_uuid_col(0, r.get::<_, String>(0)?)?)),
                None => Ok(None),
            }
        })?;
        let (id, action) = match existing {
            Some(id) => {
                self.db.with_conn(|c| {
                    c.execute(
                        &format!(
                            "update agents set description = ?, instructions = ?, model_hint = ?, tools = {tools_list}::text[], tags = {tags_list}::text[], version = version + 1, updated_at = now() where id = ?"
                        ),
                        params![a.description, a.instructions, a.model_hint, id.to_string()],
                    )?;
                    Ok(())
                })?;
                (id, "update")
            }
            None => {
                let id = Uuid::new_v4();
                self.db.with_conn(|c| {
                    c.execute(
                        &format!(
                            "insert into agents (id, name, description, instructions, model_hint, tools, tags, version) values (?, ?, ?, ?, ?, {tools_list}::text[], {tags_list}::text[], 1)"
                        ),
                        params![id.to_string(), a.name, a.description, a.instructions, a.model_hint],
                    )?;
                    Ok(())
                })?;
                (id, "insert")
            }
        };
        crate::memories::MemoryRepo::new(self.db).audit(actor, action, "agent", Some(id), serde_json::json!({"name": a.name}))?;
        self.get(&a.name)
    }

    pub fn get(&self, name: &str) -> Result<Agent> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {AGENT_SEL} from agents where name = ?"))?;
            let mut rows = st.query(params![name])?;
            match rows.next()? {
                Some(r) => Ok(row_to_agent(r)?),
                None => Err(AtlasError::NotFound(format!("agent {name}"))),
            }
        })
    }

    pub fn list(&self) -> Result<Vec<Agent>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {AGENT_SEL} from agents order by name"))?;
            Ok(st.query_map([], row_to_agent)?.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn delete(&self, name: &str, actor: &str) -> Result<()> {
        let a = self.get(name)?;
        self.db.with_conn(|c| {
            c.execute("delete from agents where id = ?", params![a.id.to_string()])?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "delete", "agent", Some(a.id), serde_json::json!({"name": name}))
    }
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

    #[test]
    fn agent_save_is_upsert_with_version_bump() {
        let db = Db::open_in_memory().unwrap();
        let r = AgentRepo::new(&db);
        let a = r
            .save(
                &NewAgent { name: "reviewer".into(), description: "reviews PRs".into(), instructions: "Be strict.".into(), model_hint: None, tools: vec!["Read".into()], tags: vec![] },
                "t",
            )
            .unwrap();
        assert_eq!(a.version, 1);
        let b = r
            .save(
                &NewAgent {
                    name: "reviewer".into(),
                    description: "reviews PRs carefully".into(),
                    instructions: "Be strict.".into(),
                    model_hint: Some("opus".into()),
                    tools: vec![],
                    tags: vec!["qa".into()],
                },
                "t",
            )
            .unwrap();
        assert_eq!(b.id, a.id);
        assert_eq!(b.version, 2);
        assert_eq!(b.model_hint.as_deref(), Some("opus"));
        assert_eq!(r.list().unwrap().len(), 1);
        r.delete("reviewer", "t").unwrap();
        assert!(matches!(r.get("reviewer"), Err(crate::AtlasError::NotFound(_))));
    }

    /// A newline in an exported field would break the Claude frontmatter, so it is
    /// refused at the door rather than escaped in each exporter.
    #[test]
    fn agent_fields_reject_line_breaks() {
        let db = Db::open_in_memory().unwrap();
        let r = AgentRepo::new(&db);
        let base = NewAgent { name: "reviewer".into(), description: "reviews PRs".into(), instructions: "Be strict.".into(), model_hint: None, tools: vec![], tags: vec![] };
        let err = r.save(&NewAgent { description: "reviews PRs\ntools: Bash".into(), ..base.clone() }, "t").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(ref m) if m.contains("line break")), "{err}");
        assert!(r.save(&NewAgent { model_hint: Some("opus\nx".into()), ..base.clone() }, "t").is_err());
        assert!(r.save(&NewAgent { tools: vec!["Read\nx".into()], ..base.clone() }, "t").is_err());
        assert!(r.save(&NewAgent { tags: vec!["qa\rx".into()], ..base.clone() }, "t").is_err());
        // The instructions are the file body, not a frontmatter line, so they may wrap.
        assert!(r.save(&NewAgent { instructions: "Be strict.\n\nAlways.".into(), ..base }, "t").is_ok());
    }

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
