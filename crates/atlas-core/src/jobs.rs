//! The background job queue. `atlasd` is the only process that opens the
//! DuckDB file, so the queue is a table rather than a channel: a job survives a
//! restart, and the daemon's worker is the single consumer.

use crate::db::Db;
use crate::memories::parse_ts_pub;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use duckdb::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// `Deserialize` as well as `Serialize`, so `RemoteBackend` can read a job back
/// off `GET /jobs/{id}` rather than re-describing the shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub payload: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Select list that casts to text so row mapping is uniform across DuckDB types,
/// matching the shape `MemoryRepo` uses.
const SEL: &str = "id::text, kind, status, payload::text, result::text, error, created_at::text, updated_at::text";

/// Wraps a column-conversion failure so a malformed value fails the query instead
/// of being silently coerced to a default.
fn conv_err(col: usize, msg: impl std::fmt::Display) -> duckdb::Error {
    duckdb::Error::FromSqlConversionFailure(
        col,
        duckdb::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())),
    )
}

fn parse_json(col: usize, s: Option<String>) -> duckdb::Result<Option<Value>> {
    match s {
        Some(text) => serde_json::from_str(&text).map(Some).map_err(|e| conv_err(col, e)),
        None => Ok(None),
    }
}

fn row_to_job(r: &Row) -> duckdb::Result<Job> {
    Ok(Job {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).map_err(|e| conv_err(0, e))?,
        kind: r.get(1)?,
        status: r.get(2)?,
        // A job with no payload reads back as JSON null rather than a missing field,
        // so `run_ingest` sees one shape whatever the row holds.
        payload: parse_json(3, r.get::<_, Option<String>>(3)?)?.unwrap_or(Value::Null),
        result: parse_json(4, r.get::<_, Option<String>>(4)?)?,
        error: r.get(5)?,
        created_at: parse_ts_pub(r.get::<_, String>(6)?)?,
        updated_at: parse_ts_pub(r.get::<_, String>(7)?)?,
    })
}

/// Drops the heavy part of a finished job's payload. Nothing reads a job's payload
/// once it has left `running`, and an `ingest` job's `text` is up to a million
/// characters of transcript, so `text` is replaced by its character count `chars`
/// (the shape `GET /jobs/{id}` already serves) and every other key stays. A payload
/// that is not an object, or carries no `text`, is left as it is, so other job
/// kinds are unaffected. Runs on the connection the status update just used.
fn trim_payload(c: &Connection, id: &str) -> Result<()> {
    let mut st = c.prepare("select payload::text from jobs where id = ?")?;
    let mut rows = st.query(params![id])?;
    let raw = match rows.next()? {
        Some(r) => r.get::<_, Option<String>>(0)?,
        None => None,
    };
    let Some(raw) = raw else { return Ok(()) };
    let mut payload: Value = serde_json::from_str(&raw)?;
    let Some(obj) = payload.as_object_mut() else { return Ok(()) };
    let Some(text) = obj.remove("text") else { return Ok(()) };
    if let Some(chars) = text.as_str().map(|t| t.chars().count()) {
        obj.insert("chars".into(), Value::from(chars));
    }
    c.execute("update jobs set payload = ?::json where id = ?", params![payload.to_string(), id])?;
    Ok(())
}

pub struct JobRepo {
    db: Arc<Db>,
}

