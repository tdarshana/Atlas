//! The persona library and each project's roster (Phase 17).
//!
//! Locking follows `TaskRepo`: every write takes the write gate first, then the
//! connection, so a roster replace (which reads the old list before rewriting it)
//! never interleaves with another board or memory write. Audit rows go through
//! `MemoryRepo::audit`, which takes neither the gate nor, while a `with_conn` closure
//! is open, the connection: the rows are written after the closure returns.

use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::*;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use duckdb::types::Type;
use duckdb::{params, Connection, Row};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

/// `position` is quoted throughout: it is a keyword in the grammar DuckDB inherits.
const PERSONA_COLS: &str = "id::text, name, slug, role, summary, instructions, to_json(skills)::text, to_json(workflows)::text, \
     to_json(practices)::text, to_json(mcp_servers)::text, to_json(tools)::text, access::text, models::text, to_json(tags)::text, \
     epoch_us(created_at), epoch_us(updated_at)";

const ROSTER_COLS: &str = "r.persona_id::text, p.name, p.slug, p.role, p.summary, to_json(p.tags)::text, r.is_default, r.\"position\", r.project_id::text";

/// The `agent_use` key and the export file name: lower case, every run of
/// non-alphanumerics folded to one `-`, none at either end.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default. Mirrors `memories::conv_err`.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

fn ts(col: usize, us: i64) -> duckdb::Result<DateTime<Utc>> {
    DateTime::from_timestamp_micros(us).ok_or_else(|| conv_err(col, Type::BigInt, format!("timestamp out of range: {us}")))
}

fn parse_uuid(col: usize, s: String) -> duckdb::Result<Uuid> {
    Uuid::parse_str(&s).map_err(|e| conv_err(col, Type::Text, e))
}

fn parse_json<T: serde::de::DeserializeOwned>(col: usize, s: &str) -> duckdb::Result<T> {
    serde_json::from_str(s).map_err(|e| conv_err(col, Type::Text, e))
}

/// A `text[]` literal. Every element is quote-doubled, the one place user text is
/// spliced into SQL rather than bound, the same rule tag literals follow.
fn list_literal(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|t| format!("'{}'", t.replace('\'', "''"))).collect::<Vec<_>>().join(","))
}

fn row_to_persona(r: &Row) -> duckdb::Result<Persona> {
    Ok(Persona {
        id: parse_uuid(0, r.get(0)?)?,
        name: r.get(1)?,
        slug: r.get(2)?,
        role: r.get(3)?,
        summary: r.get(4)?,
        instructions: r.get(5)?,
        skills: parse_json(6, &r.get::<_, String>(6)?)?,
        workflows: parse_json(7, &r.get::<_, String>(7)?)?,
        practices: parse_json(8, &r.get::<_, String>(8)?)?,
        mcp_servers: parse_json(9, &r.get::<_, String>(9)?)?,
        tools: parse_json(10, &r.get::<_, String>(10)?)?,
        access: parse_json(11, &r.get::<_, String>(11)?)?,
        models: parse_json(12, &r.get::<_, String>(12)?)?,
        tags: parse_json(13, &r.get::<_, String>(13)?)?,
        created_at: ts(14, r.get(14)?)?,
        updated_at: ts(15, r.get(15)?)?,
    })
}

fn row_to_roster(r: &Row) -> duckdb::Result<RosterRow> {
    Ok(RosterRow {
        persona_id: parse_uuid(0, r.get(0)?)?,
        name: r.get(1)?,
        slug: r.get(2)?,
        role: r.get(3)?,
        summary: r.get(4)?,
        tags: parse_json(5, &r.get::<_, String>(5)?)?,
        is_default: r.get(6)?,
        position: r.get(7)?,
        project_id: parse_uuid(8, r.get(8)?)?,
    })
}

/// Every stored field of a persona, as one write sends it.
struct Fields {
    name: String,
    slug: String,
    role: String,
    summary: String,
    instructions: String,
    skills: Vec<String>,
    workflows: Vec<String>,
    practices: Vec<String>,
    mcp_servers: Vec<String>,
    tools: Vec<String>,
    access: PersonaAccess,
    models: BTreeMap<Case, String>,
    tags: Vec<String>,
}

