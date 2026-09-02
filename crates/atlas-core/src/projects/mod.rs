pub mod detect;
pub mod profile;
pub use detect::{detect_root, Detected};
pub use profile::build_profile;

use crate::db::Db;
use crate::models::{Project, ProjectProfile};
use crate::{AtlasError, Result};
use duckdb::types::Type;
use duckdb::{params, Row};
use uuid::Uuid;

pub struct ProjectRepo<'a> {
    db: &'a Db,
}

const SEL: &str = "id::text, name, root_path, git_remote, profile::text, created_at::text, last_seen_at::text";

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default. Mirrors `memories::conv_err`.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

fn row(r: &Row) -> duckdb::Result<Project> {
    let profile: Option<String> = r.get(4)?;
    let profile = profile
        .map(|s| serde_json::from_str::<ProjectProfile>(&s).map_err(|e| conv_err(4, Type::Text, e)))
        .transpose()?;
    Ok(Project {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?,
        name: r.get(1)?,
        root_path: r.get(2)?,
        git_remote: r.get(3)?,
        profile,
        created_at: crate::memories::parse_ts_pub(r.get::<_, String>(5)?)?,
        last_seen_at: crate::memories::parse_ts_pub(r.get::<_, String>(6)?)?,
    })
}

impl<'a> ProjectRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    fn find_key(&self, d: &Detected) -> Result<Option<Project>> {
        let root = d.root.to_string_lossy().to_string();
        self.db.with_conn(|c| {
            let (sql, arg) = match &d.remote {
                Some(r) => (format!("select {SEL} from projects where git_remote = ?"), r.clone()),
                None => (format!("select {SEL} from projects where root_path = ? and git_remote is null"), root),
            };
            let mut st = c.prepare(&sql)?;
            let mut rows = st.query(params![arg])?;
            Ok(match rows.next()? {
                Some(r) => Some(row(r)?),
                None => None,
            })
        })
    }

    /// Inserts a new project or updates an existing one matched by remote
    /// (preferred) or by root path when there is no remote. Updates
    /// `root_path`, `last_seen_at`, and the profile when one is given.
    pub fn upsert(&self, d: &Detected, profile: Option<&ProjectProfile>, actor: &str) -> Result<Project> {
        let root = d.root.to_string_lossy().to_string();
        let name = profile
            .map(|p| p.name.clone())
            .unwrap_or_else(|| d.root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "project".into()));
        let profile_json = profile.map(serde_json::to_string).transpose()?;
        let id = match self.find_key(d)? {
            Some(existing) => {
                self.db.with_conn(|c| {
                    if let Some(ref pj) = profile_json {
                        c.execute(
                            "update projects set root_path = ?, name = ?, profile = ?::json, last_seen_at = now() where id = ?",
                            params![root, name, pj, existing.id.to_string()],
                        )?;
                    } else {
                        c.execute(
                            "update projects set root_path = ?, last_seen_at = now() where id = ?",
                            params![root, existing.id.to_string()],
                        )?;
                    }
                    Ok(())
                })?;
                existing.id
            }
            None => {
                let id = Uuid::new_v4();
                self.db.with_conn(|c| {
                    c.execute(
                        "insert into projects (id, name, root_path, git_remote, profile) values (?, ?, ?, ?, ?::json)",
                        params![id.to_string(), name, root, d.remote, profile_json],
                    )?;
                    Ok(())
                })?;
                crate::memories::MemoryRepo::new(self.db).audit(actor, "insert", "project", Some(id), serde_json::json!({"root": root}))?;
                id
            }
        };
        self.get(id)
    }

    pub fn get(&self, id: Uuid) -> Result<Project> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from projects where id = ?"))?;
            let mut rows = st.query(params![id.to_string()])?;
            match rows.next()? {
                Some(r) => Ok(row(r)?),
                None => Err(AtlasError::NotFound(format!("project {id}"))),
            }
        })
    }

    pub fn by_root(&self, root: &std::path::Path) -> Result<Option<Project>> {
        let root = root.to_string_lossy().to_string();
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from projects where root_path = ?"))?;
            let mut rows = st.query(params![root])?;
            Ok(match rows.next()? {
                Some(r) => Some(row(r)?),
                None => None,
            })
        })
    }

    pub fn list(&self) -> Result<Vec<Project>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from projects order by last_seen_at desc"))?;
            Ok(st.query_map([], row)?.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn set_profile(&self, id: Uuid, p: &ProjectProfile) -> Result<Project> {
        let json = serde_json::to_string(p)?;
        self.db.with_conn(|c| {
            c.execute(
                "update projects set profile = ?::json, name = ?, last_seen_at = now() where id = ?",
                params![json, p.name, id.to_string()],
            )?;
            Ok(())
        })?;
        self.get(id)
    }

    /// True when the project has no profile yet, or its profile is more than
    /// 24 hours old.
    pub fn needs_refresh(&self, p: &Project) -> bool {
        match &p.profile {
            None => true,
            Some(pr) => chrono::Utc::now() - pr.built_at > chrono::Duration::hours(24),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn upsert_matches_by_remote_then_root() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let a = repo
            .upsert(&Detected { root: "/tmp/a".into(), remote: Some("git@x/y.git".into()) }, None, "t")
            .unwrap();
        let b = repo
            .upsert(&Detected { root: "/tmp/moved".into(), remote: Some("git@x/y.git".into()) }, None, "t")
            .unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(b.root_path, "/tmp/moved");
        let c = repo.upsert(&Detected { root: "/tmp/noremote".into(), remote: None }, None, "t").unwrap();
        let c2 = repo.upsert(&Detected { root: "/tmp/noremote".into(), remote: None }, None, "t").unwrap();
        assert_eq!(c.id, c2.id);
        assert_eq!(repo.list().unwrap().len(), 2);
        assert!(repo.needs_refresh(&c));
    }

    #[test]
    fn malformed_uuid_row_is_rejected() {
        // Hand-build a result row with a malformed id column, mirroring
        // memories::tests::malformed_uuid_row_is_rejected.
        let db = Db::open_in_memory().unwrap();
        let result: Result<Project> = db.with_conn(|c| {
            let mut st = c.prepare(
                "select 'not-a-uuid' as id, 'name' as name, '/root' as root_path, null as git_remote, \
                 null as profile, '2026-01-01 00:00:00' as created_at, '2026-01-01 00:00:00' as last_seen_at",
            )?;
            let mut rows = st.query([])?;
            let r = rows.next()?.ok_or_else(|| AtlasError::NotFound("no row".into()))?;
            Ok(row(r)?)
        });
        assert!(result.is_err());
    }

    #[test]
    fn malformed_profile_json_is_rejected() {
        let db = Db::open_in_memory().unwrap();
        let result: Result<Project> = db.with_conn(|c| {
            let mut st = c.prepare(
                "select gen_random_uuid()::text as id, 'name' as name, '/root' as root_path, null as git_remote, \
                 'not json' as profile, '2026-01-01 00:00:00' as created_at, '2026-01-01 00:00:00' as last_seen_at",
            )?;
            let mut rows = st.query([])?;
            let r = rows.next()?.ok_or_else(|| AtlasError::NotFound("no row".into()))?;
            Ok(row(r)?)
        });
        assert!(result.is_err());
    }
}
