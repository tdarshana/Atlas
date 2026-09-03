//! Workflows: a named graph of actions, the runs it has produced, and the steps of
//! each run.
//!
//! # Locking
//!
//! `WorkflowRepo` takes the same write gate `TaskRepo` takes, in the same order (gate
//! first, then the connection), for the same reason: `create_run` reads `max(number)`
//! and then inserts, and `append_log` reads a step's log before rewriting it, so a
//! second writer slipping between the read and the write would lose a run number or a
//! log line. The gate is blocking and not reentrant: every public write takes it once
//! at the top and calls only the private `*_gated` helpers below that point, and none
//! of them calls a `MemoryService` method. Reads take the connection alone. Audit rows
//! go through `MemoryRepo::audit`, which takes no gate.

pub mod graph;
pub mod migrate_docs;
pub mod run;

use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::models::*;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use duckdb::types::Type;
use duckdb::{params, Connection, Row};
use serde_json::json;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

/// Settings key recording that the Markdown workflow documents have been turned into
/// workflows, so the one-time migration does not run again.
pub const DOCS_MIGRATED_SETTING: &str = "workflows.docs_migrated";

const WF_COLS: &str = "id::text, name, project_id::text, description, \"trigger\"::text, graph::text, enabled, \
     epoch_us(created_at), epoch_us(updated_at), epoch_us(last_run_at), last_status";

const RUN_COLS: &str = "id::text, workflow_id::text, number, \"trigger\", status, epoch_us(started_at), epoch_us(finished_at), summary::text";

const STEP_COLS: &str = "id::text, run_id::text, \"position\", action_id, name, agent, status, \
     epoch_us(started_at), epoch_us(finished_at), output, log::text";

/// Wraps a column-conversion failure so a malformed value fails the query instead of
/// being silently coerced to a default. Mirrors `board::conv_err`.
fn conv_err(col: usize, ty: Type, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(col, ty, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())))
}

/// Timestamps come back as microseconds since the epoch for the reason `board::ts`
/// gives: DuckDB's text rendering of a `timestamptz` moves with the session time zone.
fn ts(col: usize, us: i64) -> duckdb::Result<DateTime<Utc>> {
    DateTime::from_timestamp_micros(us).ok_or_else(|| conv_err(col, Type::BigInt, format!("timestamp out of range: {us}")))
}

fn uuid_col(col: usize, s: String) -> duckdb::Result<Uuid> {
    Uuid::parse_str(&s).map_err(|e| conv_err(col, Type::Text, e))
}

fn json_col<T: serde::de::DeserializeOwned>(col: usize, s: &str) -> duckdb::Result<T> {
    serde_json::from_str(s).map_err(|e| conv_err(col, Type::Text, e))
}

fn row_to_workflow(r: &Row) -> duckdb::Result<Workflow> {
    let trigger: String = r.get(4)?;
    let graph: String = r.get(5)?;
    let last_status: Option<String> = r.get(10)?;
    Ok(Workflow {
        id: uuid_col(0, r.get(0)?)?,
        name: r.get(1)?,
        project_id: r.get::<_, Option<String>>(2)?.map(|s| uuid_col(2, s)).transpose()?,
        description: r.get(3)?,
        trigger: json_col(4, &trigger)?,
        graph: json_col(5, &graph)?,
        enabled: r.get(6)?,
        created_at: ts(7, r.get(7)?)?,
        updated_at: ts(8, r.get(8)?)?,
        last_run_at: r.get::<_, Option<i64>>(9)?.map(|v| ts(9, v)).transpose()?,
        last_status: last_status.map(|s| s.parse().map_err(|e: AtlasError| conv_err(10, Type::Text, e))).transpose()?,
    })
}

