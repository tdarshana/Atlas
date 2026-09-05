//! The task board: tasks, blockers, subtasks, events, and the stage rules that
//! govern them.
//!
//! # Locking
//!
//! `ProjectRepo` and `SettingsRepo` rely on the `Db` connection mutex alone. That is
//! not enough here: `next_key` reads `max(seq)` and then inserts, and every stage
//! change reads the current list before rewriting it. `TaskRepo` therefore takes the
//! same write gate the memory path uses, in the same order (gate first, then the
//! connection), so a board write and a `remember` never interleave a read-modify-write.
//! `MemoryService::gate_handle` hands out that mutex.
//!
//! The gate is a blocking, non-reentrant mutex. Every public method here takes it once
//! at the top and calls only private `*_gated` helpers below that point. `TaskRepo`
//! never calls a `MemoryService` method, which would deadlock on the same gate; it
//! writes audit rows through `MemoryRepo::audit`, which does not take the gate.

pub mod render;
pub mod stages;

pub use stages::{default_stages, find_stage, parse_stages, unknown_stage, validate_stages};

use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::*;
use crate::settings::SettingsRepo;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use duckdb::types::Type;
use duckdb::{params, params_from_iter, Connection, Row};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

/// Key prefix for tasks that belong to no project.
pub const GLOBAL_BOARD_KEY: &str = "ATLAS";
/// Settings key holding the global stage list.
pub const STAGES_SETTING: &str = "board.stages";
/// Settings key for the optional `TASKS.md` mirror, read by `atlas sync`.
pub const MIRROR_SETTING: &str = "board.mirror_tasks_md";

const TASK_COLS: &str = "id::text, key, project_id::text, seq, title, description, stage, kind, priority, \
     assignee, labels::text, parent_id::text, created_by, epoch_us(created_at), epoch_us(updated_at), epoch_us(closed_at), source_ref::text, \
     (select p.key from tasks p where p.id = tasks.parent_id), (select p.title from tasks p where p.id = tasks.parent_id)";

const EVENT_COLS: &str = "id::text, task_id::text, actor, kind, body, detail::text, epoch_us(created_at)";

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default. Mirrors `memories::conv_err`.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

/// Timestamps come back as microseconds since the epoch rather than text. DuckDB
/// renders a `timestamptz` in the session time zone, and whether the ICU extension
/// has been loaded yet changes that rendering mid-session, so text would be a moving
/// target; `epoch_us` is the same integer either way.
fn ts(col: usize, us: i64) -> duckdb::Result<DateTime<Utc>> {
    DateTime::from_timestamp_micros(us).ok_or_else(|| conv_err(col, Type::BigInt, format!("timestamp out of range: {us}")))
}

fn parse_uuid(col: usize, s: Option<String>) -> duckdb::Result<Option<Uuid>> {
    s.map(|v| Uuid::parse_str(&v).map_err(|e| conv_err(col, Type::Text, e))).transpose()
}

fn row_to_task(r: &Row) -> duckdb::Result<Task> {
    let labels: String = r.get(10)?;
    Ok(Task {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?,
        key: r.get(1)?,
        project_id: parse_uuid(2, r.get::<_, Option<String>>(2)?)?,
        seq: r.get(3)?,
        title: r.get(4)?,
        description: r.get(5)?,
        stage: r.get(6)?,
        kind: r.get::<_, String>(7)?.parse().map_err(|e: AtlasError| conv_err(7, Type::Text, e))?,
        priority: r.get::<_, String>(8)?.parse().map_err(|e: AtlasError| conv_err(8, Type::Text, e))?,
        assignee: r.get(9)?,
        labels: serde_json::from_str(&labels).map_err(|e| conv_err(10, Type::Text, e))?,
        parent_id: parse_uuid(11, r.get::<_, Option<String>>(11)?)?,
        parent_key: r.get(17)?,
        parent_title: r.get(18)?,
        created_by: r.get(12)?,
        created_at: ts(13, r.get(13)?)?,
        updated_at: ts(14, r.get(14)?)?,
        closed_at: r.get::<_, Option<i64>>(15)?.map(|v| ts(15, v)).transpose()?,
        source_ref: r
            .get::<_, Option<String>>(16)?
            .map(|s| serde_json::from_str(&s).map_err(|e| conv_err(16, Type::Text, e)))
            .transpose()?,
        // Filled in by `decorate`; never stored.
        blocked_by: Vec::new(),
        open_blockers: 0,
        ready: false,
        blocked_reason: None,
        subtasks_total: 0,
        subtasks_done: 0,
    })
}

fn row_to_event(r: &Row) -> duckdb::Result<TaskEvent> {
    let detail: Option<String> = r.get(5)?;
    Ok(TaskEvent {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?,
        task_id: Uuid::parse_str(&r.get::<_, String>(1)?).map_err(|e| conv_err(1, Type::Text, e))?,
        actor: r.get(2)?,
        kind: r.get(3)?,
        body: r.get(4)?,
        detail: detail.map(|d| serde_json::from_str(&d).map_err(|e| conv_err(5, Type::Text, e))).transpose()?,
        created_at: ts(6, r.get(6)?)?,
    })
}

/// Joined with `tasks` so global search can show which board key and project an
/// event belongs to without a second query per event.
const EVENT_SEARCH_COLS: &str =
    "t.key, t.project_id::text, e.id::text, e.task_id::text, e.actor, e.kind, e.body, e.detail::text, epoch_us(e.created_at)";

fn row_to_event_for_search(r: &Row) -> duckdb::Result<(String, Option<Uuid>, TaskEvent)> {
    let key: String = r.get(0)?;
    let project_id = parse_uuid(1, r.get::<_, Option<String>>(1)?)?;
    let detail: Option<String> = r.get(7)?;
    let event = TaskEvent {
        id: Uuid::parse_str(&r.get::<_, String>(2)?).map_err(|e| conv_err(2, Type::Text, e))?,
        task_id: Uuid::parse_str(&r.get::<_, String>(3)?).map_err(|e| conv_err(3, Type::Text, e))?,
        actor: r.get(4)?,
        kind: r.get(5)?,
        body: r.get(6)?,
        detail: detail.map(|d| serde_json::from_str(&d).map_err(|e| conv_err(7, Type::Text, e))).transpose()?,
        created_at: ts(8, r.get(8)?)?,
    };
    Ok((key, project_id, event))
}

