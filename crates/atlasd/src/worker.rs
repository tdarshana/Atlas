//! The daemon's single background worker. It is the only consumer of the `jobs`
//! table, so a job is claimed once and the DuckDB writes stay behind the same
//! write gate every other writer uses.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use atlas_core::backend::LocalBackend;
use atlas_core::{extract, AtlasError, Result};
use serde_json::Value;
use uuid::Uuid;

/// How long the worker sleeps when nothing wakes it. `notify_one` covers the
/// normal path; the tick is what picks up jobs left queued by a previous run, or
/// by an enqueue whose notification was lost to a restart.
const IDLE_TICK: Duration = Duration::from_secs(30);

/// How long a `done` or `failed` job row stays for `GET /jobs/{id}` before the
/// idle tick deletes it. Without the sweep the table grows by one row per Claude
/// Code turn for ever (PERF-2).
const FINISHED_JOB_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

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
    let jobs = backend.jobs.clone();
    match backend.blocking(move || jobs.requeue_stale()).await {
        Ok(0) => {}
        Ok(n) => tracing::info!("requeued {n} stale running job(s) from a previous run"),
        Err(e) => tracing::warn!("could not requeue stale jobs at startup: {e}"),
    }
}

/// A run left `queued` or `running` with no `workflow_run` job left to finish it (its
/// job already reached a terminal status, or the process that would have made one
/// never got the chance) is orphaned: `requeue_stale` above only restarts a job that is
/// itself still `running`, so this is the sweep for the run underneath it. Run once at
/// startup, after `requeue_stale`, so a job it just put back on the queue is not
/// mistaken for an orphan.
async fn sweep_orphaned_runs(backend: &LocalBackend) {
    let workflows = backend.workflows.clone();
    let jobs = backend.jobs.clone();
    let result = backend.blocking(move || -> Result<usize> {
        let runs = workflows.active_runs()?;
        let mut recovered = 0usize;
        for run in runs {
            match jobs.workflow_run_job_active(run.id) {
                Ok(true) => {}
                Ok(false) => match workflows.fail_stuck_run(run.id, "atlasd", "daemon restarted during the run") {
                    Ok(_) => recovered += 1,
                    Err(e) => tracing::warn!(run = %run.id, "could not mark an orphaned run failed: {e}"),
                },
                Err(e) => tracing::warn!(run = %run.id, "could not check the run's job at startup: {e}"),
            }
        }
        Ok(recovered)
    }).await;
    match result {
        Ok(recovered) if recovered > 0 => tracing::info!("marked {recovered} orphaned workflow run(s) failed at startup"),
        Ok(_) => {}
        Err(e) => tracing::warn!("could not read workflow runs at startup: {e}"),
    }
}

/// Deletes finished job rows older than `FINISHED_JOB_RETENTION`. Run at startup
/// and on every idle tick; with the `jobs(status)` index the delete touches only
/// terminal rows, so it is cheap to repeat.
async fn prune_finished(backend: &LocalBackend) {
    let jobs = backend.jobs.clone();
    match backend.blocking(move || jobs.prune_finished(FINISHED_JOB_RETENTION)).await {
        Ok(0) => {}
        Ok(n) => tracing::info!("pruned {n} finished job(s) older than {} days", FINISHED_JOB_RETENTION.as_secs() / 86_400),
        Err(e) => tracing::warn!("could not prune finished jobs: {e}"),
    }
}