fn row_to_run(r: &Row) -> duckdb::Result<WorkflowRun> {
    let summary: Option<String> = r.get(7)?;
    Ok(WorkflowRun {
        id: uuid_col(0, r.get(0)?)?,
        workflow_id: uuid_col(1, r.get(1)?)?,
        number: r.get(2)?,
        trigger: r.get::<_, String>(3)?.parse().map_err(|e: AtlasError| conv_err(3, Type::Text, e))?,
        status: r.get::<_, String>(4)?.parse().map_err(|e: AtlasError| conv_err(4, Type::Text, e))?,
        started_at: ts(5, r.get(5)?)?,
        finished_at: r.get::<_, Option<i64>>(6)?.map(|v| ts(6, v)).transpose()?,
        summary: summary.map(|s| json_col(7, &s)).transpose()?,
    })
}

fn row_to_step(r: &Row) -> duckdb::Result<WorkflowStep> {
    let log: String = r.get(10)?;
    Ok(WorkflowStep {
        id: uuid_col(0, r.get(0)?)?,
        run_id: uuid_col(1, r.get(1)?)?,
        position: r.get(2)?,
        action_id: r.get(3)?,
        name: r.get(4)?,
        agent: r.get(5)?,
        status: r.get::<_, String>(6)?.parse().map_err(|e: AtlasError| conv_err(6, Type::Text, e))?,
        started_at: ts(7, r.get(7)?)?,
        finished_at: r.get::<_, Option<i64>>(8)?.map(|v| ts(8, v)).transpose()?,
        output: r.get(9)?,
        log: json_col(10, &log)?,
    })
}

/// A cron expression as a schedule. Five fields are the form everyone writes and the
/// `cron` crate wants six, so a five-field expression is read as "at second zero".
pub fn parse_cron(expr: &str) -> Result<cron::Schedule> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err(AtlasError::Invalid("a scheduled workflow needs a cron expression".into()));
    }
    let six = if expr.split_whitespace().count() == 5 { format!("0 {expr}") } else { expr.to_string() };
    cron::Schedule::from_str(&six).map_err(|e| AtlasError::Invalid(format!("invalid cron expression '{expr}': {e}")))
}

/// Checks the fields the trigger's kind makes load-bearing. A manual trigger needs
/// nothing; the other two would silently never fire without theirs.
pub fn validate_trigger(t: &Trigger) -> Result<()> {
    match t.kind {
        TriggerKind::Manual => Ok(()),
        TriggerKind::Schedule => parse_cron(t.cron.as_deref().unwrap_or_default()).map(|_| ()),
        TriggerKind::Prompt => {
            if t.prompt.as_deref().unwrap_or_default().trim().is_empty() {
                Err(AtlasError::Invalid("a prompt-triggered workflow needs a prompt".into()))
            } else {
                Ok(())
            }
        }
    }
}

/// Names are how the CLI, MCP and the doc migration address a workflow, so they are
/// trimmed, non-empty, bounded and never UUID-shaped: `resolve` tries a UUID parse
/// first, so a workflow literally named after a UUID would be unreachable by name.
fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AtlasError::Invalid("a workflow needs a name".into()));
    }
    if name.chars().count() > 120 {
        return Err(AtlasError::Invalid("a workflow name is at most 120 characters".into()));
    }
    if Uuid::parse_str(name).is_ok() {
        return Err(AtlasError::Invalid("a workflow name cannot be a UUID".into()));
    }
    Ok(name.to_string())
}

/// The trigger carried by the graph's own trigger node. `graph::validate` already
/// guarantees exactly one trigger node exists by the time this runs.
fn node_trigger(graph: &Graph) -> Option<Trigger> {
    graph.nodes.iter().find_map(|n| match &n.data {
        NodeData::Trigger(t) => Some(t.clone()),
        _ => None,
    })
}

/// `Workflow.trigger` must always match the graph's own trigger node: the runner and
/// the scheduler read `Workflow.trigger` directly, but the editor and `graph::validate`
/// treat the node as the source of truth, so the two must never be allowed to diverge.
fn derive_trigger(trigger: &Trigger, graph: &Graph) -> Result<Trigger> {
    match node_trigger(graph) {
        Some(t) if &t == trigger => Ok(t),
        Some(_) => Err(AtlasError::Invalid("the workflow's trigger does not match its trigger node".into())),
        None => Err(AtlasError::Invalid("the workflow's graph has no trigger node".into())),
    }
}

