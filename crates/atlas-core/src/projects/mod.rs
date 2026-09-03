pub mod detect;
pub mod profile;
pub use detect::{detect_root, Detected};
pub use profile::build_profile;

use crate::db::Db;
use crate::models::{Project, ProjectProfile};
use crate::{AtlasError, Result};
use duckdb::types::Type;
use duckdb::{params, params_from_iter, Row};
use uuid::Uuid;

pub struct ProjectRepo<'a> {
    db: &'a Db,
}

const SEL: &str = "id::text, name, root_path, git_remote, profile::text, created_at::text, last_seen_at::text, board_key, board_stages::text";

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default. Mirrors `memories::conv_err`.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

fn row(r: &Row) -> duckdb::Result<Project> {
    let board_stages: Option<String> = r.get(8)?;
    let board_stages = board_stages
        .map(|s| serde_json::from_str::<Option<Vec<crate::models::Stage>>>(&s).map_err(|e| conv_err(8, Type::Text, e)))
        .transpose()?
        .flatten();
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
        board_key: r.get(7)?,
        board_stages,
    })
}

impl<'a> ProjectRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    fn by_remote(&self, remote: &str) -> Result<Option<Project>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from projects where git_remote = ?"))?;
            let mut rows = st.query(params![remote])?;
            Ok(match rows.next()? {
                Some(r) => Some(row(r)?),
                None => None,
            })
        })
    }

    fn by_root_without_remote(&self, root: &str) -> Result<Option<Project>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from projects where root_path = ? and git_remote is null"))?;
            let mut rows = st.query(params![root])?;
            Ok(match rows.next()? {
                Some(r) => Some(row(r)?),
                None => None,
            })
        })
    }

    /// Matches an existing project for `d`: by remote when one is given,
    /// falling back to a root-path match against a remote-less row so a
    /// project that gains a remote migrates in place instead of duplicating;
    /// by root path (with no remote) otherwise.
    fn find_key(&self, d: &Detected) -> Result<Option<Project>> {
        let root = d.root.to_string_lossy().to_string();
        match &d.remote {
            Some(remote) => match self.by_remote(remote)? {
                Some(p) => Ok(Some(p)),
                None => self.by_root_without_remote(&root),
            },
            None => self.by_root_without_remote(&root),
        }
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
                            "update projects set root_path = ?, name = ?, git_remote = ?, profile = ?::json, last_seen_at = now() where id = ?",
                            params![root, name, d.remote, pj, existing.id.to_string()],
                        )?;
                    } else {
                        c.execute(
                            "update projects set root_path = ?, git_remote = ?, last_seen_at = now() where id = ?",
                            params![root, d.remote, existing.id.to_string()],
                        )?;
                    }
                    Ok(())
                })?;
                crate::memories::MemoryRepo::new(self.db).audit(actor, "update", "project", Some(existing.id), serde_json::json!({"root": root}))?;
                existing.id
            }
            None => {
                let id = Uuid::new_v4();
                self.db.with_conn(|c| {
                    let board_key = crate::board::pick_board_key(c, &name, None)?;
                    c.execute(
                        "insert into projects (id, name, root_path, git_remote, profile, board_key) values (?, ?, ?, ?, ?::json, ?)",
                        params![id.to_string(), name, root, d.remote, profile_json, board_key],
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

    /// Projects whose name or root path case-insensitively contain `pattern` (a
    /// caller-built `LIKE`-escaped substring, wrapped in `%...%`), restricted to a
    /// single project when `project_id` is given, most-recently-seen first, capped at
    /// 500. A SQL-level prefilter for global search's own "project" group; unrelated
    /// to `list`, which the rest of the daemon still uses for the unfiltered list.
    pub fn search_candidates(&self, project_id: Option<Uuid>, pattern: &str) -> Result<Vec<Project>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {SEL} from projects where (lower(name) like ? escape '\\' or lower(root_path) like ? escape '\\')");
            let mut args: Vec<String> = vec![pattern.to_string(), pattern.to_string()];
            if let Some(p) = project_id {
                sql.push_str(" and id = ?");
                args.push(p.to_string());
            }
            sql.push_str(" order by last_seen_at desc limit 500");
            let mut st = c.prepare(&sql)?;
            Ok(st.query_map(params_from_iter(args.iter()), row)?.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn set_profile(&self, id: Uuid, p: &ProjectProfile, actor: &str) -> Result<Project> {
        let json = serde_json::to_string(p)?;
        self.db.with_conn(|c| {
            c.execute(
                "update projects set profile = ?::json, name = ?, last_seen_at = now() where id = ?",
                params![json, p.name, id.to_string()],
            )?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "update", "project", Some(id), serde_json::json!({"profile": true}))?;
        self.get(id)
    }

    /// Removes the project row, the `sync_targets` that point at it, and its whole
    /// board: the tasks, their blocker links in both directions, and their events.
    ///
    /// The board cascades because a task belongs to its project the way a column belongs
    /// to a board. An orphaned task keeps a `project_id` no row answers to: it is judged
    /// against the global stage list on read but sits outside the scope of a global stage
    /// rename, so it can end up parked in a column no board shows, invisible to
    /// `counts_by_stage` and open forever. Tasks are also not memories: nothing else
    /// reads a task by id after its board is gone, so there is no audit value in keeping
    /// the rows. One `task_delete_cascade` audit row records how many went, when any did.
    ///
    /// Memories scoped to the project are still deliberately left alone: nothing is
    /// ever hard-deleted from `memories`, so they stay readable through the audit trail
    /// and through a re-connect of the same root, which restores the id's meaning only
    /// if the project is added again. Errors with `NotFound` when the id is unknown, so
    /// a repeat call is not a silent success.
    pub fn delete(&self, id: Uuid, actor: &str) -> Result<()> {
        let p = self.get(id)?;
        let tasks = self.db.with_conn(|c| {
            let pid = id.to_string();
            let tasks: i64 = c.query_row("select count(*) from tasks where project_id = ?", params![pid], |r| r.get(0))?;
            c.execute(
                "delete from task_blockers where task_id in (select id from tasks where project_id = ?) \
                 or blocked_by in (select id from tasks where project_id = ?)",
                params![pid, pid],
            )?;
            c.execute("delete from task_events where task_id in (select id from tasks where project_id = ?)", params![pid])?;
            c.execute("delete from tasks where project_id = ?", params![pid])?;
            c.execute("delete from board_counters where scope = ?", params![pid])?;
            c.execute("delete from sync_targets where project_id = ?", params![pid])?;
            c.execute("delete from projects where id = ?", params![pid])?;
            Ok(tasks)
        })?;
        let repo = crate::memories::MemoryRepo::new(self.db);
        if tasks > 0 {
            repo.audit(actor, "task_delete_cascade", "project", Some(id), serde_json::json!({"tasks": tasks}))?;
        }
        repo.audit(actor, "delete", "project", Some(id), serde_json::json!({"root": p.root_path, "name": p.name}))
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
    fn project_migrates_when_it_gains_a_remote() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let a = repo.upsert(&Detected { root: "/tmp/r".into(), remote: None }, None, "t").unwrap();
        assert!(a.git_remote.is_none());
        let b = repo
            .upsert(&Detected { root: "/tmp/r".into(), remote: Some("git@x/r.git".into()) }, None, "t")
            .unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(b.git_remote.as_deref(), Some("git@x/r.git"));
        assert_eq!(repo.list().unwrap().len(), 1);
    }

    #[test]
    fn upsert_update_and_set_profile_are_audited() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let det = Detected { root: "/tmp/audit".into(), remote: None };
        let a = repo.upsert(&det, None, "t").unwrap(); // insert
        repo.upsert(&det, None, "t").unwrap(); // update
        repo.set_profile(a.id, &ProjectProfile { name: "audit".into(), ..Default::default() }, "t").unwrap(); // set_profile
        let audits: i64 = db.with_conn(|c| Ok(c.query_row("select count(*) from audit", [], |r| r.get(0))?)).unwrap();
        assert_eq!(audits, 3);
    }

    #[test]
    fn delete_removes_the_row_and_its_sync_targets_and_is_audited() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let a = repo.upsert(&Detected { root: "/tmp/gone".into(), remote: None }, None, "t").unwrap();
        let b = repo.upsert(&Detected { root: "/tmp/stays".into(), remote: None }, None, "t").unwrap();
        for p in [a.id, b.id] {
            db.with_conn(|c| {
                c.execute(
                    "insert into sync_targets (id, project_id, kind, path) values (?, ?, 'claude', '/tmp/x')",
                    params![Uuid::new_v4().to_string(), p.to_string()],
                )?;
                Ok(())
            })
            .unwrap();
        }

        repo.delete(a.id, "t").unwrap();
        assert!(matches!(repo.get(a.id), Err(AtlasError::NotFound(_))));
        assert_eq!(repo.list().unwrap().len(), 1);
        let targets: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from sync_targets where project_id = ?", params![a.id.to_string()], |r| r.get(0))?))
            .unwrap();
        assert_eq!(targets, 0, "the deleted project's sync targets must go with it");
        let kept: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from sync_targets where project_id = ?", params![b.id.to_string()], |r| r.get(0))?))
            .unwrap();
        assert_eq!(kept, 1, "another project's sync targets must survive");
        let deletes: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from audit where entity = 'project' and action = 'delete'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(deletes, 1);

        // A second delete of the same id is a NotFound, not a silent success.
        assert!(matches!(repo.delete(a.id, "t"), Err(AtlasError::NotFound(_))));
    }

    /// Deleting a project leaves its memories in place; nothing is hard-deleted from
    /// `memories`, so the rows stay readable even though the project row is gone.
    #[test]
    fn delete_leaves_project_scoped_memories_alone() {
        use crate::models::{MemoryKind, MemoryScope, MemoryStatus, NewMemory};
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let p = repo.upsert(&Detected { root: "/tmp/mem".into(), remote: None }, None, "t").unwrap();
        crate::memories::MemoryRepo::new(&db)
            .insert(
                &NewMemory {
                    scope: MemoryScope::Project,
                    project_id: Some(p.id),
                    kind: MemoryKind::Fact,
                    text: "kept".into(),
                    tags: vec![],
                    source_agent: None,
                    source_tool: None,
                    confidence: 1.0,
                    status: MemoryStatus::Active,
                },
                "t",
            )
            .unwrap();
        repo.delete(p.id, "t").unwrap();
        let left: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from memories where project_id = ?", params![p.id.to_string()], |r| r.get(0))?))
            .unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn malformed_uuid_row_is_rejected() {
        // Hand-build a result row with a malformed id column, mirroring
        // memories::tests::malformed_uuid_row_is_rejected.
        let db = Db::open_in_memory().unwrap();
        let result: Result<Project> = db.with_conn(|c| {
            let mut st = c.prepare(
                "select 'not-a-uuid' as id, 'name' as name, '/root' as root_path, null as git_remote, \
                 null as profile, '2026-01-01 00:00:00' as created_at, '2026-01-01 00:00:00' as last_seen_at, \
                 null as board_key, null as board_stages",
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
                 'not json' as profile, '2026-01-01 00:00:00' as created_at, '2026-01-01 00:00:00' as last_seen_at, \
                 null as board_key, null as board_stages",
            )?;
            let mut rows = st.query([])?;
            let r = rows.next()?.ok_or_else(|| AtlasError::NotFound("no row".into()))?;
            Ok(row(r)?)
        });
        assert!(result.is_err());
    }
}