pub async fn run(backend: Arc<LocalBackend>) {
    requeue_stale(&backend).await;
    sweep_orphaned_runs(&backend).await;
    prune_finished(&backend).await;
    loop {
        drain(&backend).await;
        tokio::select! {
            _ = backend.queue.notify.notified() => {}
            _ = tokio::time::sleep(IDLE_TICK) => prune_finished(&backend).await,
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
        let jobs = backend.jobs.clone();
        let job = match backend.blocking(move || jobs.next_queued()).await {
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
                // Under test only, a payload asking to panic skips the real runner: it
                // proves the backstop below (not `run_workflow`'s own Err paths, which
                // are exercised elsewhere) is what recovers a run when its job never
                // gets the chance to write a terminal status itself.
                #[cfg(test)]
                if job.payload["panic_for_tests"].as_bool() == Some(true) {
                    run_supervised(|| async { panic!("workflow_run panicked on purpose for a test") }).await
                } else {
                    let (j, b) = (job.clone(), backend.clone());
                    run_supervised(move || async move { atlas_core::workflow::run::run_workflow(&j, &b).await }).await
                }
                #[cfg(not(test))]
                {
                    let (j, b) = (job.clone(), backend.clone());
                    run_supervised(move || async move { atlas_core::workflow::run::run_workflow(&j, &b).await }).await
                }
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
            Ok(result) => {
                let jobs = backend.jobs.clone();
                let id = job.id;
                backend.blocking(move || jobs.mark_done(id, result)).await
            }
            Err(e) => {
                tracing::warn!(job = %job.id, kind = %job.kind, "job failed: {e}");
                if job.kind == "workflow_run" {
                    // A run's own runner (`workflow::run::run_workflow`) writes a
                    // terminal status on every *logical* failure it sees, so this is
                    // reached only when the job never got that far (a panic, above
                    // all) and is a no-op via `set_run_status`'s compare-and-swap
                    // otherwise. Without it a panicking run stays `running` forever
                    // and blocks every later run of that workflow.
                    if let Some(run_id) = job.payload["run_id"].as_str().and_then(|s| Uuid::parse_str(s).ok()) {
                        let run_actor = job.payload["actor"].as_str().unwrap_or("scheduler").to_string();
                        let workflows = backend.workflows.clone();
                        if let Err(e) = backend.blocking(move || workflows.fail_stuck_run(run_id, &run_actor, INTERNAL_ERROR)).await {
                            tracing::warn!(run = %run_id, "could not mark the stuck run failed: {e}");
                        }
                    }
                }
                let jobs = backend.jobs.clone();
                let id = job.id;
                let err_text = e.to_string();
                backend.blocking(move || jobs.mark_failed(id, &err_text)).await
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
    use atlas_core::backend::{StatusBackend, WorkflowBackend};
    use atlas_core::models::*;
    use atlas_core::paths::AtlasPaths;
    use serde_json::json;

    /// One trigger, one action, one output, wired straight through: enough for
    /// `graph::validate` and a workflow run, with no model call actually made in the
    /// tests that need it (the panic happens before `run_workflow` is ever called).
    fn single_action_graph() -> Graph {
        Graph {
            nodes: vec![
                Node { id: "t".into(), kind: NodeKind::Trigger, position: Position { x: 0.0, y: 0.0 }, data: NodeData::Trigger(Trigger::manual()) },
                Node {
                    id: "a".into(),
                    kind: NodeKind::Action,
                    position: Position { x: 200.0, y: 0.0 },
                    data: NodeData::Action { name: "do it".into(), instructions: "do it".into(), agent: "desktop".into(), practices: vec![], memories: None },
                },
                Node { id: "o".into(), kind: NodeKind::Output, position: Position { x: 400.0, y: 0.0 }, data: NodeData::Output { propose_memories: false, file_tasks: false } },
            ],
            edges: vec![Edge { id: "t-a".into(), source: "t".into(), target: "a".into() }, Edge { id: "a-o".into(), source: "a".into(), target: "o".into() }],
        }
    }

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

    /// A `workflow_run` job whose handler panics still ends the run `failed`, not stuck
    /// `running` forever: without the backstop in `drain`, `run_workflow`'s own Err
    /// paths never run, nothing else ever marks the row terminal, and every later run
    /// of the workflow is refused by `has_pending_run` for good.
    #[tokio::test]
    async fn a_panicking_workflow_run_job_ends_the_run_failed() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let new = NewWorkflow { name: "panics".into(), project_id: None, description: String::new(), trigger: Trigger::manual(), graph: single_action_graph(), enabled: true };
        let workflow = backend.create_workflow(new, "t").await.unwrap();
        let run = backend.workflows.create_run(workflow.id, TriggerKind::Manual).unwrap();
        backend.workflows.set_run_status(run.id, RunStatus::Running, None).unwrap();
        backend
            .jobs
            .enqueue("workflow_run", json!({"run_id": run.id.to_string(), "workflow_id": workflow.id.to_string(), "actor": "t", "panic_for_tests": true}))
            .unwrap();

        drain(&backend).await;

        let (finished, steps) = backend.workflows.get_run(run.id).unwrap();
        assert_eq!(finished.status, RunStatus::Failed);
        assert!(steps.iter().any(|s| s.log.iter().any(|l| l.level == LogLevel::Error && l.text == INTERNAL_ERROR)), "{steps:?}");
        assert!(!backend.workflows.has_pending_run(workflow.id).unwrap(), "the workflow must be runnable again");
    }

    /// A run left `queued` or `running` at startup, with its job already gone (here,
    /// never enqueued at all), is swept to `failed` rather than blocking the workflow
    /// forever; a run whose job is genuinely still active is left alone.
    #[tokio::test]
    async fn sweep_orphaned_runs_recovers_a_run_with_no_job_behind_it() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let orphan_wf = backend
            .create_workflow(NewWorkflow { name: "orphan".into(), project_id: None, description: String::new(), trigger: Trigger::manual(), graph: single_action_graph(), enabled: true }, "t")
            .await
            .unwrap();
        let orphan_run = backend.workflows.create_run(orphan_wf.id, TriggerKind::Manual).unwrap();
        backend.workflows.set_run_status(orphan_run.id, RunStatus::Running, None).unwrap();

        let live_wf = backend
            .create_workflow(NewWorkflow { name: "live".into(), project_id: None, description: String::new(), trigger: Trigger::manual(), graph: single_action_graph(), enabled: true }, "t")
            .await
            .unwrap();
        let live_run = backend.workflows.create_run(live_wf.id, TriggerKind::Manual).unwrap();
        backend.jobs.enqueue("workflow_run", json!({"run_id": live_run.id.to_string(), "workflow_id": live_wf.id.to_string(), "actor": "t"})).unwrap();

        sweep_orphaned_runs(&backend).await;

        let orphan_after = backend.workflows.get_run(orphan_run.id).unwrap().0;
        assert_eq!(orphan_after.status, RunStatus::Failed);
        let (_run, steps) = backend.workflows.get_run(orphan_run.id).unwrap();
        assert!(steps.iter().any(|s| s.log.iter().any(|l| l.text == "daemon restarted during the run")), "{steps:?}");

        let live_after = backend.workflows.get_run(live_run.id).unwrap().0;
        assert_eq!(live_after.status, RunStatus::Queued, "a run whose job is still queued must be left alone");
    }
}