/// The first three letters or digits of `name`, uppercased and padded with `X`.
/// Non-ASCII characters are skipped so the key stays typeable in a CLI argument.
pub fn board_key_base(name: &str) -> String {
    let mut key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(3).collect::<String>().to_uppercase();
    while key.len() < 3 {
        key.push('X');
    }
    key
}

/// `board_key_base` made unique against the other projects: `ATL`, then `ATL2`,
/// `ATL3`, and so on. Run inside the caller's `with_conn` closure so the scan and
/// the write that follows it share one hold of the connection mutex.
pub fn pick_board_key(c: &Connection, name: &str, except: Option<Uuid>) -> duckdb::Result<String> {
    let base = board_key_base(name);
    let except = except.map(|e| e.to_string()).unwrap_or_default();
    for n in 1..10_000 {
        let candidate = if n == 1 { base.clone() } else { format!("{base}{n}") };
        let taken: i64 = c.query_row(
            "select count(*) from projects where board_key = ? and id::text <> ?",
            params![candidate, except],
            |r| r.get(0),
        )?;
        if taken == 0 {
            return Ok(candidate);
        }
    }
    Err(conv_err(0, Type::Text, format!("no free board key for '{name}'")))
}

/// The stage lists in play for a set of tasks, one per owning project.
struct StageIndex {
    by_project: HashMap<Option<Uuid>, Vec<Stage>>,
}

impl StageIndex {
    fn is_done(&self, project_id: Option<Uuid>, stage: &str) -> bool {
        self.by_project
            .get(&project_id)
            .and_then(|s| find_stage(s, stage))
            .map(|s| s.done)
            // A task sitting in a stage that is no longer on the board is open, not done.
            .unwrap_or(false)
    }
}

pub struct TaskRepo {
    db: Arc<Db>,
    gate: Arc<Mutex<()>>,
}

impl TaskRepo {
    pub fn new(db: Arc<Db>, gate: Arc<Mutex<()>>) -> Self {
        Self { db, gate }
    }

    /// Poison-tolerant, like the lock accessors in `MemoryService`: a panic raised
    /// while the gate was held must not fail every later board write.
    fn gate(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(|e| e.into_inner())
    }

    // -- stages ------------------------------------------------------------

    /// The list a project's tasks are moved through: the project's own
    /// `board_stages` when it has one, else the `board.stages` setting, else the
    /// built-in default.
    pub fn effective_stages(&self, project_id: Option<Uuid>) -> Result<StageList> {
        self.db.with_conn(|c| self.stages_for(c, project_id))
    }

    fn global_stages(&self, c: &Connection) -> Result<Vec<Stage>> {
        let mut st = c.prepare("select value::text from settings where key = ?")?;
        let mut rows = st.query(params![STAGES_SETTING])?;
        match rows.next()? {
            Some(r) => {
                let text: String = r.get(0)?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                if v.is_null() {
                    Ok(default_stages())
                } else {
                    parse_stages(v)
                }
            }
            None => Ok(default_stages()),
        }
    }

    fn stages_for(&self, c: &Connection, project_id: Option<Uuid>) -> Result<StageList> {
        if let Some(pid) = project_id {
            let mut st = c.prepare("select board_stages::text from projects where id = ?")?;
            let mut rows = st.query(params![pid.to_string()])?;
            // A missing project row falls back to the global list rather than failing
            // the read: `ProjectRepo::delete` leaves tasks behind, and a board that
            // cannot be listed is worse than one judged against the global stages.
            if let Some(row) = rows.next()? {
                if let Some(text) = row.get::<_, Option<String>>(0)? {
                    let v: serde_json::Value = serde_json::from_str(&text)?;
                    if !v.is_null() {
                        return Ok(StageList { stages: parse_stages(v)?, overridden: true });
                    }
                }
            }
        }
        Ok(StageList { stages: self.global_stages(c)?, overridden: false })
    }

    // -- keys --------------------------------------------------------------

    /// The next key and sequence number for a project: `ATL-7`, or `ATLAS-7` for a
    /// task that belongs to no project. Backfills `projects.board_key` when the row
    /// predates migration 3. Callers must already hold the gate.
    fn next_key(&self, c: &Connection, project_id: Option<Uuid>) -> Result<(String, i64)> {
        let prefix = match project_id {
            None => GLOBAL_BOARD_KEY.to_string(),
            Some(pid) => {
                let mut st = c.prepare("select board_key, name from projects where id = ?")?;
                let mut rows = st.query(params![pid.to_string()])?;
                let row = rows.next()?.ok_or_else(|| AtlasError::NotFound(format!("project {pid}")))?;
                let existing: Option<String> = row.get(0)?;
                let name: String = row.get(1)?;
                match existing {
                    Some(k) if !k.trim().is_empty() => k,
                    _ => {
                        let k = pick_board_key(c, &name, Some(pid))?;
                        c.execute("update projects set board_key = ? where id = ?", params![k, pid.to_string()])?;
                        k
                    }
                }
            }
        };
        let seq = self.take_seq(c, project_id)?;
        Ok((format!("{prefix}-{seq}"), seq))
    }

    /// Claims the next sequence number for a board from `board_counters` and moves the
    /// counter on. Deleting the highest-numbered task must not free its key for reuse:
    /// a key is a task's public identity, quoted in commit messages, memories and agent
    /// notes, so `ATL-3` must never name two different pieces of work. A missing counter
    /// row is seeded from `max(seq) + 1` over the live rows, which is both the right
    /// answer for a board created before this table and a safe floor if a row is ever
    /// lost. Callers must already hold the gate.
    fn take_seq(&self, c: &Connection, project_id: Option<Uuid>) -> Result<i64> {
        let scope = project_id.map(|p| p.to_string()).unwrap_or_else(|| "global".to_string());
        let mut st = c.prepare("select next_seq from board_counters where scope = ?")?;
        let mut rows = st.query(params![scope])?;
        let seq: i64 = match rows.next()? {
            Some(r) => r.get(0)?,
            None => {
                let floor: i64 = match project_id {
                    Some(pid) => c.query_row(
                        "select coalesce(max(seq), 0) + 1 from tasks where project_id = ?",
                        params![pid.to_string()],
                        |r| r.get(0),
                    )?,
                    None => c.query_row("select coalesce(max(seq), 0) + 1 from tasks where project_id is null", [], |r| r.get(0))?,
                };
                c.execute("insert into board_counters (scope, next_seq) values (?, ?)", params![scope, floor])?;
                floor
            }
        };
        c.execute("update board_counters set next_seq = ? where scope = ?", params![seq + 1, scope])?;
        Ok(seq)
    }

