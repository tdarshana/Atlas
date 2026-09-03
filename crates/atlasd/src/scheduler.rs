//! Fires a workflow's `schedule` trigger into a queued run. Runs alongside the
//! worker in the daemon, ticking every 30 s.

use std::sync::Arc;
use std::time::Duration;

use atlas_core::backend::LocalBackend;
use atlas_core::models::TriggerKind;
use atlas_core::Result;
use chrono::{DateTime, Utc};

/// How often the scheduler checks for a workflow whose cron has fired.
const TICK: Duration = Duration::from_secs(30);

/// The identity a scheduled run's job payload carries, recorded as the run's actor.
const SCHEDULER_ACTOR: &str = "scheduler";

/// One sweep: every enabled scheduled workflow whose cron fired since it was last
/// checked (or since `started_at`, for one that has never run) and that has no run
/// already queued or running gets a new queued run and an enqueued job. Answers how
/// many runs it started, split out from [`run`] so a test can drive it with a fake
/// `now` instead of waiting on the real clock.
pub fn tick(backend: &LocalBackend, started_at: DateTime<Utc>, now: DateTime<Utc>) -> Result<usize> {
    let mut started = 0usize;
    for workflow in backend.workflows.due_scheduled(started_at, now)? {
        if backend.workflows.has_pending_run(workflow.id)? {
            continue;
        }
        let run = backend.workflows.create_run(workflow.id, TriggerKind::Schedule)?;
        backend.jobs.enqueue(
            "workflow_run",
            serde_json::json!({
                "workflow_id": workflow.id, "run_id": run.id,
                "trigger": TriggerKind::Schedule.as_str(), "actor": SCHEDULER_ACTOR,
            }),
        )?;
        backend.queue.notify.notify_one();
        started += 1;
    }
    Ok(started)
}

/// Ticks every 30 s for the lifetime of the daemon. `started_at` is the daemon's own
/// start time, so a workflow that has never run does not replay every occurrence
/// since the epoch on the first tick.
pub async fn run(backend: Arc<LocalBackend>, started_at: DateTime<Utc>) {
    loop {
        tokio::time::sleep(TICK).await;
        if let Err(e) = tick(&backend, started_at, Utc::now()) {
            tracing::warn!("workflow scheduler tick failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::models::*;
    use atlas_core::paths::AtlasPaths;
    use chrono::TimeZone;

    /// A minimal manual-trigger-shaped graph with one action, wired for a schedule
    /// trigger instead: a trigger node, one action, one output, in a line.
    fn scheduled_workflow(backend: &LocalBackend, name: &str, cron: &str) -> Workflow {
        let trigger = Trigger { kind: TriggerKind::Schedule, cron: Some(cron.into()), prompt: None };
        let graph = Graph {
            nodes: vec![
                Node { id: "t".into(), kind: NodeKind::Trigger, position: Position { x: 0.0, y: 0.0 }, data: NodeData::Trigger(trigger.clone()) },
                Node {
                    id: "a".into(),
                    kind: NodeKind::Action,
                    position: Position { x: 240.0, y: 0.0 },
                    data: NodeData::Action { name: "step".into(), instructions: "do it".into(), agent: "desktop".into(), practices: vec![], memories: None },
                },
                Node { id: "o".into(), kind: NodeKind::Output, position: Position { x: 480.0, y: 0.0 }, data: NodeData::Output { propose_memories: false, file_tasks: false } },
            ],
            edges: vec![
                Edge { id: "t-a".into(), source: "t".into(), target: "a".into() },
                Edge { id: "a-o".into(), source: "a".into(), target: "o".into() },
            ],
        };
        backend
            .workflows
            .create(&NewWorkflow { name: name.into(), project_id: None, description: String::new(), trigger, graph, enabled: true }, "t")
            .unwrap()
    }

    /// A cron that fired once inside `(since, now]` starts exactly one run and enqueues
    /// its job; a second tick, with the first run still queued, starts none.
    #[test]
    fn a_fired_cron_enqueues_one_run_and_a_pending_one_blocks_the_next_tick() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap();
        let workflow = scheduled_workflow(&backend, "nightly", "0 3 * * *");

        let started_at = Utc.with_ymd_and_hms(2026, 9, 3, 2, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 3, 3, 5, 0).unwrap();

        assert_eq!(tick(&backend, started_at, now).unwrap(), 1);
        assert!(backend.workflows.has_pending_run(workflow.id).unwrap());
        let jobs = backend.jobs.next_queued().unwrap().expect("the scheduler must enqueue a job");
        assert_eq!(jobs.kind, "workflow_run");
        assert_eq!(jobs.payload["actor"], SCHEDULER_ACTOR);
        assert_eq!(jobs.payload["trigger"], "schedule");

        // Still queued (the job above was claimed, not finished, so the run itself is
        // still `queued`): a second tick a moment later must not start a second run.
        assert_eq!(tick(&backend, started_at, now + chrono::Duration::seconds(30)).unwrap(), 0);
    }

    /// A workflow whose cron has not fired inside the window starts nothing. (Which
    /// workflows count as due at all — enabled, schedule-triggered, cron actually
    /// fired — is `WorkflowRepo::due_scheduled`'s own contract, exercised in
    /// `atlas-core`; this only checks that a tick with nothing due starts no run.)
    #[test]
    fn nothing_due_starts_no_run() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap();
        scheduled_workflow(&backend, "not-yet", "0 3 * * *");
        let started_at = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 30).unwrap();
        assert_eq!(tick(&backend, started_at, now).unwrap(), 0);
    }
}
