pub mod access;
pub mod context;
pub mod detect;
pub mod profile;
pub use access::{access_defaults, actor_is_user, check_memory_write, check_task_move, effective_access, Action, Actor, ActorKind, Decision, PersonaRef};
pub use detect::{detect_root, Detected};
pub use profile::build_profile;

pub mod log;

use crate::db::Db;
use crate::models::{AgentAccess, Project, ProjectExtraction, ProjectPatch, ProjectProfile};
use crate::{AtlasError, Result};
use duckdb::types::Type;
use duckdb::{params, params_from_iter, Row};
use uuid::Uuid;

pub struct ProjectRepo<'a> {
    db: &'a Db,
}

const SEL: &str = "id::text, name, root_path, git_remote, profile::text, created_at::text, last_seen_at::text, board_key, board_stages::text, \
     agent_access::text, extraction::text, mcp_disabled_tools::text, skills_disabled::text";

/// A board key: two to six characters, a letter first, then letters or digits.
/// Uppercased before the check, so `atl` is accepted and stored as `ATL`.
pub fn normalize_board_key(raw: &str) -> Result<String> {
    let key = raw.trim().to_uppercase();
    let shaped = (2..=6).contains(&key.len())
        && key.starts_with(|c: char| c.is_ascii_uppercase())
        && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !shaped {
        return Err(AtlasError::Invalid(format!(
            "board key '{raw}' must be 2 to 6 characters, start with a letter and hold only letters and digits"
        )));
    }
    Ok(key)
}
/// The override as a client may see it: the api key becomes `"***"` when one is
/// stored, the same rule the global `extraction.api_key` follows.
fn mask_extraction(mut e: ProjectExtraction) -> ProjectExtraction {
    if e.api_key.as_deref().is_some_and(|k| !k.is_empty()) {
        e.api_key = Some(crate::settings::MASKED.to_string());
    }
    e
}

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
    let agent_access: Option<String> = r.get(9)?;
    let agent_access = agent_access
        .map(|s| serde_json::from_str::<Option<AgentAccess>>(&s).map_err(|e| conv_err(9, Type::Text, e)))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    // Masked here rather than in each caller, so no read path can hand out the key.
    // `ProjectRepo::extraction_raw` is the one deliberate way to the real value.
    let extraction: Option<String> = r.get(10)?;
    let extraction = extraction
        .map(|s| serde_json::from_str::<Option<ProjectExtraction>>(&s).map_err(|e| conv_err(10, Type::Text, e)))
        .transpose()?
        .flatten()
        .map(mask_extraction);
    let profile: Option<String> = r.get(4)?;
    let profile = profile
        .map(|s| serde_json::from_str::<ProjectProfile>(&s).map_err(|e| conv_err(4, Type::Text, e)))
        .transpose()?;
    let mcp_disabled_tools: Option<String> = r.get(11)?;
    let mcp_disabled_tools = mcp_disabled_tools
        .map(|s| serde_json::from_str::<Option<Vec<String>>>(&s).map_err(|e| conv_err(11, Type::Text, e)))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let skills_disabled: Option<String> = r.get(12)?;
    let skills_disabled = skills_disabled
        .map(|s| serde_json::from_str::<Option<Vec<String>>>(&s).map_err(|e| conv_err(12, Type::Text, e)))
        .transpose()?
        .flatten()
        .unwrap_or_default();
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
        agent_access,
        extraction,
        mcp_disabled_tools,
        skills_disabled,
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

    /// Applies a patch to the project's identity. `name` and `git_remote` are plain
    /// column writes; `board_key` is the one that moves other rows, so it is validated,
    /// checked for uniqueness against the other projects and the global board, and then
    /// applied together with a rename of every task key on this board inside one
    /// transaction. Callers must already hold the task write gate: renaming
    /// `tasks.key` is a board write.
    ///
    /// `board_counters` is scoped by project id, not by key, so a rename leaves the
    /// sequence alone and `NEW-8` follows `OLD-7`.
    pub fn update(&self, id: Uuid, patch: &ProjectPatch, actor: &str) -> Result<Project> {
        let current = self.get(id)?;

        // The new board key, when this patch moves it. Validated and checked for
        // uniqueness before the transaction opens, so a bad key is a plain `Invalid`
        // rather than a rollback.
        let rename = match &patch.board_key {
            None => None,
            Some(raw) => {
                let key = normalize_board_key(raw)?;
                let old = current.board_key.clone().unwrap_or_else(|| crate::board::board_key_base(&current.name));
                if key == old && current.board_key.is_some() {
                    None
                } else {
                    if key == crate::board::GLOBAL_BOARD_KEY {
                        return Err(AtlasError::Invalid(format!("board key '{key}' is reserved for the global board")));
                    }
                    Some((old, key))
                }
            }
        };
        let name = match &patch.name {
            None => None,
            Some(name) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(AtlasError::Invalid("a project needs a name".into()));
                }
                Some(name.to_string())
            }
        };
        // Same known-tool-name rule the global `mcp.disabled_tools` setting validates
        // against, checked before the transaction opens for the same reason the board
        // key is: a bad name is a plain `Invalid`, not a rollback.
        if let Some(names) = &patch.mcp_disabled_tools {
            crate::settings::validate_mcp_tool_names(names)?;
        }

        // One transaction for the whole patch, not one for the rename and a bare write
        // after it: a failure between the two would otherwise leave the board renamed
        // and the name untouched.
        let renamed_tasks = self.db.with_conn(|c| {
            if let Some((_, key)) = &rename {
                let taken: i64 = c.query_row(
                    "select count(*) from projects where board_key = ? and id::text <> ?",
                    params![key, id.to_string()],
                    |r| r.get(0),
                )?;
                if taken > 0 {
                    return Err(AtlasError::Invalid(format!("board key '{key}' is already used by another project")));
                }
            }
            c.execute_batch("begin transaction")?;
            let applied = (|| -> Result<i64> {
                let mut tasks = 0;
                if let Some((_, key)) = &rename {
                    tasks = c.query_row("select count(*) from tasks where project_id = ?", params![id.to_string()], |r| r.get(0))?;
                    c.execute("update projects set board_key = ? where id = ?", params![key, id.to_string()])?;
                    // A task's key is always `<board key>-<seq>`, and `seq` is the stored
                    // column the key was built from, so rebuilding it is exact.
                    c.execute("update tasks set key = ? || '-' || seq where project_id = ?", params![key, id.to_string()])?;
                }
                if let Some(name) = &name {
                    c.execute("update projects set name = ? where id = ?", params![name, id.to_string()])?;
                }
                if let Some(remote) = &patch.git_remote {
                    let remote = remote.as_deref().map(str::trim).filter(|r| !r.is_empty());
                    c.execute("update projects set git_remote = ? where id = ?", params![remote, id.to_string()])?;
                }
                if let Some(names) = &patch.mcp_disabled_tools {
                    let json = serde_json::to_string(names)?;
                    c.execute("update projects set mcp_disabled_tools = ?::json where id = ?", params![json, id.to_string()])?;
                }
                Ok(tasks)
            })();
            match applied {
                Ok(tasks) => {
                    c.execute_batch("commit")?;
                    Ok(tasks)
                }
                Err(e) => {
                    c.execute_batch("rollback")?;
                    Err(e)
                }
            }
        })?;

        let repo = crate::memories::MemoryRepo::new(self.db);
        if let Some((from, to)) = rename {
            repo.audit(actor, "board_key_rename", "project", Some(id), serde_json::json!({"from": from, "to": to, "tasks": renamed_tasks}))?;
        }
        if name.is_some() || patch.git_remote.is_some() {
            repo.audit(actor, "update", "project", Some(id), serde_json::json!({"name": name, "git_remote": patch.git_remote}))?;
        }
        if let Some(names) = &patch.mcp_disabled_tools {
            repo.audit(actor, "set_mcp_disabled_tools", "project", Some(id), serde_json::json!({"disabled": names}))?;
        }
        self.get(id)
    }

    /// Replaces the project's disabled-skill list wholesale; an empty list clears the
    /// override. The ids are not validated here: only the skill service can say which
    /// ids exist right now, so it checks them (`skills::set_project_skills_disabled`)
    /// before calling this.
    pub fn set_skills_disabled(&self, id: Uuid, ids: &[String], actor: &str) -> Result<Project> {
        self.get(id)?;
        let json = serde_json::to_string(ids)?;
        self.db.with_conn(|c| {
            c.execute("update projects set skills_disabled = ?::json where id = ?", params![json, id.to_string()])?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "set_skills_disabled", "project", Some(id), serde_json::json!({"disabled": ids}))?;
        self.get(id)
    }

    /// Replaces the project's agent access rules.
    pub fn set_agent_access(&self, id: Uuid, access: &AgentAccess, actor: &str) -> Result<Project> {
        self.get(id)?;
        let json = serde_json::to_string(access)?;
        self.db.with_conn(|c| {
            c.execute("update projects set agent_access = ?::json where id = ?", params![json, id.to_string()])?;
            Ok(())
        })?;
        crate::memories::MemoryRepo::new(self.db).audit(actor, "set_agent_access", "project", Some(id), serde_json::json!(access))?;
        self.get(id)
    }

    /// The stored override with its api key unmasked, for the extraction worker.
    /// Every other read masks it; this is the one deliberate way to the real value.
    pub fn extraction_raw(&self, id: Uuid) -> Result<Option<ProjectExtraction>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare("select extraction::text from projects where id = ?")?;
            let mut rows = st.query(params![id.to_string()])?;
            let Some(r) = rows.next()? else { return Err(AtlasError::NotFound(format!("project {id}"))) };
            let text: Option<String> = r.get(0)?;
            Ok(text.map(|t| serde_json::from_str::<Option<ProjectExtraction>>(&t)).transpose()?.flatten())
        })
    }

    /// Replaces the project's extraction override, or clears it with `None`.
    ///
    /// Two rules mirror the global setting, for the same reason: an api key of `"***"`
    /// is the GUI saying "leave the key alone", so the stored one is kept; and pointing
    /// `base_url` at a new endpoint without supplying a new key drops the stored key,
    /// because a key entered against one endpoint is not a key for another.
    pub fn set_project_extraction(&self, id: Uuid, over: Option<ProjectExtraction>, actor: &str) -> Result<Project> {
        self.get(id)?;
        let stored = self.extraction_raw(id)?;
        let next = over.map(|mut e| {
            let stored_key = stored.as_ref().and_then(|s| s.api_key.clone()).unwrap_or_default();
            let masked = e.api_key.as_deref() == Some(crate::settings::MASKED);
            if masked {
                e.api_key = (!stored_key.is_empty()).then_some(stored_key.clone());
            }
            let brings_key = e.api_key.as_deref().is_some_and(|k| !k.is_empty() && !masked);
            let moved = match (&e.base_url, stored.as_ref().and_then(|s| s.base_url.as_deref())) {
                (Some(incoming), Some(old)) => !crate::settings::same_endpoint(incoming, old),
                (Some(_), None) => true,
                _ => false,
            };
            if moved && !brings_key && !stored_key.is_empty() {
                tracing::info!("project extraction base_url changed without a new api key; the stored key was cleared");
                e.api_key = None;
            }
            e
        });
        let json = next.as_ref().map(serde_json::to_string).transpose()?;
        self.db.with_conn(|c| {
            c.execute("update projects set extraction = ?::json where id = ?", params![json, id.to_string()])?;
            Ok(())
        })?;
        // The key value itself never reaches the audit log, only whether one is set.
        let detail = match &next {
            None => serde_json::json!({"extraction": null}),
            Some(e) => serde_json::json!({
                "enabled": e.enabled, "base_url": e.base_url, "model": e.model,
                "api_key_set": e.api_key.as_deref().is_some_and(|k| !k.is_empty()),
                "auto_accept_min_confidence": e.auto_accept_min_confidence,
            }),
        };
        crate::memories::MemoryRepo::new(self.db).audit(actor, "set_extraction", "project", Some(id), detail)?;
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
    use crate::models::{AgentAccess, ProjectExtraction, ProjectPatch};
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

    /// A board key rename moves the project's key and every task key with it, in one
    /// go, and leaves one audit row naming what changed.
    #[test]
    fn update_renames_the_board_key_and_every_task_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let p = repo.upsert(&Detected { root: "/tmp/rename".into(), remote: None }, None, "t").unwrap();

        // Two tasks on this board, so the rename has to touch more than one row.
        let db = std::sync::Arc::new(db);
        let board = crate::board::TaskRepo::new(db.clone(), Default::default());
        let a = board
            .create(&crate::models::NewTask { project_id: Some(p.id), title: "first".into(), ..Default::default() }, "t")
            .unwrap();
        let b = board
            .create(&crate::models::NewTask { project_id: Some(p.id), title: "second".into(), ..Default::default() }, "t")
            .unwrap();
        assert!(a.key.ends_with("-1") && b.key.ends_with("-2"), "{} {}", a.key, b.key);

        let repo = ProjectRepo::new(&db);
        // Lower case in, upper case stored.
        let updated = repo.update(p.id, &ProjectPatch { board_key: Some("zed".into()), ..Default::default() }, "t").unwrap();
        assert_eq!(updated.board_key.as_deref(), Some("ZED"));
        assert_eq!(board.get("ZED-1").unwrap().task.title, "first");
        assert_eq!(board.get("ZED-2").unwrap().task.title, "second");

        // The counter is scoped by project id, so the next task carries on from 3.
        let c = board
            .create(&crate::models::NewTask { project_id: Some(p.id), title: "third".into(), ..Default::default() }, "t")
            .unwrap();
        assert_eq!(c.key, "ZED-3");

        let detail: String = db
            .with_conn(|c| {
                Ok(c.query_row("select detail::text from audit where action = 'board_key_rename'", [], |r| r.get(0))?)
            })
            .unwrap();
        assert!(detail.contains("\"to\":\"ZED\"") && detail.contains("\"tasks\":2"), "{detail}");
    }

    /// A key another project already holds, one shaped wrong, and the global board's own
    /// prefix are all refused, and nothing is written.
    #[test]
    fn update_refuses_a_duplicate_or_malformed_board_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let a = repo.upsert(&Detected { root: "/tmp/one".into(), remote: None }, None, "t").unwrap();
        let b = repo.upsert(&Detected { root: "/tmp/two".into(), remote: None }, None, "t").unwrap();
        repo.update(b.id, &ProjectPatch { board_key: Some("TAKEN".into()), ..Default::default() }, "t").unwrap();

        for bad in ["TAKEN", "A", "TOOLONGKEY", "1AB", "A-B", crate::board::GLOBAL_BOARD_KEY] {
            let err = repo.update(a.id, &ProjectPatch { board_key: Some(bad.into()), ..Default::default() }, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(_)), "{bad}: {err}");
        }
        assert_ne!(repo.get(a.id).unwrap().board_key.as_deref(), Some("TAKEN"));
    }

    /// The other two fields of the patch: a rename and a remote that an explicit null
    /// clears.
    #[test]
    fn update_sets_the_name_and_clears_the_remote() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let p = repo.upsert(&Detected { root: "/tmp/named".into(), remote: Some("git@x/y.git".into()) }, None, "t").unwrap();

        let renamed = repo.update(p.id, &ProjectPatch { name: Some("Atlas".into()), ..Default::default() }, "t").unwrap();
        assert_eq!(renamed.name, "Atlas");
        assert_eq!(renamed.git_remote.as_deref(), Some("git@x/y.git"), "an absent remote is left alone");

        let cleared = repo.update(p.id, &ProjectPatch { git_remote: Some(None), ..Default::default() }, "t").unwrap();
        assert!(cleared.git_remote.is_none());
        assert!(matches!(repo.update(p.id, &ProjectPatch { name: Some("  ".into()), ..Default::default() }, "t"), Err(AtlasError::Invalid(_))));
    }

    /// A project's MCP tool override: stored, read back, refused when a name is not a
    /// known tool, and audited under its own action rather than folded into `update`.
    #[test]
    fn update_sets_and_validates_mcp_disabled_tools() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let p = repo.upsert(&Detected { root: "/tmp/mcp".into(), remote: None }, None, "t").unwrap();
        assert!(p.mcp_disabled_tools.is_empty(), "an unset column reads as an empty list");

        let updated = repo
            .update(p.id, &ProjectPatch { mcp_disabled_tools: Some(vec!["task_move".into()]), ..Default::default() }, "t")
            .unwrap();
        assert_eq!(updated.mcp_disabled_tools, vec!["task_move".to_string()]);
        assert_eq!(repo.get(p.id).unwrap().mcp_disabled_tools, vec!["task_move".to_string()]);

        let err = repo
            .update(p.id, &ProjectPatch { mcp_disabled_tools: Some(vec!["no_such_tool".into()]), ..Default::default() }, "t")
            .unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
        // The refused patch changed nothing.
        assert_eq!(repo.get(p.id).unwrap().mcp_disabled_tools, vec!["task_move".to_string()]);

        let cleared = repo.update(p.id, &ProjectPatch { mcp_disabled_tools: Some(vec![]), ..Default::default() }, "t").unwrap();
        assert!(cleared.mcp_disabled_tools.is_empty());

        let audits: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'set_mcp_disabled_tools'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(audits, 2);
    }

    /// The access rules: a `None` list admits anyone, a list is an allow-list matched on
    /// the label or its tool half, and the user's own hands are never checked.
    #[test]
    fn agent_access_persists_and_gates_the_right_actors() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let p = repo.upsert(&Detected { root: "/tmp/access".into(), remote: None }, None, "t").unwrap();
        assert_eq!(p.agent_access, AgentAccess::default(), "an unset column reads as all-null");
        let no_defaults = AgentAccess::default();
        assert!(check_task_move("codex", &p, &no_defaults).is_ok(), "a null list admits anyone");

        let p = repo
            .set_agent_access(
                p.id,
                &AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: Some(vec!["claude-code".into()]), require_review: true },
                "t",
            )
            .unwrap();
        assert!(p.agent_access.require_review);
        assert!(matches!(check_task_move("codex", &p, &no_defaults), Err(AtlasError::Conflict(_))));
        assert!(check_task_move("claude-code", &p, &no_defaults).is_ok());
        assert!(check_task_move("claude-code/reviewer", &p, &no_defaults).is_ok(), "a sub-agent inherits its tool's permission");
        for exempt in ["desktop", "api", "cli", "cli/anything", "scheduler"] {
            assert!(check_task_move(exempt, &p, &no_defaults).is_ok(), "{exempt} is the user's own hands");
        }
        // The exemption is an exact set, not a prefix: `cline` is a real coding agent,
        // and `clippy` and `client-x` are not the CLI either.
        for agent in ["cline", "clippy", "client-x", "cli-bot", "desktop-agent"] {
            assert!(matches!(check_task_move(agent, &p, &no_defaults), Err(AtlasError::Conflict(_))), "{agent} must be checked");
        }
        assert!(matches!(check_memory_write("codex", &p, &no_defaults), Err(AtlasError::Conflict(_))));
        assert!(check_memory_write("cli", &p, &no_defaults).is_ok());
    }

    /// `effective_access`: the project's own value wins when set, the global default
    /// fills in an unset field, and `require_review` is a floor a project can only
    /// raise, never lower, by ORing the two flags.
    #[test]
    fn effective_access_lets_the_project_win_and_the_default_fill_in() {
        let defaults =
            AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: None, require_review: true };

        // An all-null project inherits both lists from the default, and its floor.
        let unset = AgentAccess::default();
        let eff = effective_access(&unset, &defaults);
        assert_eq!(eff.memory_writers, Some(vec!["claude-code".to_string()]));
        assert_eq!(eff.task_movers, None);
        assert!(eff.require_review);

        // A project's own `Some` list wins over the default, whatever it holds.
        let project = AgentAccess { memory_writers: Some(vec!["codex".into()]), task_movers: Some(vec!["codex".into()]), require_review: false };
        let eff = effective_access(&project, &defaults);
        assert_eq!(eff.memory_writers, Some(vec!["codex".to_string()]));
        assert_eq!(eff.task_movers, Some(vec!["codex".to_string()]));
        // `require_review` is true on either side, so the project cannot turn it off.
        assert!(eff.require_review, "the global flag is a floor a project can only raise");

        // Neither side sets it: still all-null and false.
        let eff = effective_access(&unset, &AgentAccess::default());
        assert_eq!(eff, AgentAccess::default());
    }

    /// `access_defaults` reads the three `access.*` settings keys, defaulting to
    /// `AgentAccess::default()` when none are set.
    #[test]
    fn access_defaults_reads_the_settings_keys() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(access_defaults(&crate::settings::SettingsRepo::new(&db)).unwrap(), AgentAccess::default());

        let settings = crate::settings::SettingsRepo::new(&db);
        settings
            .set_many(
                &serde_json::Map::from_iter([
                    ("access.memory_writers".to_string(), serde_json::json!(["claude-code"])),
                    ("access.task_movers".to_string(), serde_json::Value::Null),
                    ("access.require_review".to_string(), serde_json::Value::from(true)),
                ]),
                "t",
            )
            .unwrap();
        let defaults = access_defaults(&settings).unwrap();
        assert_eq!(defaults.memory_writers, Some(vec!["claude-code".to_string()]));
        assert_eq!(defaults.task_movers, None);
        assert!(defaults.require_review);
    }

    /// The override's key is masked on every read, kept when the masked value is sent
    /// back, and dropped when the endpoint moves without a new key.
    #[test]
    fn project_extraction_masks_and_clears_its_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = ProjectRepo::new(&db);
        let p = repo.upsert(&Detected { root: "/tmp/extract".into(), remote: None }, None, "t").unwrap();

        let over = ProjectExtraction {
            enabled: Some(true),
            base_url: Some("https://api.deepseek.com".into()),
            model: Some("deepseek-chat".into()),
            api_key: Some("sk-project".into()),
            auto_accept_min_confidence: Some(0.8),
        };
        let stored = repo.set_project_extraction(p.id, Some(over.clone()), "t").unwrap();
        assert_eq!(stored.extraction.as_ref().unwrap().api_key.as_deref(), Some("***"));
        assert_eq!(repo.get(p.id).unwrap().extraction.unwrap().api_key.as_deref(), Some("***"));
        assert_eq!(repo.extraction_raw(p.id).unwrap().unwrap().api_key.as_deref(), Some("sk-project"));

        // The masked value read back and sent on is "leave the key alone".
        let round_trip = ProjectExtraction { api_key: Some("***".into()), model: Some("deepseek-reasoner".into()), ..over.clone() };
        repo.set_project_extraction(p.id, Some(round_trip), "t").unwrap();
        assert_eq!(repo.extraction_raw(p.id).unwrap().unwrap().api_key.as_deref(), Some("sk-project"));

        // A new endpoint with no new key does not take the old key with it.
        let moved = ProjectExtraction { base_url: Some("http://attacker.example/v1".into()), api_key: None, ..over.clone() };
        repo.set_project_extraction(p.id, Some(moved), "t").unwrap();
        let raw = repo.extraction_raw(p.id).unwrap().unwrap();
        assert_eq!(raw.api_key, None);
        assert_eq!(raw.base_url.as_deref(), Some("http://attacker.example/v1"));

        // The api key value never reaches the audit log.
        let details: Vec<String> = db
            .with_conn(|c| {
                let mut st = c.prepare("select detail::text from audit where action = 'set_extraction'")?;
                Ok(st.query_map([], |r| r.get(0))?.collect::<duckdb::Result<Vec<String>>>()?)
            })
            .unwrap();
        assert!(!details.iter().any(|d| d.contains("sk-project")), "{details:?}");

        // `null` clears the override entirely.
        assert!(repo.set_project_extraction(p.id, None, "t").unwrap().extraction.is_none());
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
                 null as board_key, null as board_stages, null as agent_access, null as extraction, \
                 null as mcp_disabled_tools, null as skills_disabled",
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
                 null as board_key, null as board_stages, null as agent_access, null as extraction, \
                 null as mcp_disabled_tools, null as skills_disabled",
            )?;
            let mut rows = st.query([])?;
            let r = rows.next()?.ok_or_else(|| AtlasError::NotFound("no row".into()))?;
            Ok(row(r)?)
        });
        assert!(result.is_err());
    }
}