    // -- reads -------------------------------------------------------------

    fn resolve(&self, c: &Connection, id_or_key: &str) -> Result<Uuid> {
        let s = id_or_key.trim();
        if let Ok(id) = Uuid::parse_str(s) {
            let n: i64 = c.query_row("select count(*) from tasks where id = ?", params![id.to_string()], |r| r.get(0))?;
            if n == 1 {
                return Ok(id);
            }
            return Err(AtlasError::NotFound(format!("task {s}")));
        }
        let mut st = c.prepare("select id::text from tasks where upper(key) = upper(?)")?;
        let mut rows = st.query(params![s])?;
        match rows.next()? {
            Some(r) => Ok(Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?),
            None => Err(AtlasError::NotFound(format!("task {s}"))),
        }
    }

    fn load_bare(&self, c: &Connection, id: Uuid) -> Result<Task> {
        let mut st = c.prepare(&format!("select {TASK_COLS} from tasks where id = ?"))?;
        let mut rows = st.query(params![id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(row_to_task(r)?),
            None => Err(AtlasError::NotFound(format!("task {id}"))),
        }
    }

    fn load_one(&self, c: &Connection, id: Uuid) -> Result<Task> {
        let mut tasks = vec![self.load_bare(c, id)?];
        self.decorate(c, &mut tasks)?;
        Ok(tasks.remove(0))
    }

    /// Fills in `blocked_by`, `ready` and `blocked_reason`, which are computed per
    /// read against the stage list each task's project is using.
    fn decorate(&self, c: &Connection, tasks: &mut [Task]) -> Result<HashMap<Uuid, bool>> {
        if tasks.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<String> = tasks.iter().map(|t| t.id.to_string()).collect();
        let holes = std::iter::repeat_n("?", ids.len()).collect::<Vec<_>>().join(",");

        // task_id -> blocker ids
        let mut links: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        let mut st = c.prepare(&format!("select task_id::text, blocked_by::text from task_blockers where task_id in ({holes})"))?;
        let mut rows = st.query(params_from_iter(ids.iter()))?;
        while let Some(r) = rows.next()? {
            let t = Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?;
            let b = Uuid::parse_str(&r.get::<_, String>(1)?).map_err(|e| conv_err(1, Type::Text, e))?;
            links.entry(t).or_default().push(b);
        }

        // blocker id -> (key, stage, project_id)
        let blocker_ids: Vec<String> = {
            let mut v: Vec<String> = links.values().flatten().map(|b| b.to_string()).collect();
            v.sort();
            v.dedup();
            v
        };
        let mut blockers: HashMap<Uuid, (String, String, Option<Uuid>)> = HashMap::new();
        if !blocker_ids.is_empty() {
            let holes = std::iter::repeat_n("?", blocker_ids.len()).collect::<Vec<_>>().join(",");
            let mut st = c.prepare(&format!("select id::text, key, stage, project_id::text from tasks where id in ({holes})"))?;
            let mut rows = st.query(params_from_iter(blocker_ids.iter()))?;
            while let Some(r) = rows.next()? {
                let id = Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?;
                blockers.insert(id, (r.get(1)?, r.get(2)?, parse_uuid(3, r.get::<_, Option<String>>(3)?)?));
            }
        }

        // parent id -> children (key, stage, project_id)
        let mut children: HashMap<Uuid, Vec<(String, String, Option<Uuid>)>> = HashMap::new();
        let mut st = c.prepare(&format!("select parent_id::text, key, stage, project_id::text from tasks where parent_id in ({holes})"))?;
        let mut rows = st.query(params_from_iter(ids.iter()))?;
        while let Some(r) = rows.next()? {
            let p = Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?;
            children.entry(p).or_default().push((r.get(1)?, r.get(2)?, parse_uuid(3, r.get::<_, Option<String>>(3)?)?));
        }

        // Every project whose stage list any of these rows is judged against.
        let mut wanted: Vec<Option<Uuid>> = tasks.iter().map(|t| t.project_id).collect();
        wanted.extend(blockers.values().map(|(_, _, p)| *p));
        wanted.extend(children.values().flatten().map(|(_, _, p)| *p));
        wanted.sort();
        wanted.dedup();
        let mut by_project = HashMap::new();
        for p in wanted {
            by_project.insert(p, self.stages_for(c, p)?.stages);
        }
        let index = StageIndex { by_project };

        let mut done_map = HashMap::with_capacity(tasks.len());
        for t in tasks.iter_mut() {
            let done = index.is_done(t.project_id, &t.stage);
            let mut keys: Vec<String> = Vec::new();
            let mut open_blockers: Vec<String> = Vec::new();
            for b in links.get(&t.id).into_iter().flatten() {
                // A link whose target row is gone is ignored rather than treated as an
                // open blocker, so a broken link cannot wedge a task as never-ready.
                // `sweep_dangling_blockers` removes it on the next write.
                if let Some((key, stage, project)) = blockers.get(b) {
                    keys.push(key.clone());
                    if !index.is_done(*project, stage) {
                        open_blockers.push(key.clone());
                    }
                }
            }
            keys.sort();
            open_blockers.sort();
            let mut open_children: Vec<String> = children
                .get(&t.id)
                .into_iter()
                .flatten()
                .filter(|(_, stage, project)| !index.is_done(*project, stage))
                .map(|(key, _, _)| key.clone())
                .collect();
            open_children.sort();
            let own_children = children.get(&t.id).map(|v| v.len()).unwrap_or(0);

            t.blocked_by = keys;
            t.open_blockers = open_blockers.len();
            t.ready = !done && open_blockers.is_empty() && open_children.is_empty();
            t.subtasks_total = own_children as u32;
            t.subtasks_done = (own_children - open_children.len()) as u32;
            t.blocked_reason = if done {
                None
            } else if !open_blockers.is_empty() {
                Some(format!("blocked by {}", open_blockers.join(", ")))
            } else if !open_children.is_empty() {
                Some(format!("{} open subtask{}: {}", open_children.len(), if open_children.len() == 1 { "" } else { "s" }, open_children.join(", ")))
            } else {
                None
            };
            done_map.insert(t.id, done);
        }
        Ok(done_map)
    }

    /// A task with its subtasks and its whole history, oldest event first.
    pub fn get(&self, id_or_key: &str) -> Result<TaskDetail> {
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            let task = self.load_one(c, id)?;
            let mut children = {
                let mut st = c.prepare(&format!("select {TASK_COLS} from tasks where parent_id = ? order by seq"))?;
                let rows = st.query_map(params![id.to_string()], row_to_task)?;
                rows.collect::<duckdb::Result<Vec<_>>>()?
            };
            self.decorate(c, &mut children)?;
            let events = {
                let mut st = c.prepare(&format!("select {EVENT_COLS} from task_events where task_id = ? order by created_at, id"))?;
                let rows = st.query_map(params![id.to_string()], row_to_event)?;
                rows.collect::<duckdb::Result<Vec<_>>>()?
            };
            Ok(TaskDetail { task, children, events })
        })
    }

    /// Grouped by project and ordered by each task's sequence number, so the order is
    /// stable across calls. `ready` and `include_done` are applied after the rows are
    /// decorated, because both depend on the stage list rather than on the row.
    pub fn list(&self, f: &TaskFilter) -> Result<Vec<Task>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {TASK_COLS} from tasks where 1 = 1");
            let mut args: Vec<String> = Vec::new();
            if let Some(p) = f.project_id {
                sql.push_str(" and project_id = ?");
                args.push(p.to_string());
            }
            if f.global_only {
                sql.push_str(" and project_id is null");
            }
            if let Some(top_level) = f.top_level {
                sql.push_str(if top_level { " and parent_id is null" } else { " and parent_id is not null" });
            }
            if let Some(s) = &f.stage {
                sql.push_str(" and lower(stage) = lower(?)");
                args.push(s.clone());
            }
            if let Some(a) = &f.assignee {
                // Case-insensitive like the stage and text filters: a task claimed by
                // `codex` must be found by `Codex`.
                sql.push_str(" and lower(assignee) = lower(?)");
                args.push(a.clone());
            }
            if let Some(q) = &f.query {
                // `contains` rather than `like`, so `%` and `_` in a search term are
                // literal characters and not wildcards.
                sql.push_str(" and (contains(lower(key), ?) or contains(lower(title), ?) or contains(lower(description), ?))");
                let q = q.trim().to_lowercase();
                args.extend([q.clone(), q.clone(), q]);
            }
            // By project, then by the numeric sequence. Ordering by the key text would
            // interleave `ATL-10` between `ATL-1` and `ATL-2`, and ordering by
            // `created_at` alone has no tie-break inside one microsecond.
            sql.push_str(" order by project_id nulls first, seq");
            let mut st = c.prepare(&sql)?;
            let mut tasks: Vec<Task> = st.query_map(params_from_iter(args.iter()), row_to_task)?.collect::<duckdb::Result<Vec<_>>>()?;
            let done = self.decorate(c, &mut tasks)?;
            if f.ready {
                tasks.retain(|t| t.ready);
            } else if !f.include_done {
                tasks.retain(|t| !done.get(&t.id).copied().unwrap_or(false));
            }
            Ok(tasks)
        })
    }

    /// Rows whose key, title or description case-insensitively contain `pattern` (a
    /// caller-built `LIKE`-escaped substring, wrapped in `%...%`), newest-updated
    /// first, capped at 500. A SQL-level prefilter for global search: the caller
    /// re-scores and re-ranks the candidates in memory, this only bounds how many rows
    /// it has to look at, so a board with far more than 500 matches still answers in
    /// bounded time rather than scanning every task on every keystroke.
    pub fn search_candidates(&self, project_id: Option<Uuid>, pattern: &str) -> Result<Vec<Task>> {
        self.db.with_conn(|c| {
            let mut sql = format!(
                "select {TASK_COLS} from tasks where (lower(key) like ? escape '\\' or lower(title) like ? escape '\\' or lower(description) like ? escape '\\')"
            );
            let mut args: Vec<String> = vec![pattern.to_string(), pattern.to_string(), pattern.to_string()];
            if let Some(p) = project_id {
                sql.push_str(" and project_id = ?");
                args.push(p.to_string());
            }
            sql.push_str(" order by updated_at desc limit 500");
            let mut st = c.prepare(&sql)?;
            let tasks: Vec<Task> = st.query_map(params_from_iter(args.iter()), row_to_task)?.collect::<duckdb::Result<Vec<_>>>()?;
            Ok(tasks)
        })
    }

    /// One row per stage of the effective list, in board order, counting the tasks
    /// sitting in it. Stages with no tasks are present with a zero. Scoped the same
    /// way [`list`](Self::list) reads `project_id`/`global_only`: neither set counts
    /// every project's tasks, `project_id` alone counts just that project's, and
    /// `global_only` counts just the project-less ones (the caller refuses passing
    /// both at once).
    pub fn counts_by_stage(&self, project_id: Option<Uuid>, global_only: bool, top_level: Option<bool>) -> Result<Vec<(String, i64)>> {
        self.db.with_conn(|c| {
            let stages = self.stages_for(c, project_id)?.stages;
            let mut out = Vec::with_capacity(stages.len());
            for s in stages {
                let mut sql = String::from("select count(*) from tasks where lower(stage) = lower(?)");
                let mut args: Vec<String> = vec![s.name.clone()];
                if global_only {
                    sql.push_str(" and project_id is null");
                } else if let Some(p) = project_id {
                    sql.push_str(" and project_id = ?");
                    args.push(p.to_string());
                }
                if let Some(top_level) = top_level {
                    sql.push_str(if top_level { " and parent_id is null" } else { " and parent_id is not null" });
                }
                let n: i64 = c.query_row(&sql, params_from_iter(args.iter()), |r| r.get(0))?;
                out.push((s.name, n));
            }
            Ok(out)
        })
    }

    /// Task key, owning project id, and one event whose kind or body case-insensitively
    /// contains `pattern` (a caller-built `LIKE`-escaped substring, wrapped in
    /// `%...%`), across every task in scope (or every task, when `project_id` is
    /// `None`), newest first, capped at 500. A SQL-level prefilter for global search;
    /// not used by any board route.
    pub fn events_for_search(&self, project_id: Option<Uuid>, pattern: &str) -> Result<Vec<(String, Option<Uuid>, TaskEvent)>> {
        self.db.with_conn(|c| {
            let mut sql = format!(
                "select {EVENT_SEARCH_COLS} from task_events e join tasks t on t.id = e.task_id where (lower(e.kind) like ? escape '\\' or lower(e.body) like ? escape '\\')"
            );
            let mut args: Vec<String> = vec![pattern.to_string(), pattern.to_string()];
            if let Some(p) = project_id {
                sql.push_str(" and t.project_id = ?");
                args.push(p.to_string());
            }
            sql.push_str(" order by e.created_at desc limit 500");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(args.iter()), row_to_event_for_search)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    /// The task carrying `source_ref`, scoped to `project_id` the same way every
    /// other board read is (`None` means the project-less board, not "any project").
    /// Matched field by field through DuckDB's JSON functions rather than by exact
    /// text, so a difference in how the column happened to be reformatted on write
    /// never hides a match. Used by `frameworks::import::import_tasks` so a re-import
    /// finds the task it already created instead of filing a duplicate.
    pub fn find_by_source_ref(&self, project_id: Option<Uuid>, source_ref: &SourceRef) -> Result<Option<Task>> {
        self.db.with_conn(|c| {
            let mut sql = format!(
                "select {TASK_COLS} from tasks where source_ref is not null \
                 and json_extract_string(source_ref, '$.framework') = ? \
                 and json_extract_string(source_ref, '$.path') = ? \
                 and json_extract_string(source_ref, '$.anchor') = ?"
            );
            let mut args: Vec<String> = vec![source_ref.framework.as_str().to_string(), source_ref.path.clone(), source_ref.anchor.clone()];
            if let Some(p) = project_id {
                sql.push_str(" and project_id = ?");
                args.push(p.to_string());
            } else {
                sql.push_str(" and project_id is null");
            }
            let mut st = c.prepare(&sql)?;
            let mut rows = st.query(params_from_iter(args.iter()))?;
            match rows.next()? {
                Some(r) => Ok(Some(row_to_task(r)?)),
                None => Ok(None),
            }
        })
    }

    // -- writes ------------------------------------------------------------

    fn event(&self, c: &Connection, task_id: Uuid, actor: &str, kind: &str, body: &str, detail: Option<serde_json::Value>) -> Result<TaskEvent> {
        let id = Uuid::new_v4();
        c.execute(
            "insert into task_events (id, task_id, actor, kind, body, detail) values (?, ?, ?, ?, ?, ?::json)",
            params![id.to_string(), task_id.to_string(), actor, kind, body, detail.map(|d| d.to_string())],
        )?;
        let mut st = c.prepare(&format!("select {EVENT_COLS} from task_events where id = ?"))?;
        let mut rows = st.query(params![id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(row_to_event(r)?),
            None => Err(AtlasError::Other(format!("event {id} vanished"))),
        }
    }

    /// Compared at millisecond resolution. `updated_at` is stored and read back in
    /// microseconds, but a JavaScript `Date` truncates to milliseconds, so a value the
    /// desktop app read and sent straight back would never match a microsecond
    /// comparison and every optimistic update from the GUI would be a phantom conflict.
    /// A concurrent writer that lands inside the same millisecond is not detected; the
    /// gate serializes board writes, so the loser sees the winner's row either way.
    fn check_expected(task: &Task, expected: Option<DateTime<Utc>>) -> Result<()> {
        match expected {
            Some(e) if e.timestamp_millis() != task.updated_at.timestamp_millis() => Err(AtlasError::Conflict(format!(
                "{} changed since you read it (expected {}, found {})",
                task.key,
                e.to_rfc3339(),
                task.updated_at.to_rfc3339()
            ))),
            _ => Ok(()),
        }
    }

    /// Refuses a blocker set that would put `task` on a cycle: a blocker that is the
    /// task itself, or one the task already blocks, directly or through other tasks.
    fn check_no_blocker_cycle(&self, c: &Connection, task: Uuid, new_blockers: &[Uuid]) -> Result<()> {
        for b in new_blockers {
            if *b == task {
                return Err(AtlasError::Invalid("a task cannot block itself".into()));
            }
            // Walk what `b` waits on. Reaching `task` means `task` would wait on
            // something that waits on `task`.
            let mut seen: Vec<Uuid> = vec![*b];
            let mut queue: Vec<Uuid> = vec![*b];
            while let Some(cur) = queue.pop() {
                let mut st = c.prepare("select blocked_by::text from task_blockers where task_id = ?")?;
                let mut rows = st.query(params![cur.to_string()])?;
                while let Some(r) = rows.next()? {
                    let next = Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, Type::Text, e))?;
                    if next == task {
                        return Err(AtlasError::Invalid("that blocker would make a cycle".into()));
                    }
                    if !seen.contains(&next) {
                        seen.push(next);
                        queue.push(next);
                    }
                }
            }
        }
        Ok(())
    }

    /// Refuses a parent that is the task itself or one of its own descendants.
    fn check_no_parent_cycle(&self, c: &Connection, task: Uuid, parent: Uuid) -> Result<()> {
        if parent == task {
            return Err(AtlasError::Invalid("a task cannot be its own parent".into()));
        }
        let mut cur = parent;
        for _ in 0..1000 {
            let mut st = c.prepare("select parent_id::text from tasks where id = ?")?;
            let mut rows = st.query(params![cur.to_string()])?;
            let Some(r) = rows.next()? else { return Ok(()) };
            let Some(next) = parse_uuid(0, r.get::<_, Option<String>>(0)?)? else { return Ok(()) };
            if next == task {
                return Err(AtlasError::Invalid("that parent would make a cycle".into()));
            }
            cur = next;
        }
        Err(AtlasError::Invalid("the parent chain is too deep".into()))
    }

    fn replace_blockers(&self, c: &Connection, task: Uuid, blockers: &[Uuid]) -> Result<()> {
        c.execute("delete from task_blockers where task_id = ?", params![task.to_string()])?;
        for b in blockers {
            c.execute(
                "insert into task_blockers (task_id, blocked_by) values (?, ?)",
                params![task.to_string(), b.to_string()],
            )?;
        }
        self.sweep_dangling_blockers(c)
    }

    /// Drops blocker links whose task or target row no longer exists. `delete` already
    /// clears the links it can see, so this is a self-healing sweep for a row removed
    /// by some other path, such as a project cascade.
    fn sweep_dangling_blockers(&self, c: &Connection) -> Result<()> {
        c.execute(
            "delete from task_blockers where task_id not in (select id from tasks) or blocked_by not in (select id from tasks)",
            [],
        )?;
        Ok(())
    }

    /// Creates a task in the first stage of its board unless a stage is named.
    pub fn create(&self, new: &NewTask, actor: &str) -> Result<Task> {
        let _gate = self.gate();
        let title = new.title.trim().to_string();
        if title.is_empty() {
            return Err(AtlasError::Invalid("a task needs a title".into()));
        }
        self.db.with_conn(|c| {
            let stages = self.stages_for(c, new.project_id)?.stages;
            let stage = match &new.stage {
                Some(s) => find_stage(&stages, s).ok_or_else(|| unknown_stage(s, &stages))?.clone(),
                None => stages[0].clone(),
            };
            let parent = new.parent.as_deref().map(|p| self.resolve(c, p)).transpose()?;
            let blockers = new
                .blocked_by
                .iter()
                .flatten()
                .map(|b| self.resolve(c, b))
                .collect::<Result<Vec<_>>>()?;

            let (key, seq) = self.next_key(c, new.project_id)?;
            let id = Uuid::new_v4();
            self.check_no_blocker_cycle(c, id, &blockers)?;
            let labels = serde_json::to_string(new.labels.as_deref().unwrap_or(&[]))?;
            let source_ref = new.source_ref.as_ref().map(serde_json::to_string).transpose()?;
            let closed = if stage.done { "now()" } else { "null" };
            c.execute(
                &format!(
                    "insert into tasks (id, key, project_id, seq, title, description, stage, kind, priority, assignee, labels, parent_id, created_by, source_ref, created_at, updated_at, closed_at) \
                     values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?::json, ?, ?, ?::json, now(), now(), {closed})"
                ),
                params![
                    id.to_string(),
                    key,
                    new.project_id.map(|p| p.to_string()),
                    seq,
                    title,
                    new.description.clone().unwrap_or_default(),
                    stage.name,
                    new.kind.unwrap_or(TaskKind::Task).as_str(),
                    new.priority.unwrap_or(TaskPriority::Medium).as_str(),
                    new.assignee,
                    labels,
                    parent.map(|p| p.to_string()),
                    actor,
                    source_ref,
                ],
            )?;
            self.replace_blockers(c, id, &blockers)?;
            self.event(c, id, actor, "created", &format!("created {key}"), Some(json!({"stage": stage.name, "title": title})))?;
            self.load_one(c, id)
        })
    }

    /// Applies a patch. `assignee` and `parent` clear on an explicit null.
    pub fn update(&self, id_or_key: &str, upd: &TaskUpdate, actor: &str) -> Result<Task> {
        let _gate = self.gate();
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            let task = self.load_bare(c, id)?;
            Self::check_expected(&task, upd.expected_updated_at)?;

            let mut sets: Vec<String> = Vec::new();
            let mut args: Vec<duckdb::types::Value> = Vec::new();
            let mut detail = serde_json::Map::new();
            let text = duckdb::types::Value::Text;

            if let Some(t) = &upd.title {
                let t = t.trim();
                if t.is_empty() {
                    return Err(AtlasError::Invalid("a task needs a title".into()));
                }
                sets.push("title = ?".into());
                args.push(text(t.to_string()));
                detail.insert("title".into(), json!(t));
            }
            if let Some(d) = &upd.description {
                sets.push("description = ?".into());
                args.push(text(d.clone()));
                detail.insert("description".into(), json!(d));
            }
            if let Some(k) = upd.kind {
                sets.push("kind = ?".into());
                args.push(text(k.as_str().to_string()));
                detail.insert("kind".into(), json!(k.as_str()));
            }
            if let Some(p) = upd.priority {
                sets.push("priority = ?".into());
                args.push(text(p.as_str().to_string()));
                detail.insert("priority".into(), json!(p.as_str()));
            }
            if let Some(a) = &upd.assignee {
                sets.push("assignee = ?".into());
                args.push(match a {
                    Some(v) => text(v.clone()),
                    None => duckdb::types::Value::Null,
                });
                detail.insert("assignee".into(), json!(a));
            }
            if let Some(l) = &upd.labels {
                sets.push("labels = ?::json".into());
                args.push(text(serde_json::to_string(l)?));
                detail.insert("labels".into(), json!(l));
            }
            if let Some(p) = &upd.parent {
                let parent = match p {
                    Some(v) => {
                        let pid = self.resolve(c, v)?;
                        self.check_no_parent_cycle(c, id, pid)?;
                        Some(pid)
                    }
                    None => None,
                };
                sets.push("parent_id = ?".into());
                args.push(match parent {
                    Some(v) => text(v.to_string()),
                    None => duckdb::types::Value::Null,
                });
                detail.insert("parent".into(), json!(parent.map(|v| v.to_string())));
            }
            if sets.is_empty() {
                return self.load_one(c, id);
            }
            sets.push("updated_at = now()".into());
            args.push(text(id.to_string()));
            c.execute(&format!("update tasks set {} where id = ?", sets.join(", ")), params_from_iter(args.iter()))?;

            // One event per call. A patch that touches the assignee is an `assigned`
            // event so the history reads as a sentence; anything else is `edited`.
            // Clearing the assignee is still an `assigned` event, but its body says
            // `unassigned`, since "assigned ATL-1" reads backwards for an unassign.
            let kind = if upd.assignee.is_some() { "assigned" } else { "edited" };
            let verb = match &upd.assignee {
                Some(None) => "unassigned",
                Some(Some(_)) => "assigned",
                None => "edited",
            };
            let fields = detail.keys().cloned().collect::<Vec<_>>().join(", ");
            self.event(c, id, actor, kind, &format!("{verb} {}", task.key), Some(json!({"fields": fields, "to": detail})))?;
            self.load_one(c, id)
        })
    }

    /// Moves a task to another stage of its board. Entering a done stage stamps
    /// `closed_at`; leaving one clears it.
    pub fn move_stage(&self, id_or_key: &str, stage: &str, expected: Option<DateTime<Utc>>, actor: &str) -> Result<Task> {
        let _gate = self.gate();
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            let task = self.load_bare(c, id)?;
            Self::check_expected(&task, expected)?;
            let stages = self.stages_for(c, task.project_id)?.stages;
            let target = find_stage(&stages, stage).ok_or_else(|| unknown_stage(stage, &stages))?.clone();
            // `find_stage` matches case-insensitively on trimmed names, so compare the
            // same way: moving from `in progress` to `In Progress` is the stage the
            // task is already in, and writing a `moved` event for it reads as a no-op.
            if target.name.trim().eq_ignore_ascii_case(task.stage.trim()) {
                return self.load_one(c, id);
            }
            self.move_gated(c, &task, &target, actor)?;
            self.load_one(c, id)
        })
    }

    /// The body of a move, for callers inside this type that already resolved the
    /// task and the target stage.
    fn move_gated(&self, c: &Connection, task: &Task, target: &Stage, actor: &str) -> Result<()> {
        let closed = if target.done { "now()" } else { "null" };
        c.execute(
            &format!("update tasks set stage = ?, closed_at = {closed}, updated_at = now() where id = ?"),
            params![target.name, task.id.to_string()],
        )?;
        self.event(
            c,
            task.id,
            actor,
            "moved",
            &format!("moved {} from {} to {}", task.key, task.stage, target.name),
            Some(json!({"from": task.stage, "to": target.name})),
        )?;
        Ok(())
    }

    /// Appends a comment to a task's history.
    pub fn comment(&self, id_or_key: &str, body: &str, actor: &str) -> Result<TaskEvent> {
        let _gate = self.gate();
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err(AtlasError::Invalid("a comment cannot be empty".into()));
        }
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            self.load_bare(c, id)?;
            self.event(c, id, actor, "commented", &body, None)
        })
    }

    /// Takes a task: assigns it to the actor and, when it is still in the first
    /// stage, moves it to the second. Fails when someone else holds it unless
    /// `force` is set.
    pub fn claim(&self, id_or_key: &str, force: bool, actor: &str) -> Result<Task> {
        let _gate = self.gate();
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            let task = self.load_bare(c, id)?;
            if let Some(held) = &task.assignee {
                // Case-insensitively, like the assignee filter in `list`: `Codex`
                // retaking a task held by `codex` is the same agent, not a conflict,
                // and neither should silently take one held by someone else.
                if !held.eq_ignore_ascii_case(actor) && !force {
                    return Err(AtlasError::Conflict(format!("{} is assigned to {held}; claim with force to take it", task.key)));
                }
            }
            c.execute("update tasks set assignee = ?, updated_at = now() where id = ?", params![actor, id.to_string()])?;
            self.event(c, id, actor, "assigned", &format!("{actor} claimed {}", task.key), Some(json!({"assignee": actor, "force": force})))?;
            let stages = self.stages_for(c, task.project_id)?.stages;
            if stages.len() > 1 && find_stage(&stages, &task.stage).map(|s| s.name == stages[0].name).unwrap_or(false) {
                let target = stages[1].clone();
                self.move_gated(c, &task, &target, actor)?;
            }
            self.load_one(c, id)
        })
    }

    /// Replaces the set of tasks this one waits on. An empty list clears them.
    pub fn set_blockers(&self, id_or_key: &str, blocked_by: Vec<String>, actor: &str) -> Result<Task> {
        let _gate = self.gate();
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            let task = self.load_bare(c, id)?;
            let mut ids = blocked_by.iter().map(|b| self.resolve(c, b)).collect::<Result<Vec<_>>>()?;
            ids.sort();
            ids.dedup();
            self.check_no_blocker_cycle(c, id, &ids)?;
            self.replace_blockers(c, id, &ids)?;
            c.execute("update tasks set updated_at = now() where id = ?", params![id.to_string()])?;
            let after = self.load_one(c, id)?;
            let kind = if after.blocked_by.is_empty() { "unblocked" } else { "blocked" };
            let body = if after.blocked_by.is_empty() {
                format!("cleared the blockers on {}", task.key)
            } else {
                format!("{} is blocked by {}", task.key, after.blocked_by.join(", "))
            };
            self.event(c, id, actor, kind, &body, Some(json!({"blocked_by": after.blocked_by})))?;
            self.load_one(c, id)
        })
    }

    /// Removes a task: drops its blocker links in both directions, detaches its
    /// subtasks, appends a `deleted` event and an audit row, then removes the row.
    /// The task's events are kept, so the history of what was deleted survives.
    pub fn delete(&self, id_or_key: &str, actor: &str) -> Result<()> {
        let _gate = self.gate();
        let (id, key, title) = self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_key)?;
            let task = self.load_bare(c, id)?;
            c.execute("delete from task_blockers where task_id = ? or blocked_by = ?", params![id.to_string(), id.to_string()])?;
            c.execute("update tasks set parent_id = null, updated_at = now() where parent_id = ?", params![id.to_string()])?;
            self.event(c, id, actor, "deleted", &format!("deleted {}", task.key), Some(json!({"title": task.title})))?;
            c.execute("delete from tasks where id = ?", params![id.to_string()])?;
            self.sweep_dangling_blockers(c)?;
            Ok((id, task.key, task.title))
        })?;
        MemoryRepo::new(&self.db).audit(actor, "task_delete", "task", Some(id), json!({"key": key, "title": title}))
    }

    // -- stage administration ---------------------------------------------

    /// The stages a task would land in after `renames` is applied, or an error
    /// naming how many tasks sit in a stage the new list drops.
    fn check_stage_removals(&self, c: &Connection, scope_sql: &str, args: &[String], target: &[Stage], renames: &HashMap<String, String>) -> Result<()> {
        let mut st = c.prepare(&format!("select stage, count(*) from tasks where {scope_sql} group by stage"))?;
        let mut rows = st.query(params_from_iter(args.iter()))?;
        while let Some(r) = rows.next()? {
            let stage: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            let after = rename_of(renames, &stage).unwrap_or_else(|| stage.clone());
            if find_stage(target, &after).is_none() {
                return Err(AtlasError::Invalid(format!(
                    "stage '{stage}' still holds {n} task{}; move or rename them first",
                    if n == 1 { "" } else { "s" }
                )));
            }
        }
        Ok(())
    }

    /// Applies `renames` to the tasks in scope in a single pass, recording a `moved`
    /// event for each task that changes column.
    ///
    /// Each task's new stage is computed from the stage it was already in, never from
    /// one this call just wrote. `{Testing: Done, Done: Archive}` therefore leaves a
    /// task that was in `Testing` in `Done`, and a swap `{A: B, B: A}` exchanges the two
    /// columns instead of collapsing them. Applying the map entry by entry would make
    /// the result depend on `HashMap` iteration order, which is not defined.
    fn apply_renames(&self, c: &Connection, scope_sql: &str, args: &[String], renames: &HashMap<String, String>, actor: &str) -> Result<()> {
        if renames.is_empty() {
            return Ok(());
        }
        // Read every task in scope first, so no write can feed the next lookup.
        let mut rows: Vec<(String, String, String)> = Vec::new();
        {
            let mut st = c.prepare(&format!("select id::text, key, stage from tasks where {scope_sql} order by project_id nulls first, seq"))?;
            let mut r = st.query(params_from_iter(args.iter()))?;
            while let Some(row) = r.next()? {
                rows.push((row.get(0)?, row.get(1)?, row.get(2)?));
            }
        }
        for (id, key, from) in rows {
            let Some(to) = rename_of(renames, &from) else { continue };
            if to.eq_ignore_ascii_case(from.trim()) {
                continue;
            }
            c.execute("update tasks set stage = ?, updated_at = now() where id = ?", params![to, id])?;
            let uid = Uuid::parse_str(&id).map_err(|e| conv_err(0, Type::Text, e))?;
            self.event(
                c,
                uid,
                actor,
                "moved",
                &format!("moved {key} from {from} to {to}"),
                Some(json!({"from": from, "to": to, "reason": "stage renamed"})),
            )?;
        }
        Ok(())
    }

    /// Refuses a rename map that cannot be applied in one pass: two sources that differ
    /// only in case would be picked between by `HashMap` order, and a target that is not
    /// on the new board would move tasks into a column nothing can show.
    fn check_renames(renames: &HashMap<String, String>, target: &[Stage]) -> Result<()> {
        let mut seen: Vec<String> = Vec::with_capacity(renames.len());
        for (from, to) in renames {
            let folded = from.trim().to_lowercase();
            if seen.contains(&folded) {
                return Err(AtlasError::Invalid(format!("stage '{}' is renamed twice", from.trim())));
            }
            seen.push(folded);
            if find_stage(target, to).is_none() {
                return Err(unknown_stage(to, target));
            }
        }
        Ok(())
    }

    /// Rewrites the global stage list. `renames` maps an old stage name to a new
    /// one and carries the tasks along; dropping a stage that still holds tasks is
    /// refused with the count.
    pub fn set_global_stages(&self, stages: Vec<Stage>, renames: &HashMap<String, String>, actor: &str) -> Result<Vec<Stage>> {
        let _gate = self.gate();
        validate_stages(&stages)?;
        let stages = self.db.with_conn(|c| {
            // Tasks judged against the global list: global ones, plus every project
            // without its own override.
            let scope = "(project_id is null or project_id in (select id from projects where board_stages is null))";
            Self::check_renames(renames, &stages)?;
            self.check_stage_removals(c, scope, &[], &stages, renames)?;
            self.apply_renames(c, scope, &[], renames, actor)?;
            Ok(stages)
        })?;
        // Not `set_many`: that refuses `board.stages` outright, so no client can write
        // the list without the checks and the gate this method has just taken.
        SettingsRepo::new(&self.db).set_board_stages(&serde_json::to_value(&stages)?, actor)?;
        Ok(stages)
    }

    /// Sets or clears a project's stage override. `None` restores the global list.
    pub fn set_project_stages(&self, project_id: Uuid, stages: Option<Vec<Stage>>, renames: &HashMap<String, String>, actor: &str) -> Result<StageList> {
        let _gate = self.gate();
        if let Some(s) = &stages {
            validate_stages(s)?;
        }
        let out = self.db.with_conn(|c| {
            let n: i64 = c.query_row("select count(*) from projects where id = ?", params![project_id.to_string()], |r| r.get(0))?;
            if n == 0 {
                return Err(AtlasError::NotFound(format!("project {project_id}")));
            }
            let target = match &stages {
                Some(s) => s.clone(),
                None => self.global_stages(c)?,
            };
            let scope = "project_id = ?";
            let args = vec![project_id.to_string()];
            Self::check_renames(renames, &target)?;
            self.check_stage_removals(c, scope, &args, &target, renames)?;
            self.apply_renames(c, scope, &args, renames, actor)?;
            match &stages {
                Some(s) => c.execute(
                    "update projects set board_stages = ?::json where id = ?",
                    params![serde_json::to_string(s)?, project_id.to_string()],
                )?,
                None => c.execute("update projects set board_stages = null where id = ?", params![project_id.to_string()])?,
            };
            self.stages_for(c, Some(project_id))
        })?;
        MemoryRepo::new(&self.db).audit(actor, "set_board_stages", "project", Some(project_id), json!({"stages": stages, "renames": renames}))?;
        Ok(out)
    }
}

/// The new name for `stage` in a rename map, matched case-insensitively on
/// trimmed names so a map built from a GUI form still lines up with stored rows.
fn rename_of(renames: &HashMap<String, String>, stage: &str) -> Option<String> {
    renames
        .iter()
        .find(|(from, _)| from.trim().eq_ignore_ascii_case(stage.trim()))
        .map(|(_, to)| to.trim().to_string())
}

#[cfg(test)]
mod tests;