impl JobRepo {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub fn enqueue(&self, kind: &str, payload: Value) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let json = payload.to_string();
        self.db.with_conn(|c| {
            c.execute(
                "insert into jobs (id, kind, status, payload) values (?, ?, 'queued', ?::json)",
                params![id.to_string(), kind, json],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    /// Claims the oldest queued job. The select and the status change are one
    /// `update ... returning`, so two drains cannot hand the same job out twice.
    pub fn next_queued(&self) -> Result<Option<Job>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!(
                "update jobs set status = 'running', updated_at = now() \
                 where id = (select id from jobs where status = 'queued' order by created_at, id limit 1) \
                 returning {SEL}"
            ))?;
            let mut rows = st.query([])?;
            Ok(match rows.next()? {
                Some(r) => Some(row_to_job(r)?),
                None => None,
            })
        })
    }

    pub fn mark_done(&self, id: Uuid, result: Value) -> Result<()> {
        let json = result.to_string();
        self.db.with_conn(|c| {
            c.execute(
                "update jobs set status = 'done', result = ?::json, error = null, updated_at = now() where id = ?",
                params![json, id.to_string()],
            )?;
            trim_payload(c, &id.to_string())
        })
    }

    pub fn mark_failed(&self, id: Uuid, error: &str) -> Result<()> {
        self.db.with_conn(|c| {
            c.execute(
                "update jobs set status = 'failed', error = ?, updated_at = now() where id = ?",
                params![error, id.to_string()],
            )?;
            trim_payload(c, &id.to_string())
        })
    }

    /// Deletes `done` and `failed` jobs whose last status change is older than
    /// `older_than`. A finished row only exists so `GET /jobs/{id}` can answer for a
    /// while after the caller was handed the id; without this sweep the table keeps
    /// one row per Claude Code turn for ever. Returns how many rows went.
    pub fn prune_finished(&self, older_than: Duration) -> Result<usize> {
        let secs = i64::try_from(older_than.as_secs()).unwrap_or(i64::MAX);
        self.db.with_conn(|c| {
            Ok(c.execute(
                "delete from jobs where status in ('done','failed') and updated_at < now()::timestamp - to_seconds(?)",
                params![secs],
            )?)
        })
    }

    /// Requeues every job left `running`. Called once at daemon startup: `atlasd` is
    /// the only process that opens the DuckDB file, so a `running` row found there
    /// belongs to a process that is gone, not to work still in flight. Returns how
    /// many rows were requeued.
    pub fn requeue_stale(&self) -> Result<usize> {
        self.db.with_conn(|c| Ok(c.execute("update jobs set status = 'queued', updated_at = now() where status = 'running'", [])?))
    }

    /// Whether a `workflow_run` job behind this run id is still `queued` or `running`.
    /// Read at daemon startup for every workflow run left `queued`/`running`: `false`
    /// means the job that was going to finish it is gone (already terminal, or never
    /// made it into the table), so the run itself is orphaned.
    pub fn workflow_run_job_active(&self, run_id: Uuid) -> Result<bool> {
        self.db.with_conn(|c| {
            let n: i64 = c.query_row(
                "select count(*) from jobs where kind = 'workflow_run' and status in ('queued','running') and json_extract_string(payload, '$.run_id') = ?",
                params![run_id.to_string()],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    /// One job by id, or `NotFound`: a key lookup, so it answers the way every other
    /// repository's `get` does rather than with an `Option` a caller has to remember.
    pub fn get(&self, id: Uuid) -> Result<Job> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from jobs where id = ?"))?;
            let mut rows = st.query(params![id.to_string()])?;
            match rows.next()? {
                Some(r) => Ok(row_to_job(r)?),
                None => Err(AtlasError::NotFound(format!("job {id}"))),
            }
        })
    }
}

/// Wakes the worker as soon as something is enqueued, so an ingest does not wait
/// out the worker's idle tick.
#[derive(Default)]
pub struct JobQueue {
    pub notify: tokio::sync::Notify,
}

impl JobQueue {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn repo() -> JobRepo {
        JobRepo::new(Arc::new(Db::open_in_memory().unwrap()))
    }

    /// ARCH-18: a lookup by primary key misses with `NotFound`, like every other repo.
    #[test]
    fn get_of_an_unknown_id_is_not_found() {
        let r = repo();
        let missing = Uuid::new_v4();
        let err = r.get(missing).unwrap_err();
        assert!(matches!(&err, AtlasError::NotFound(m) if m.contains(&missing.to_string())), "{err}");
    }

    #[test]
    fn next_queued_claims_a_job_once() {
        let r = repo();
        let id = r.enqueue("ingest", json!({"text": "hello"})).unwrap();
        let job = r.next_queued().unwrap().expect("a queued job");
        assert_eq!(job.id, id);
        assert_eq!(job.kind, "ingest");
        assert_eq!(job.status, "running");
        assert_eq!(job.payload["text"], "hello");
        assert!(r.next_queued().unwrap().is_none(), "a claimed job must not be handed out again");
        assert_eq!(r.get(id).unwrap().status, "running");
    }

    #[test]
    fn next_queued_takes_the_oldest_first() {
        let r = repo();
        let first = r.enqueue("ingest", json!({"n": 1})).unwrap();
        let second = r.enqueue("ingest", json!({"n": 2})).unwrap();
        assert_eq!(r.next_queued().unwrap().unwrap().id, first);
        assert_eq!(r.next_queued().unwrap().unwrap().id, second);
    }

