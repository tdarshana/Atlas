//! The background job queue. `atlasd` is the only process that opens the
//! DuckDB file, so the queue is a table rather than a channel: a job survives a
//! restart, and the daemon's worker is the single consumer.

use crate::db::Db;
use crate::memories::parse_ts_pub;
use crate::Result;
use chrono::{DateTime, Utc};
use duckdb::{params, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
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
            Ok(())
        })
    }

    pub fn mark_failed(&self, id: Uuid, error: &str) -> Result<()> {
        self.db.with_conn(|c| {
            c.execute(
                "update jobs set status = 'failed', error = ?, updated_at = now() where id = ?",
                params![error, id.to_string()],
            )?;
            Ok(())
        })
    }

    /// Requeues every job left `running`. Called once at daemon startup: `atlasd` is
    /// the only process that opens the DuckDB file, so a `running` row found there
    /// belongs to a process that is gone, not to work still in flight. Returns how
    /// many rows were requeued.
    pub fn requeue_stale(&self) -> Result<usize> {
        self.db.with_conn(|c| Ok(c.execute("update jobs set status = 'queued', updated_at = now() where status = 'running'", [])?))
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Job>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare(&format!("select {SEL} from jobs where id = ?"))?;
            let mut rows = st.query(params![id.to_string()])?;
            Ok(match rows.next()? {
                Some(r) => Some(row_to_job(r)?),
                None => None,
            })
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
        assert_eq!(r.get(id).unwrap().unwrap().status, "running");
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
        let job = r.get(id).unwrap().expect("the job");
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
        let job = r.get(id).unwrap().expect("the job");
        assert_eq!(job.status, "failed");
        assert_eq!(job.error.as_deref(), Some("unknown job kind"));
        assert!(job.result.is_none());
    }

    #[test]
    fn get_of_an_unknown_id_is_none() {
        assert!(repo().get(Uuid::new_v4()).unwrap().is_none());
    }

    /// A job stuck `running` (the daemon that claimed it never came back) is put
    /// back on the queue at startup; a `queued` or `done` job is left alone.
    #[test]
    fn requeue_stale_puts_running_jobs_back_on_the_queue() {
        let r = repo();
        let running = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        assert_eq!(r.get(running).unwrap().unwrap().status, "running");

        let done = r.enqueue("ingest", json!({})).unwrap();
        r.next_queued().unwrap().unwrap();
        r.mark_done(done, json!({})).unwrap();

        let queued = r.enqueue("ingest", json!({})).unwrap();

        assert_eq!(r.requeue_stale().unwrap(), 1);
        assert_eq!(r.get(running).unwrap().unwrap().status, "queued");
        assert_eq!(r.get(done).unwrap().unwrap().status, "done");
        assert_eq!(r.get(queued).unwrap().unwrap().status, "queued");

        assert_eq!(r.requeue_stale().unwrap(), 0, "nothing left running to requeue");
    }
}
