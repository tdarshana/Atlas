//! The daemon's single background worker. It is the only consumer of the `jobs`
//! table, so a job is claimed once and the DuckDB writes stay behind the same
//! write gate every other writer uses.

use std::sync::Arc;
use std::time::Duration;

use atlas_core::backend::LocalBackend;
use atlas_core::{extract, AtlasError};

/// How long the worker sleeps when nothing wakes it. `notify_one` covers the
/// normal path; the tick is what picks up jobs left queued by a previous run, or
/// by an enqueue whose notification was lost to a restart.
const IDLE_TICK: Duration = Duration::from_secs(30);

pub async fn run(backend: Arc<LocalBackend>) {
    loop {
        drain(&backend).await;
        tokio::select! {
            _ = backend.queue.notify.notified() => {}
            _ = tokio::time::sleep(IDLE_TICK) => {}
        }
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
            "ingest" => extract::run_ingest(&job, backend).await,
            other => Err(AtlasError::Invalid(format!("unknown job kind '{other}'"))),
        };
        // Error text comes from `AtlasError`, which never carries the api key.
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