pub struct WorkflowRepo {
    db: Arc<Db>,
    gate: Arc<Mutex<()>>,
}

impl WorkflowRepo {
    pub fn new(db: Arc<Db>, gate: Arc<Mutex<()>>) -> Self {
        Self { db, gate }
    }

    /// Poison-tolerant, like the lock accessors in `MemoryService`: a panic raised
    /// while the gate was held must not fail every later workflow write.
    fn gate(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(|e| e.into_inner())
    }

    // -- reads -------------------------------------------------------------

    fn resolve(&self, c: &Connection, id_or_name: &str) -> Result<Uuid> {
        let s = id_or_name.trim();
        if let Ok(id) = Uuid::parse_str(s) {
            let n: i64 = c.query_row("select count(*) from workflows where id = ?", params![id.to_string()], |r| r.get(0))?;
            if n == 1 {
                return Ok(id);
            }
            return Err(AtlasError::NotFound(format!("workflow {s}")));
        }
        let mut st = c.prepare("select id::text from workflows where lower(name) = lower(?)")?;
        let mut rows = st.query(params![s])?;
        match rows.next()? {
            Some(r) => Ok(uuid_col(0, r.get(0)?)?),
            None => Err(AtlasError::NotFound(format!("workflow {s}"))),
        }
    }

