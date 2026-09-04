//! Atlas-native skills: the ones stored in DuckDB rather than found on disk.
//!
//! Shaped like `library::DocRepo`, with two differences the skill format forces: a name
//! is unique per scope (global, or within one project) rather than globally, and it is
//! matched case-insensitively, since a skill's name is what an agent types.

use duckdb::types::Type;
use duckdb::{params, Row};
use uuid::Uuid;

use crate::db::Db;
use crate::models::{MemoryScope, NewSkill, Skill, SkillSource, SkillSummary, SkillUpdate};
use crate::{AtlasError, Result};

/// Validates a native skill name: lowercase letters, digits or `-`, starting with a
/// letter or digit, at most 64 characters. The shape `^[a-z0-9][a-z0-9-]{0,63}$`
/// describes, checked without the `regex` crate since the alphabet is tiny. Stricter
/// than `library::validate_name` (no `_`), because a skill's name is also its folder
/// name wherever one is written out.
pub fn validate_skill_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().next().map(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(AtlasError::Invalid(format!(
            "invalid skill name '{name}': use lowercase letters, digits or -, max 64 chars, starting with a letter or digit"
        )))
    }
}

const SEL: &str = "id::text, project_id::text, name, description, body, created_at::text, updated_at::text";

fn conv_err(col: usize, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

fn row_to_skill(r: &Row) -> duckdb::Result<Skill> {
    let id: String = r.get(0)?;
    let project_id: Option<String> = r.get(1)?;
    let project_id = project_id.map(|s| Uuid::parse_str(&s).map_err(|e| conv_err(1, e))).transpose()?;
    Ok(Skill {
        summary: SkillSummary {
            id: id.clone(),
            source: SkillSource::Native,
            name: r.get(2)?,
            description: r.get(3)?,
            scope: if project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global },
            project_id,
            path: None,
            plugin: None,
            editable: true,
            updated_at: Some(crate::memories::parse_ts_pub(r.get::<_, String>(6)?)?),
            enabled_here: None,
        },
        body: r.get(4)?,
        files: vec![],
    })
}

pub struct SkillRepo<'a> {
    db: &'a Db,
}

