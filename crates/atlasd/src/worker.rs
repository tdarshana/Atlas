//! The daemon's single background worker. It is the only consumer of the `jobs`
//! table, so a job is claimed once and the DuckDB writes stay behind the same
//! write gate every other writer uses.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use atlas_core::backend::LocalBackend;
use atlas_core::{extract, AtlasError, Result};
use serde_json::Value;

/// How long the worker sleeps when nothing wakes it. `notify_one` covers the
/// normal path; the tick is what picks up jobs left queued by a previous run, or
/// by an enqueue whose notification was lost to a restart.
const IDLE_TICK: Duration = Duration::from_secs(30);

/// What a panicking job records. A panic payload can quote anything that was in
/// scope, so none of it reaches the log or the `jobs.error` column.
const INTERNAL_ERROR: &str = "internal error";

/// A job kind that only exists under `cfg(test)`, to prove a panic inside a job
/// does not take the worker (or the DuckDB connection) with it.
#[cfg(test)]
const PANIC_KIND: &str = "panic-for-tests";

/// Requeues any job left `running` from a previous process. `atlasd` is the only
/// writer of the `jobs` table, so a `running` row found at startup was never
/// actually in flight: the process that claimed it is gone.
async fn requeue_stale(backend: &LocalBackend) {
    match backend.jobs.requeue_stale() {
        Ok(0) => {}
        Ok(n) => tracing::info!("requeued {n} stale running job(s) from a previous run"),
        Err(e) => tracing::warn!("could not requeue stale jobs at startup: {e}"),
    }
}

pub async fn run(backend: Arc<LocalBackend>) {
    requeue_stale(&backend).await;
    loop {
        drain(&backend).await;
        tokio::select! {
            _ = backend.queue.notify.notified() => {}
            _ = tokio::time::sleep(IDLE_TICK) => {}
        }
    }
}

/// Runs one job's work on its own task, so a panic inside it is caught here
/// instead of aborting the worker for the lifetime of the process. Without this
/// a single bad transcript would leave every later `POST /ingest` queued for
/// ever, answered with a 202 and a job id that nothing would ever pick up.
async fn run_supervised<F, Fut>(work: F) -> Result<Value>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<Value>> + Send + 'static,
{
    match tokio::spawn(async move { work().await }).await {
        Ok(result) => result,
        Err(e) if e.is_panic() => Err(AtlasError::Other(INTERNAL_ERROR.to_string())),
        Err(e) => Err(AtlasError::Other(format!("job task ended early: {e}"))),
    }
}

/// Runs queued jobs until there are none left. A job that fails is recorded as
/// failed and the drain continues: one bad transcript must not stall the queue.
async fn drain(backend: &Arc<LocalBackend>) {
    loop {
        let job = match backend.jobs.next_queued() {
            Ok(Some(job)) => job,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("could not read the job queue: {e}");
                return;
            }
        };
        let outcome = match job.kind.as_str() {
            "ingest" => {
                let (j, b) = (job.clone(), backend.clone());
                run_supervised(move || async move { extract::run_ingest(&j, &b).await }).await
            }
            "project_summary" => {
                let (j, b) = (job.clone(), backend.clone());
                run_supervised(move || async move { extract::run_project_summary(&j, &b).await }).await
            }
            "workflow_run" => {
                let (j, b) = (job.clone(), backend.clone());
                run_supervised(move || async move { atlas_core::workflow::run::run_workflow(&j, &b).await }).await
            }
            #[cfg(test)]
            PANIC_KIND => {
                // Panics while the DuckDB connection is held, which is the case that
                // used to poison the mutex for the rest of the daemon's life.
                let db = backend.db.clone();
                run_supervised(move || async move { db.with_conn(|_| -> Result<Value> { panic!("job panicked on purpose") }) }).await
            }
            other => Err(AtlasError::Invalid(format!("unknown job kind '{other}'"))),
        };
        // `AtlasError` text is safe to record: `LlmClient` redacts the api key out of
        // any upstream body it quotes, and a panic is recorded as "internal error"
        // rather than as its payload.
        let recorded = match outcome {
            Ok(result) => backend.jobs.mark_done(job.id, result),
            Err(e) => {
                tracing::warn!(job = %job.id, kind = %job.kind, "job failed: {e}");
                backend.jobs.mark_failed(job.id, &e.to_string())
            }
        };
        if let Err(e) = recorded {
            tracing::warn!(job = %job.id, "could not record the job outcome: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::backend::Backend;
    use atlas_core::paths::AtlasPaths;
    use serde_json::json;

    #[tokio::test]
    async fn run_supervised_turns_a_panic_into_an_error_without_its_payload() {
        let err = run_supervised(|| async { panic!("secret-looking panic payload") }).await.unwrap_err();
        assert_eq!(err.to_string(), INTERNAL_ERROR);
        // The helper is still usable afterwards: the panic took only its own task.
        assert_eq!(run_supervised(|| async { Ok(json!({"ok": true})) }).await.unwrap()["ok"], true);
    }

    #[tokio::test]
    async fn run_supervised_passes_an_ordinary_error_through() {
        let err = run_supervised(|| async { Err(AtlasError::Invalid("bad payload".into())) }).await.unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
    }

    /// A panicking job is recorded as failed, the drain moves on to the next job,
    /// and the DuckDB connection the panic was holding still works.
    #[tokio::test]
    async fn a_panicking_job_fails_and_the_drain_continues() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let boom = backend.jobs.enqueue(PANIC_KIND, json!({})).unwrap();
        let after = backend.jobs.enqueue("mystery", json!({})).unwrap();

        drain(&backend).await;

        let failed = backend.jobs.get(boom).unwrap().expect("the panicking job");
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error.as_deref(), Some(INTERNAL_ERROR));

        let next = backend.jobs.get(after).unwrap().expect("the job behind it");
        assert_eq!(next.status, "failed", "the drain must not stop at the panic");
        assert!(next.error.unwrap().contains("unknown job kind"));

        // The panic fired inside `with_conn`; a non-poison-tolerant lock would make
        // every query from here on fail.
        assert!(backend.status().await.is_ok(), "the DuckDB connection was poisoned by the panic");
    }
}