    fn load(&self, c: &Connection, id: Uuid) -> Result<Workflow> {
        let mut st = c.prepare(&format!("select {WF_COLS} from workflows where id = ?"))?;
        let mut rows = st.query(params![id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(row_to_workflow(r)?),
            None => Err(AtlasError::NotFound(format!("workflow {id}"))),
        }
    }

    pub fn get(&self, id_or_name: &str) -> Result<Workflow> {
        self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_name)?;
            self.load(c, id)
        })
    }

    /// `project_id` given: that project's workflows and every global one, the widening
    /// `DocRepo::list` uses. `None`: every workflow.
    pub fn list(&self, project_id: Option<Uuid>) -> Result<Vec<Workflow>> {
        self.db.with_conn(|c| {
            let mut sql = format!("select {WF_COLS} from workflows");
            let mut args: Vec<String> = vec![];
            if let Some(p) = project_id {
                sql.push_str(" where project_id = ? or project_id is null");
                args.push(p.to_string());
            }
            sql.push_str(" order by name");
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(duckdb::params_from_iter(args.iter()), row_to_workflow)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    /// The runs of a workflow, newest first.
    pub fn list_runs(&self, workflow_id: Uuid, limit: usize) -> Result<Vec<WorkflowRun>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {RUN_COLS} from workflow_runs where workflow_id = ? order by number desc limit ?"))?;
            let rows = st.query_map(params![workflow_id.to_string(), limit as i64], row_to_run)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    pub fn get_run(&self, id: Uuid) -> Result<(WorkflowRun, Vec<WorkflowStep>)> {
        self.db.with_conn(|c| {
            let run = self.load_run(c, id)?;
            let mut st = c.prepare(&format!("select {STEP_COLS} from workflow_steps where run_id = ? order by \"position\""))?;
            let rows = st.query_map(params![id.to_string()], row_to_step)?;
            Ok((run, rows.collect::<duckdb::Result<Vec<_>>>()?))
        })
    }

    /// Whether a run of this workflow is queued or already running, which is what stops
    /// the scheduler starting a second one on top of the first.
    pub fn has_pending_run(&self, workflow_id: Uuid) -> Result<bool> {
        self.db.with_conn(|c| {
            let n: i64 = c.query_row(
                "select count(*) from workflow_runs where workflow_id = ? and status in ('queued','running')",
                params![workflow_id.to_string()],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    /// Every run still `queued` or `running`, across every workflow. Read once at
    /// daemon startup, alongside `JobRepo::requeue_stale`, to find a run left behind by
    /// a process that stopped before finishing it.
    pub fn active_runs(&self) -> Result<Vec<WorkflowRun>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {RUN_COLS} from workflow_runs where status in ('queued','running')"))?;
            let rows = st.query_map([], row_to_run)?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
    }

    /// The enabled scheduled workflows whose cron fired in `(after, now]`, where
    /// `after` is the workflow's own `last_run_at` or, for one that has never run,
    /// `since` (the caller passes the time the daemon started, so a workflow does not
    /// fire for every occurrence since the epoch on the first tick).
    ///
    /// A workflow whose cron no longer parses is skipped with a warning rather than
    /// failing the whole sweep: one bad expression must not stop every other schedule.
    pub fn due_scheduled(&self, since: DateTime<Utc>, now: DateTime<Utc>) -> Result<Vec<Workflow>> {
        let mut due = vec![];
        for w in self.list(None)? {
            if !w.enabled || w.trigger.kind != TriggerKind::Schedule {
                continue;
            }
            let schedule = match parse_cron(w.trigger.cron.as_deref().unwrap_or_default()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("workflow '{}' has an unusable cron expression, skipping: {e}", w.name);
                    continue;
                }
            };
            let after = w.last_run_at.unwrap_or(since);
            if schedule.after(&after).next().is_some_and(|next| next <= now) {
                due.push(w);
            }
        }
        Ok(due)
    }

    fn load_run(&self, c: &Connection, id: Uuid) -> Result<WorkflowRun> {
        let mut st = c.prepare(&format!("select {RUN_COLS} from workflow_runs where id = ?"))?;
        let mut rows = st.query(params![id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(row_to_run(r)?),
            None => Err(AtlasError::NotFound(format!("run {id}"))),
        }
    }

    fn load_step(&self, c: &Connection, id: Uuid) -> Result<WorkflowStep> {
        let mut st = c.prepare(&format!("select {STEP_COLS} from workflow_steps where id = ?"))?;
        let mut rows = st.query(params![id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(row_to_step(r)?),
            None => Err(AtlasError::NotFound(format!("step {id}"))),
        }
    }

    // -- writes ------------------------------------------------------------

    pub fn create(&self, new: &NewWorkflow, actor: &str) -> Result<Workflow> {
        let _gate = self.gate();
        let name = check_name(&new.name)?;
        validate_trigger(&new.trigger)?;
        graph::validate(&new.graph)?;
        let derived_trigger = derive_trigger(&new.trigger, &new.graph)?;
        let trigger = serde_json::to_string(&derived_trigger)?;
        let graph = serde_json::to_string(&new.graph)?;
        let id = Uuid::new_v4();
        self.db.with_conn(|c| {
            if self.name_taken(c, &name, None)? {
                return Err(AtlasError::Invalid(format!("a workflow named '{name}' already exists")));
            }
            c.execute(
                "insert into workflows (id, name, project_id, description, \"trigger\", graph, enabled, created_at, updated_at) \
                 values (?, ?, ?, ?, ?::json, ?::json, ?, now(), now())",
                params![id.to_string(), name, new.project_id.map(|p| p.to_string()), new.description, trigger, graph, new.enabled],
            )?;
            Ok(())
        })?;
        MemoryRepo::new(&self.db).audit(actor, "create", "workflow", Some(id), json!({"name": name}))?;
        self.db.with_conn(|c| self.load(c, id))
    }

    pub fn update(&self, id_or_name: &str, patch: &WorkflowPatch, actor: &str) -> Result<Workflow> {
        let _gate = self.gate();
        let (id, detail) = self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_name)?;
            let current = self.load(c, id)?;
            let mut detail = serde_json::Map::new();

            let name = match &patch.name {
                Some(n) => {
                    let n = check_name(n)?;
                    if self.name_taken(c, &n, Some(id))? {
                        return Err(AtlasError::Invalid(format!("a workflow named '{n}' already exists")));
                    }
                    detail.insert("name".into(), json!(n));
                    n
                }
                None => current.name.clone(),
            };
            let project_id = match patch.project_id {
                Some(p) => {
                    detail.insert("project_id".into(), json!(p));
                    p
                }
                None => current.project_id,
            };
            let description = match &patch.description {
                Some(d) => {
                    detail.insert("description".into(), json!(d));
                    d.clone()
                }
                None => current.description.clone(),
            };
            let trigger = match &patch.trigger {
                Some(t) => {
                    validate_trigger(t)?;
                    detail.insert("trigger".into(), json!(t.kind.as_str()));
                    t.clone()
                }
                None => current.trigger.clone(),
            };
            let graph = match &patch.graph {
                Some(g) => {
                    let order = graph::validate(g)?;
                    detail.insert("actions".into(), json!(order.len()));
                    g.clone()
                }
                None => current.graph.clone(),
            };
            let trigger = derive_trigger(&trigger, &graph)?;
            let enabled = match patch.enabled {
                Some(e) => {
                    detail.insert("enabled".into(), json!(e));
                    e
                }
                None => current.enabled,
            };

            c.execute(
                "update workflows set name = ?, project_id = ?, description = ?, \"trigger\" = ?::json, graph = ?::json, enabled = ?, updated_at = now() where id = ?",
                params![
                    name,
                    project_id.map(|p| p.to_string()),
                    description,
                    serde_json::to_string(&trigger)?,
                    serde_json::to_string(&graph)?,
                    enabled,
                    id.to_string()
                ],
            )?;
            Ok((id, detail))
        })?;
        MemoryRepo::new(&self.db).audit(actor, "update", "workflow", Some(id), serde_json::Value::Object(detail))?;
        self.db.with_conn(|c| self.load(c, id))
    }

    /// Removes the workflow and everything it produced. Runs and steps are history of a
    /// definition that is gone, so unlike a memory they are deleted rather than kept.
    pub fn delete(&self, id_or_name: &str, actor: &str) -> Result<()> {
        let _gate = self.gate();
        let (id, name) = self.db.with_conn(|c| {
            let id = self.resolve(c, id_or_name)?;
            let w = self.load(c, id)?;
            c.execute(
                "delete from workflow_steps where run_id in (select id from workflow_runs where workflow_id = ?)",
                params![id.to_string()],
            )?;
            c.execute("delete from workflow_runs where workflow_id = ?", params![id.to_string()])?;
            c.execute("delete from workflows where id = ?", params![id.to_string()])?;
            Ok((id, w.name))
        })?;
        MemoryRepo::new(&self.db).audit(actor, "delete", "workflow", Some(id), json!({"name": name}))
    }

    /// Opens a queued run and hands back its number: `max(number) + 1` for this
    /// workflow, claimed under the gate so two triggers firing at once cannot both
    /// take run 4.
    pub fn create_run(&self, workflow_id: Uuid, trigger: TriggerKind) -> Result<WorkflowRun> {
        let _gate = self.gate();
        let id = Uuid::new_v4();
        self.db.with_conn(|c| {
            let n: i64 = c.query_row("select count(*) from workflows where id = ?", params![workflow_id.to_string()], |r| r.get(0))?;
            if n == 0 {
                return Err(AtlasError::NotFound(format!("workflow {workflow_id}")));
            }
            let number: i64 = c.query_row(
                "select coalesce(max(number), 0) + 1 from workflow_runs where workflow_id = ?",
                params![workflow_id.to_string()],
                |r| r.get(0),
            )?;
            c.execute(
                "insert into workflow_runs (id, workflow_id, number, \"trigger\", status, started_at) values (?, ?, ?, ?, 'queued', now())",
                params![id.to_string(), workflow_id.to_string(), number, trigger.as_str()],
            )?;
            c.execute("update workflows set last_run_at = now() where id = ?", params![workflow_id.to_string()])?;
            self.refresh_last_status(c, workflow_id)?;
            self.load_run(c, id)
        })
    }

    /// Moves a run along. A terminal status stamps `finished_at`. `summary` of `None`
    /// leaves whatever summary the run already has, so a caller reporting only a status
    /// change does not have to read the old one back first.
    pub fn set_run_status(&self, id: Uuid, status: RunStatus, summary: Option<serde_json::Value>) -> Result<WorkflowRun> {
        let _gate = self.gate();
        self.db.with_conn(|c| self.set_run_status_gated(c, id, status, summary.as_ref()))
    }

    /// Stops a run that has not finished. A run that already succeeded or failed is
    /// history and is left alone, as is one that is already cancelled. `actor` is
    /// whoever called this route or command, not the run's own triggering actor: the
    /// runner writes its own audit row when it later observes the cancellation
    /// (attributed to the trigger), so this one records who actually asked to stop it.
    pub fn cancel_run(&self, id: Uuid, actor: &str) -> Result<WorkflowRun> {
        let _gate = self.gate();
        let run = self.db.with_conn(|c| {
            let run = self.load_run(c, id)?;
            if run.status.is_terminal() {
                return Err(AtlasError::Conflict(format!("run {} of this workflow already {}", run.number, run.status)));
            }
            self.set_run_status_gated(c, id, RunStatus::Cancelled, None)
        })?;
        MemoryRepo::new(&self.db).audit(actor, "cancel", "workflow_run", Some(id), json!({"number": run.number}))?;
        Ok(run)
    }

    /// Recovers a run that never received a terminal write of its own: the job behind
    /// it panicked, or was found still `queued`/`running` at daemon start with no
    /// `jobs` row left to finish it. Finishes whichever step was left `running` (or, if
    /// none was, appends a synthetic one, the shape `fail_before_steps` in `run.rs` uses
    /// for a run that never got to start) with one ERR line, then marks the run
    /// `failed`. A no-op, via `set_run_status`'s own compare-and-swap, when the run
    /// already reached a terminal status some other way, so this is safe to call
    /// speculatively.
    pub fn fail_stuck_run(&self, run_id: Uuid, actor: &str, message: &str) -> Result<WorkflowRun> {
        let _gate = self.gate();
        let run = self.db.with_conn(|c| {
            let run = self.load_run(c, run_id)?;
            if run.status.is_terminal() {
                return Ok(run);
            }
            let mut st = c.prepare(&format!("select {STEP_COLS} from workflow_steps where run_id = ? order by \"position\" desc limit 1"))?;
            let mut rows = st.query(params![run_id.to_string()])?;
            let last = rows.next()?.map(row_to_step).transpose()?;
            let line = LogLine::now(LogLevel::Error, message.to_string());
            match last {
                Some(step) if step.status == StepStatus::Running => {
                    let mut log = step.log.clone();
                    log.push(line);
                    c.execute(
                        "update workflow_steps set status = 'failed', log = ?::json, finished_at = now() where id = ?",
                        params![serde_json::to_string(&log)?, step.id.to_string()],
                    )?;
                }
                other => {
                    let position = other.map(|s| s.position + 1).unwrap_or(0);
                    c.execute(
                        "insert into workflow_steps (id, run_id, \"position\", action_id, name, agent, status, started_at, finished_at, log) \
                         values (?, ?, ?, '', 'recover', '', 'failed', now(), now(), ?::json)",
                        params![Uuid::new_v4().to_string(), run_id.to_string(), position, serde_json::to_string(&vec![line])?],
                    )?;
                }
            }
            self.set_run_status_gated(c, run_id, RunStatus::Failed, None)
        })?;
        let _ = MemoryRepo::new(&self.db).audit(actor, "run", "workflow_run", Some(run_id), json!({"status": "failed", "reason": message}));
        Ok(run)
    }

    /// Opens a step. `position` is the step's index in the run's execution order, which
    /// is what `get_run` sorts by.
    pub fn append_step(&self, run_id: Uuid, position: i32, action_id: &str, name: &str, agent: &str) -> Result<WorkflowStep> {
        let _gate = self.gate();
        let id = Uuid::new_v4();
        self.db.with_conn(|c| {
            self.load_run(c, run_id)?;
            c.execute(
                "insert into workflow_steps (id, run_id, \"position\", action_id, name, agent, status, started_at, log) \
                 values (?, ?, ?, ?, ?, ?, 'running', now(), '[]'::json)",
                params![id.to_string(), run_id.to_string(), position, action_id, name, agent],
            )?;
            self.load_step(c, id)
        })
    }

    /// Closes a step with its final status, output and log. The log is written whole
    /// rather than appended to, so a runner that buffered its lines does not have to
    /// replay them one at a time.
    pub fn finish_step(&self, id: Uuid, status: StepStatus, output: Option<&str>, log: &[LogLine]) -> Result<WorkflowStep> {
        let _gate = self.gate();
        let log = serde_json::to_string(log)?;
        self.db.with_conn(|c| {
            let n = c.execute(
                "update workflow_steps set status = ?, output = ?, log = ?::json, finished_at = now() where id = ?",
                params![status.as_str(), output, log, id.to_string()],
            )?;
            if n == 0 {
                return Err(AtlasError::NotFound(format!("step {id}")));
            }
            self.load_step(c, id)
        })
    }

    /// Adds one line to a running step's log. Read-modify-write, so it takes the gate.
    pub fn append_log(&self, step_id: Uuid, line: &LogLine) -> Result<()> {
        let _gate = self.gate();
        self.db.with_conn(|c| {
            let mut step = self.load_step(c, step_id)?;
            step.log.push(line.clone());
            let log = serde_json::to_string(&step.log)?;
            c.execute("update workflow_steps set log = ?::json where id = ?", params![log, step_id.to_string()])?;
            Ok(())
        })
    }

    // -- gated helpers -----------------------------------------------------

    fn name_taken(&self, c: &Connection, name: &str, except: Option<Uuid>) -> Result<bool> {
        let n: i64 = match except {
            Some(id) => c.query_row(
                "select count(*) from workflows where lower(name) = lower(?) and id <> ?",
                params![name, id.to_string()],
                |r| r.get(0),
            )?,
            None => c.query_row("select count(*) from workflows where lower(name) = lower(?)", params![name], |r| r.get(0))?,
        };
        Ok(n > 0)
    }

    /// A terminal run (`success`, `failed` or `cancelled`) is history: once written, no
    /// later call may move it, whatever status it asks for. `run_workflow` cannot
    /// itself see a cancellation land during its very last step's model call (there is
    /// no next loop iteration to notice it in), so it always tries to close the run as
    /// `success`; the guard here is what makes that attempt a silent no-op instead of
    /// clobbering a `cancelled` written moments earlier by `cancel_run`. Enforced as a
    /// compare-and-swap in the `update`'s own `where` clause rather than trusted to the
    /// read above: the two run under the same gate today, but the guard should hold
    /// even if that ever changes.
    fn set_run_status_gated(&self, c: &Connection, id: Uuid, status: RunStatus, summary: Option<&serde_json::Value>) -> Result<WorkflowRun> {
        let run = self.load_run(c, id)?;
        if run.status.is_terminal() {
            return Ok(run);
        }
        let finished = if status.is_terminal() { "now()" } else { "null" };
        c.execute(
            &format!(
                "update workflow_runs set status = ?, summary = coalesce(?::json, summary), finished_at = {finished} \
                 where id = ? and status not in ('success', 'failed', 'cancelled')"
            ),
            params![status.as_str(), summary.map(|s| s.to_string()), id.to_string()],
        )?;
        self.refresh_last_status(c, run.workflow_id)?;
        self.load_run(c, id)
    }

    /// Points `workflows.last_status` at the newest run's status, so a late update to an
    /// older run cannot overwrite what the newest one says.
    fn refresh_last_status(&self, c: &Connection, workflow_id: Uuid) -> Result<()> {
        c.execute(
            "update workflows set last_status = (select status from workflow_runs where workflow_id = ? order by number desc limit 1) where id = ?",
            params![workflow_id.to_string(), workflow_id.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