impl<'a> SkillRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// `project_id` given: that project's skills plus every global one, the same
    /// widening `DocRepo::list` does. `None`: the global skills alone, since a global
    /// listing must not show one project's skills to another.
    pub fn list(&self, project_id: Option<Uuid>) -> Result<Vec<Skill>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {SEL} from skills");
            let mut args: Vec<String> = vec![];
            match project_id {
                Some(p) => {
                    sql.push_str(" where project_id = ? or project_id is null");
                    args.push(p.to_string());
                }
                None => sql.push_str(" where project_id is null"),
            }
            sql.push_str(" order by name");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(duckdb::params_from_iter(args.iter()), row_to_skill)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn get(&self, id: Uuid) -> Result<Skill> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from skills where id = ?"))?;
            let mut rows = st.query(params![id.to_string()])?;
            match rows.next()? {
                Some(r) => Ok(row_to_skill(r)?),
                None => Err(AtlasError::NotFound(format!("skill {id}"))),
            }
        })
    }

    pub fn create(&self, s: &NewSkill, actor: &str) -> Result<Skill> {
        validate_skill_name(&s.name)?;
        self.check_name_free(&s.name, s.project_id, None)?;
        let id = Uuid::new_v4();
        self.db.with_conn(|c| {
            c.execute(
                "insert into skills (id, project_id, name, description, body) values (?, ?, ?, ?, ?)",
                params![id.to_string(), s.project_id.map(|p| p.to_string()), s.name, s.description, s.body],
            )?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "create", "skill", Some(id), serde_json::json!({"name": s.name}))?;
        self.get(id)
    }

    /// Applies whichever of name, description and body the patch carries. An empty
    /// patch is still a write: it touches `updated_at` and audits, which is what a
    /// caller who sent one asked for.
    pub fn update(&self, id: Uuid, patch: &SkillUpdate, actor: &str) -> Result<Skill> {
        let existing = self.get(id)?;
        if let Some(name) = &patch.name {
            validate_skill_name(name)?;
            self.check_name_free(name, existing.summary.project_id, Some(id))?;
        }
        self.db.with_conn(|c| {
            if let Some(name) = &patch.name {
                c.execute("update skills set name = ? where id = ?", params![name, id.to_string()])?;
            }
            if let Some(description) = &patch.description {
                c.execute("update skills set description = ? where id = ?", params![description, id.to_string()])?;
            }
            if let Some(body) = &patch.body {
                c.execute("update skills set body = ? where id = ?", params![body, id.to_string()])?;
            }
            c.execute("update skills set updated_at = now() where id = ?", params![id.to_string()])?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(
            actor,
            "update",
            "skill",
            Some(id),
            serde_json::json!({"name": patch.name.as_deref().unwrap_or(&existing.summary.name), "body": patch.body.is_some()}),
        )?;
        self.get(id)
    }

    pub fn delete(&self, id: Uuid, actor: &str) -> Result<()> {
        let existing = self.get(id)?;
        self.db.with_conn(|c| {
            c.execute("delete from skills where id = ?", params![id.to_string()])?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "delete", "skill", Some(id), serde_json::json!({"name": existing.summary.name}))
    }

    /// Refuses a name another skill in the same scope already holds, compared without
    /// case: `Deploy` and `deploy` are the same skill to an agent asking for one.
    fn check_name_free(&self, name: &str, project_id: Option<Uuid>, except: Option<Uuid>) -> Result<()> {
        let taken: i64 = self.db.with_conn(|c| {
            let mut sql = String::from("select count(*) from skills where lower(name) = lower(?)");
            let mut args: Vec<String> = vec![name.to_string()];
            match project_id {
                Some(p) => {
                    sql.push_str(" and project_id = ?");
                    args.push(p.to_string());
                }
                None => sql.push_str(" and project_id is null"),
            }
            if let Some(id) = except {
                sql.push_str(" and id::text <> ?");
                args.push(id.to_string());
            }
            Ok(c.query_row(&sql, duckdb::params_from_iter(args.iter()), |r| r.get(0))?)
        })?;
        if taken > 0 {
            return Err(AtlasError::Invalid(format!("a skill named '{name}' already exists in this scope")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_skill(name: &str, project_id: Option<Uuid>) -> NewSkill {
        NewSkill { project_id, name: name.into(), description: "does a thing".into(), body: "# body\n".into() }
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_skill_name("review-pr").is_ok());
        assert!(validate_skill_name("9lives").is_ok());
        for bad in ["", "Review", "under_score", "-lead", "has space", &"a".repeat(65)] {
            assert!(validate_skill_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn crud_round_trip_and_audit() {
        let db = Db::open_in_memory().unwrap();
        let r = SkillRepo::new(&db);
        let s = r.create(&new_skill("review-pr", None), "t").unwrap();
        assert_eq!(s.summary.name, "review-pr");
        assert_eq!(s.summary.scope, MemoryScope::Global);
        assert!(s.summary.editable);

        let updated = r.update(s.summary.id.parse().unwrap(), &SkillUpdate { body: Some("# new\n".into()), ..Default::default() }, "t").unwrap();
        assert_eq!(updated.body, "# new\n");
        assert_eq!(updated.summary.id, s.summary.id);

        r.delete(s.summary.id.parse().unwrap(), "t").unwrap();
        assert!(matches!(r.get(s.summary.id.parse().unwrap()), Err(AtlasError::NotFound(_))));

        let audits: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from audit where entity = 'skill'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(audits, 3, "create, update and delete each append an audit row");
    }

    #[test]
    fn names_are_unique_per_scope_without_case() {
        let db = Db::open_in_memory().unwrap();
        let r = SkillRepo::new(&db);
        let p = Uuid::new_v4();
        r.create(&new_skill("deploy", None), "t").unwrap();
        let clash = r.create(&new_skill("deploy", None), "t").unwrap_err();
        assert!(matches!(clash, AtlasError::Invalid(ref m) if m.contains("already exists")), "{clash}");
        // A different scope may hold the same name.
        r.create(&new_skill("deploy", Some(p)), "t").unwrap();
        // Renaming onto a name the same scope already holds is refused too.
        let other = r.create(&new_skill("other", None), "t").unwrap();
        let rename = r
            .update(other.summary.id.parse().unwrap(), &SkillUpdate { name: Some("deploy".into()), ..Default::default() }, "t")
            .unwrap_err();
        assert!(matches!(rename, AtlasError::Invalid(ref m) if m.contains("already exists")), "{rename}");
    }

    #[test]
    fn list_widens_a_project_to_the_global_skills() {
        let db = Db::open_in_memory().unwrap();
        let r = SkillRepo::new(&db);
        let p = Uuid::new_v4();
        r.create(&new_skill("global-one", None), "t").unwrap();
        r.create(&new_skill("project-one", Some(p)), "t").unwrap();
        assert_eq!(r.list(Some(p)).unwrap().len(), 2);
        assert_eq!(r.list(Some(Uuid::new_v4())).unwrap().len(), 1);
        assert_eq!(r.list(None).unwrap().len(), 1, "a global listing never shows a project's own skills");
    }
}