    #[test]
    fn mark_done_round_trips_through_get() {
        let r = repo();
        let id = r.enqueue("ingest", json!({"text": "x"})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_done(id, json!({"inserted": 2, "skipped_duplicates": 0})).unwrap();
        let job = r.get(id).expect("the job");
        assert_eq!(job.status, "done");
        assert_eq!(job.result.unwrap()["inserted"], 2);
        assert_eq!(job.error, None);
    }

    #[test]
    fn mark_failed_round_trips_through_get() {
        let r = repo();
        let id = r.enqueue("mystery", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_failed(id, "unknown job kind").unwrap();
        let job = r.get(id).expect("the job");
        assert_eq!(job.status, "failed");
        assert_eq!(job.error.as_deref(), Some("unknown job kind"));
        assert!(job.result.is_none());
    }

    /// A `workflow_run` job still `queued` or `running` for a run id is active; one
    /// that is `done`/`failed`, or that names a different run, is not.
    #[test]
    fn workflow_run_job_active_reads_the_payloads_run_id() {
        let r = repo();
        let run_id = Uuid::new_v4();
        assert!(!r.workflow_run_job_active(run_id).unwrap(), "no job at all");

        let id = r.enqueue("workflow_run", json!({"run_id": run_id.to_string()})).unwrap();
        assert!(r.workflow_run_job_active(run_id).unwrap(), "queued");

        r.next_queued().unwrap();
        assert!(r.workflow_run_job_active(run_id).unwrap(), "running");

        r.mark_done(id, json!({})).unwrap();
        assert!(!r.workflow_run_job_active(run_id).unwrap(), "done");

        let other = Uuid::new_v4();
        r.enqueue("workflow_run", json!({"run_id": other.to_string()})).unwrap();
        assert!(!r.workflow_run_job_active(run_id).unwrap(), "a different run's job must not count");
    }

    /// A job stuck `running` (the daemon that claimed it never came back) is put
    /// back on the queue at startup; a `queued` or `done` job is left alone.
    #[test]
    fn requeue_stale_puts_running_jobs_back_on_the_queue() {
        let r = repo();
        let running = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        assert_eq!(r.get(running).unwrap().status, "running");

        let done = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_done(done, json!({})).unwrap();

        let queued = r.enqueue("ingest", json!({})).unwrap();

        assert_eq!(r.requeue_stale().unwrap(), 1);
        assert_eq!(r.get(running).unwrap().status, "queued");
        assert_eq!(r.get(done).unwrap().status, "done");
        assert_eq!(r.get(queued).unwrap().status, "queued");

        assert_eq!(r.requeue_stale().unwrap(), 0, "nothing left running to requeue");
    }

    /// Once an ingest job is finished nothing reads its transcript again, so the
    /// row drops `payload.text` (replaced by its character count) and keeps every
    /// other key a caller follows the job by.
    #[test]
    fn a_finished_ingest_job_drops_the_transcript_and_keeps_the_rest() {
        let r = repo();
        let payload = json!({"text": "héllo", "source_tool": "test", "project_root": "/p", "project_id": "abc"});
        let done = r.enqueue("ingest", payload.clone()).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_done(done, json!({"inserted": 1})).unwrap();
        let job = r.get(done).unwrap();
        assert!(job.payload.get("text").is_none(), "text must be gone: {}", job.payload);
        assert_eq!(job.payload["chars"], 5);
        assert_eq!(job.payload["source_tool"], "test");
        assert_eq!(job.payload["project_root"], "/p");
        assert_eq!(job.payload["project_id"], "abc");
        assert_eq!(job.result.unwrap()["inserted"], 1);

        let failed = r.enqueue("ingest", payload).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_failed(failed, "boom").unwrap();
        let job = r.get(failed).unwrap();
        assert!(job.payload.get("text").is_none(), "text must be gone: {}", job.payload);
        assert_eq!(job.payload["chars"], 5);
        assert_eq!(job.payload["source_tool"], "test");
        assert_eq!(job.error.as_deref(), Some("boom"));
    }

    /// A job kind whose payload carries no `text` is stored back unchanged, and a
    /// job with no payload at all does not fail to finish.
    #[test]
    fn finishing_a_job_without_text_leaves_its_payload_alone() {
        let r = repo();
        let run_id = Uuid::new_v4().to_string();
        let id = r.enqueue("workflow_run", json!({"run_id": run_id, "actor": "scheduler"})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_done(id, json!({})).unwrap();
        let job = r.get(id).unwrap();
        assert_eq!(job.payload, json!({"run_id": run_id, "actor": "scheduler"}));

        let bare = r.enqueue("ingest", Value::Null).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_failed(bare, "no payload").unwrap();
        assert_eq!(r.get(bare).unwrap().status, "failed");
    }

    /// Only `done` and `failed` rows past the retention window go; a queued or
    /// running row of any age, and a recently finished one, stay.
    #[test]
    fn prune_finished_removes_only_old_terminal_rows() {
        let r = repo();
        let old_done = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_done(old_done, json!({})).unwrap();
        let old_failed = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_failed(old_failed, "x").unwrap();
        let old_running = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        let old_queued = r.enqueue("ingest", json!({})).unwrap();
        // Everything so far is backdated ten days; the rows below stay recent.
        r.db.with_conn(|c| {
            c.execute("update jobs set updated_at = now()::timestamp - to_days(10), created_at = now()::timestamp - to_days(10)", [])?;
            Ok(())
        })
        .unwrap();
        // Finished without a claim: `next_queued` would hand out `old_queued` first.
        let recent_done = r.enqueue("ingest", json!({})).unwrap();
        r.mark_done(recent_done, json!({})).unwrap();

        assert_eq!(r.prune_finished(std::time::Duration::from_secs(7 * 24 * 3600)).unwrap(), 2);
        assert!(r.get(old_done).is_err(), "old done row must be gone");
        assert!(r.get(old_failed).is_err(), "old failed row must be gone");
        assert_eq!(r.get(old_running).unwrap().status, "running");
        assert_eq!(r.get(old_queued).unwrap().status, "queued");
        assert_eq!(r.get(recent_done).unwrap().status, "done");
        assert_eq!(r.prune_finished(std::time::Duration::from_secs(7 * 24 * 3600)).unwrap(), 0);
    }
}