impl Fields {
    fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.slug.is_empty() {
            return Err(AtlasError::Invalid("a persona needs a name".into()));
        }
        for (field, value) in [("name", &self.name), ("role", &self.role)] {
            if value.contains('\n') || value.contains('\r') {
                return Err(AtlasError::Invalid(format!("a persona {field} must not contain a line break")));
            }
        }
        // `review` means "land the memory pending"; a task move or a workflow trigger
        // has no pending state to land in.
        if self.access.task_move == PersonaRule::Review || self.access.workflow_trigger == PersonaRule::Review {
            return Err(AtlasError::Invalid("review only applies to memory_write; task_move and workflow_trigger take allow or deny".into()));
        }
        Ok(())
    }
}

pub struct PersonaRepo {
    db: Arc<Db>,
    gate: Arc<Mutex<()>>,
}

impl PersonaRepo {
    pub fn new(db: Arc<Db>, gate: Arc<Mutex<()>>) -> Self {
        Self { db, gate }
    }

    /// Poison-tolerant, like the lock accessors in `MemoryService`.
    fn gate(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn audit(&self, actor: &str, action: &str, entity: &str, id: Option<Uuid>, detail: serde_json::Value) -> Result<()> {
        MemoryRepo::new(&self.db).audit(actor, action, entity, id, detail)
    }

    pub fn list(&self) -> Result<Vec<Persona>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {PERSONA_COLS} from personas order by lower(name)"))?;
            Ok(st.query_map([], row_to_persona)?.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    /// By id, by slug, or by name compared without case: the three ways a caller can
    /// spell a persona.
    pub fn get(&self, id_or_slug: &str) -> Result<Persona> {
        self.db.with_conn(|c| Self::get_gated(c, id_or_slug))
    }

    fn get_gated(c: &Connection, id_or_slug: &str) -> Result<Persona> {
        let key = id_or_slug.trim();
        let mut st = c.prepare(&format!("select {PERSONA_COLS} from personas where id::text = lower(?) or slug = ? or lower(name) = lower(?)"))?;
        let mut rows = st.query(params![key, key, key])?;
        match rows.next()? {
            Some(r) => Ok(row_to_persona(r)?),
            None => Err(AtlasError::NotFound(format!("persona {key}"))),
        }
    }

    /// Refuses a name (or the slug it derives) another persona already holds.
    fn check_unique(c: &Connection, fields: &Fields, except: Option<Uuid>) -> Result<()> {
        let mut st = c.prepare("select name from personas where (lower(name) = lower(?) or slug = ?) and id::text <> ?")?;
        let mut rows = st.query(params![fields.name, fields.slug, except.map(|id| id.to_string()).unwrap_or_default()])?;
        if let Some(r) = rows.next()? {
            let taken: String = r.get(0)?;
            return Err(AtlasError::Conflict(format!("a persona named '{taken}' already exists")));
        }
        Ok(())
    }

    pub fn create(&self, new: &NewPersona, actor: &str) -> Result<Persona> {
        let _gate = self.gate();
        let name = new.name.trim().to_string();
        let fields = Fields {
            slug: slugify(&name),
            name,
            role: new.role.trim().to_string(),
            summary: new.summary.clone(),
            instructions: new.instructions.clone(),
            skills: new.skills.clone(),
            workflows: new.workflows.clone(),
            practices: new.practices.clone(),
            mcp_servers: new.mcp_servers.clone(),
            tools: new.tools.clone(),
            access: new.access.clone(),
            models: new.models.clone(),
            tags: new.tags.clone(),
        };
        fields.validate()?;
        let id = Uuid::new_v4();
        let persona = self.db.with_conn(|c| {
            Self::check_unique(c, &fields, None)?;
            c.execute(
                &format!(
                    "insert into personas (id, name, slug, role, summary, instructions, skills, workflows, practices, mcp_servers, tools, access, models, tags) \
                     values (?, ?, ?, ?, ?, ?, {}::text[], {}::text[], {}::text[], {}::text[], {}::text[], ?::json, ?::json, {}::text[])",
                    list_literal(&fields.skills),
                    list_literal(&fields.workflows),
                    list_literal(&fields.practices),
                    list_literal(&fields.mcp_servers),
                    list_literal(&fields.tools),
                    list_literal(&fields.tags),
                ),
                params![
                    id.to_string(),
                    fields.name,
                    fields.slug,
                    fields.role,
                    fields.summary,
                    fields.instructions,
                    serde_json::to_string(&fields.access)?,
                    serde_json::to_string(&fields.models)?,
                ],
            )?;
            Self::get_gated(c, &id.to_string())
        })?;
        self.audit(actor, "create", "persona", Some(id), json!({"name": persona.name, "slug": persona.slug}))?;
        Ok(persona)
    }

    /// Applies a patch. A new `name` derives a new slug, which has to be free too.
    pub fn update(&self, id: Uuid, upd: &PersonaUpdate, actor: &str) -> Result<Persona> {
        let _gate = self.gate();
        let persona = self.db.with_conn(|c| {
            let old = Self::get_gated(c, &id.to_string())?;
            let name = upd.name.as_deref().map(str::trim).unwrap_or(&old.name).to_string();
            let fields = Fields {
                slug: slugify(&name),
                name,
                role: upd.role.as_deref().map(str::trim).unwrap_or(&old.role).to_string(),
                summary: upd.summary.clone().unwrap_or(old.summary),
                instructions: upd.instructions.clone().unwrap_or(old.instructions),
                skills: upd.skills.clone().unwrap_or(old.skills),
                workflows: upd.workflows.clone().unwrap_or(old.workflows),
                practices: upd.practices.clone().unwrap_or(old.practices),
                mcp_servers: upd.mcp_servers.clone().unwrap_or(old.mcp_servers),
                tools: upd.tools.clone().unwrap_or(old.tools),
                access: upd.access.clone().unwrap_or(old.access),
                models: upd.models.clone().unwrap_or(old.models),
                tags: upd.tags.clone().unwrap_or(old.tags),
            };
            fields.validate()?;
            Self::check_unique(c, &fields, Some(id))?;
            c.execute(
                &format!(
                    "update personas set name = ?, slug = ?, role = ?, summary = ?, instructions = ?, skills = {}::text[], workflows = {}::text[], \
                     practices = {}::text[], mcp_servers = {}::text[], tools = {}::text[], access = ?::json, models = ?::json, tags = {}::text[], \
                     updated_at = now() where id = ?",
                    list_literal(&fields.skills),
                    list_literal(&fields.workflows),
                    list_literal(&fields.practices),
                    list_literal(&fields.mcp_servers),
                    list_literal(&fields.tools),
                    list_literal(&fields.tags),
                ),
                params![
                    fields.name,
                    fields.slug,
                    fields.role,
                    fields.summary,
                    fields.instructions,
                    serde_json::to_string(&fields.access)?,
                    serde_json::to_string(&fields.models)?,
                    id.to_string(),
                ],
            )?;
            Self::get_gated(c, &id.to_string())
        })?;
        self.audit(actor, "update", "persona", Some(id), json!({"name": persona.name, "slug": persona.slug}))?;
        Ok(persona)
    }

    /// Removes the persona, its roster rows and its mark on every task. Nothing else
    /// is touched: the tasks keep their history.
    pub fn delete(&self, id: Uuid, actor: &str) -> Result<()> {
        let _gate = self.gate();
        let persona = self.db.with_conn(|c| {
            let persona = Self::get_gated(c, &id.to_string())?;
            let key = id.to_string();
            c.execute("delete from project_personas where persona_id = ?", params![key])?;
            c.execute("update tasks set persona_id = null where persona_id = ?", params![key])?;
            c.execute("delete from personas where id = ?", params![key])?;
            Ok(persona)
        })?;
        self.audit(actor, "delete", "persona", Some(id), json!({"name": persona.name, "slug": persona.slug}))
    }

    /// A project's roster in its own order: by `position`, then by name.
    pub fn roster(&self, project_id: Uuid) -> Result<Vec<RosterRow>> {
        self.db.with_conn(|c| Self::roster_gated(c, project_id))
    }

    fn roster_gated(c: &Connection, project_id: Uuid) -> Result<Vec<RosterRow>> {
        let mut st = c.prepare(&format!(
            "select {ROSTER_COLS} from project_personas r join personas p on p.id = r.persona_id where r.project_id = ? order by r.\"position\", lower(p.name)"
        ))?;
        Ok(st.query_map(params![project_id.to_string()], row_to_roster)?.collect::<duckdb::Result<Vec<_>>>()?)
    }

    /// Replaces the whole roster: which personas, which one is the default, and the
    /// order. The audit trail records the difference from the old list, one `assign`,
    /// `unassign` or `set_default` row per persona it touches, then one `roster` row
    /// on the project.
    pub fn set_roster(&self, project_id: Uuid, entries: &[RosterEntry], actor: &str) -> Result<Vec<RosterRow>> {
        let _gate = self.gate();
        if entries.iter().filter(|e| e.is_default).count() > 1 {
            return Err(AtlasError::Invalid("a roster has at most one default persona".into()));
        }
        for (i, e) in entries.iter().enumerate() {
            if entries[..i].iter().any(|o| o.persona_id == e.persona_id) {
                return Err(AtlasError::Invalid(format!("persona {} is listed twice", e.persona_id)));
            }
        }
        let (old, new) = self.db.with_conn(|c| {
            let known: i64 = c.query_row("select count(*) from projects where id = ?", params![project_id.to_string()], |r| r.get(0))?;
            if known == 0 {
                return Err(AtlasError::NotFound(format!("project {project_id}")));
            }
            for e in entries {
                let found: i64 = c.query_row("select count(*) from personas where id = ?", params![e.persona_id.to_string()], |r| r.get(0))?;
                if found == 0 {
                    return Err(AtlasError::Invalid(format!("no persona {}", e.persona_id)));
                }
            }
            let old = Self::roster_gated(c, project_id)?;
            c.execute("delete from project_personas where project_id = ?", params![project_id.to_string()])?;
            for e in entries {
                c.execute(
                    "insert into project_personas (project_id, persona_id, is_default, \"position\") values (?, ?, ?, ?)",
                    params![project_id.to_string(), e.persona_id.to_string(), e.is_default, e.position],
                )?;
            }
            Ok((old, Self::roster_gated(c, project_id)?))
        })?;
        let detail = |row: &RosterRow| json!({"project_id": project_id, "slug": row.slug});
        for row in new.iter().filter(|n| !old.iter().any(|o| o.persona_id == n.persona_id)) {
            self.audit(actor, "assign", "persona", Some(row.persona_id), detail(row))?;
        }
        for row in old.iter().filter(|o| !new.iter().any(|n| n.persona_id == o.persona_id)) {
            self.audit(actor, "unassign", "persona", Some(row.persona_id), detail(row))?;
        }
        let old_default = old.iter().find(|r| r.is_default).map(|r| r.persona_id);
        if let Some(row) = new.iter().find(|r| r.is_default).filter(|r| Some(r.persona_id) != old_default) {
            self.audit(actor, "set_default", "persona", Some(row.persona_id), detail(row))?;
        }
        self.audit(actor, "roster", "project", Some(project_id), json!({"personas": new.iter().map(|r| r.slug.as_str()).collect::<Vec<_>>()}))?;
        Ok(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::TaskRepo;
    use crate::projects::detect::Detected;
    use crate::projects::ProjectRepo;

    fn repo() -> (Arc<Db>, PersonaRepo) {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let repo = PersonaRepo::new(db.clone(), Arc::new(Mutex::new(())));
        (db, repo)
    }

    fn project(db: &Db, root: &str) -> Project {
        ProjectRepo::new(db).upsert(&Detected { root: root.into(), remote: None }, None, "t").unwrap()
    }

    fn named(name: &str) -> NewPersona {
        NewPersona { name: name.into(), ..Default::default() }
    }

    /// Every audit action on entity `persona` or `project`, in insertion order
    /// (`rowid`, since several rows can share one `"at"` microsecond).
    fn audits(db: &Db) -> Vec<(String, String)> {
        db.with_conn(|c| {
            let mut st = c.prepare("select entity, action from audit where entity in ('persona', 'project') order by rowid")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
        .unwrap()
    }

    #[test]
    fn slugify_lowercases_and_folds_runs_of_punctuation() {
        assert_eq!(slugify("Mobile  Developer!"), "mobile-developer");
        assert_eq!(slugify("  Security / Reviewer  "), "security-reviewer");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn create_derives_the_slug_and_keeps_every_field() {
        let (_db, repo) = repo();
        let new = NewPersona {
            name: " Mobile Developer ".into(),
            role: "Builds the mobile app".into(),
            summary: "Knows Expo.".into(),
            instructions: "Prefer Expo Router.".into(),
            skills: vec!["plugin:expo/expo/expo-router".into()],
            workflows: vec!["review".into()],
            practices: vec!["p1".into()],
            mcp_servers: vec!["claude:user:expo".into()],
            tools: vec!["memory_search".into()],
            access: PersonaAccess { memory_write: PersonaRule::Review, ..Default::default() },
            models: BTreeMap::from([(Case::Implement, "sonnet".to_string())]),
            tags: vec!["mobile".into(), "it's".into()],
        };
        let p = repo.create(&new, "t").unwrap();
        assert_eq!((p.name.as_str(), p.slug.as_str()), ("Mobile Developer", "mobile-developer"));
        assert_eq!(p.skills, new.skills);
        assert_eq!(p.tags, new.tags);
        assert_eq!(p.access.memory_write, PersonaRule::Review);
        assert_eq!(p.models.get(&Case::Implement).map(String::as_str), Some("sonnet"));
        assert_eq!(repo.get("mobile-developer").unwrap().id, p.id);
        assert_eq!(repo.get(&p.id.to_string()).unwrap().id, p.id);
        assert_eq!(repo.get("MOBILE developer").unwrap().id, p.id);
        assert!(matches!(repo.get("nobody"), Err(AtlasError::NotFound(_))));
        assert_eq!(repo.list().unwrap().len(), 1);
    }

    #[test]
    fn a_name_is_unique_without_case_and_review_is_memory_write_only() {
        let (_db, repo) = repo();
        repo.create(&named("Mobile Developer"), "t").unwrap();
        assert!(matches!(repo.create(&named("mobile developer"), "t"), Err(AtlasError::Conflict(_))));
        assert!(matches!(repo.create(&named("Mobile-Developer"), "t"), Err(AtlasError::Conflict(_))), "the same slug");
        assert!(matches!(repo.create(&named("  "), "t"), Err(AtlasError::Invalid(_))));
        let bad = NewPersona { access: PersonaAccess { task_move: PersonaRule::Review, ..Default::default() }, ..named("Ops") };
        assert!(matches!(repo.create(&bad, "t"), Err(AtlasError::Invalid(_))));
    }

    #[test]
    fn update_changes_fields_and_bumps_updated_at() {
        let (_db, repo) = repo();
        let p = repo.create(&named("Mobile Developer"), "t").unwrap();
        repo.create(&named("Reviewer"), "t").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let upd = PersonaUpdate { name: Some("Android Developer".into()), role: Some("Android".into()), tools: Some(vec!["task_list".into()]), ..Default::default() };
        let u = repo.update(p.id, &upd, "t").unwrap();
        assert_eq!((u.name.as_str(), u.slug.as_str(), u.role.as_str()), ("Android Developer", "android-developer", "Android"));
        assert_eq!(u.tools, vec!["task_list".to_string()]);
        assert!(u.updated_at > p.updated_at, "{} should be after {}", u.updated_at, p.updated_at);
        assert_eq!(u.created_at, p.created_at);
        let clash = PersonaUpdate { name: Some("reviewer".into()), ..Default::default() };
        assert!(matches!(repo.update(p.id, &clash, "t"), Err(AtlasError::Conflict(_))));
        assert!(matches!(repo.update(Uuid::new_v4(), &upd, "t"), Err(AtlasError::NotFound(_))));
    }

    #[test]
    fn a_roster_has_at_most_one_default_and_follows_position() {
        let (db, repo) = repo();
        let project = project(&db, "/tmp/atlas");
        let a = repo.create(&named("A"), "t").unwrap();
        let b = repo.create(&named("B"), "t").unwrap();
        let two_defaults = [
            RosterEntry { persona_id: a.id, is_default: true, position: 0 },
            RosterEntry { persona_id: b.id, is_default: true, position: 1 },
        ];
        assert!(matches!(repo.set_roster(project.id, &two_defaults, "t"), Err(AtlasError::Invalid(_))));
        let unknown = [RosterEntry { persona_id: Uuid::new_v4(), is_default: false, position: 0 }];
        assert!(matches!(repo.set_roster(project.id, &unknown, "t"), Err(AtlasError::Invalid(_))));
        assert!(matches!(repo.set_roster(Uuid::new_v4(), &[], "t"), Err(AtlasError::NotFound(_))));

        let entries = [
            RosterEntry { persona_id: a.id, is_default: false, position: 2 },
            RosterEntry { persona_id: b.id, is_default: true, position: 1 },
        ];
        let rows = repo.set_roster(project.id, &entries, "t").unwrap();
        assert_eq!(rows.iter().map(|r| r.slug.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(rows[0].is_default && !rows[1].is_default);
        assert_eq!((rows[0].position, rows[0].project_id), (1, project.id));
        assert_eq!(repo.roster(project.id).unwrap(), rows);
        assert!(repo.roster(Uuid::new_v4()).unwrap().is_empty());
    }

    #[test]
    fn delete_cascades_the_roster_and_nulls_the_task_persona() {
        let (db, repo) = repo();
        let project = project(&db, "/tmp/atlas");
        let p = repo.create(&named("Mobile Developer"), "t").unwrap();
        repo.set_roster(project.id, &[RosterEntry { persona_id: p.id, is_default: true, position: 0 }], "t").unwrap();
        let tasks = TaskRepo::new(db.clone(), Arc::new(Mutex::new(())));
        let task = tasks.create(&NewTask { project_id: Some(project.id), title: "ship".into(), persona: Some("mobile-developer".into()), ..Default::default() }, "t").unwrap();
        assert_eq!(task.persona_id, Some(p.id));

        repo.delete(p.id, "t").unwrap();
        assert!(matches!(repo.get("mobile-developer"), Err(AtlasError::NotFound(_))));
        assert!(repo.roster(project.id).unwrap().is_empty());
        let again = tasks.get(&task.key).unwrap().task;
        assert_eq!((again.persona_id, again.persona_name, again.persona_slug), (None, None, None));
        assert!(matches!(repo.delete(p.id, "t"), Err(AtlasError::NotFound(_))));
    }

    #[test]
    fn every_write_leaves_an_audit_row_with_its_action() {
        let (db, repo) = repo();
        let project = project(&db, "/tmp/atlas");
        let a = repo.create(&named("A"), "t").unwrap();
        let b = repo.create(&named("B"), "t").unwrap();
        repo.update(a.id, &PersonaUpdate { role: Some("r".into()), ..Default::default() }, "t").unwrap();
        repo.set_roster(project.id, &[RosterEntry { persona_id: a.id, is_default: true, position: 0 }], "t").unwrap();
        repo.set_roster(project.id, &[RosterEntry { persona_id: b.id, is_default: false, position: 0 }], "t").unwrap();
        repo.delete(b.id, "t").unwrap();
        let expected = [
            ("project", "insert"),
            ("persona", "create"),
            ("persona", "create"),
            ("persona", "update"),
            ("persona", "assign"),
            ("persona", "set_default"),
            ("project", "roster"),
            ("persona", "assign"),
            ("persona", "unassign"),
            ("project", "roster"),
            ("persona", "delete"),
        ];
        let rows = audits(&db);
        let seen: Vec<(&str, &str)> = rows.iter().map(|(e, a)| (e.as_str(), a.as_str())).collect();
        assert_eq!(seen, expected);
    }
}
