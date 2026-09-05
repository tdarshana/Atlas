// The desktop app's notification poller: a plain tokio task, not a Tauri command, so it
// keeps running whether or not a webview page is open to drive it. Every 30 seconds it
// reads the three `ui.notify.*` settings from the daemon and acts only on the ones that
// are on, batching everything it has to say into one notification per kind per tick:
//
// - `ui.notify.review_pending`: `GET /api/v1/status`'s `memories_pending`, notified only
//   when it grew since the last tick (so clearing the queue does not itself notify).
// - `ui.notify.workflow_runs`: `GET /api/v1/runs?since=` for runs that finished since
//   the last tick, naming the workflow when there is exactly one to report.
// - `ui.notify.daemon_errors`: once when the daemon stops answering, and again only
//   once it has answered in between, so a restart notifies at most twice rather than
//   once per failed tick. Because the flag itself lives in the daemon's own settings,
//   the poller remembers the last value it read and keeps using that while the daemon
//   is down, rather than staying silent for having nothing to ask.
//
// Every read goes through `atlas_client::RemoteBackend`, the same typed client the CLI
// uses, so a route change is a compile error here rather than a string to find. A
// daemon that is not running yet, or that restarts mid-poll, only ever surfaces as a
// failed call: the loop below never returns and never panics on one, so a restart is
// invisible to anything but the `daemon_errors` notification itself.

use std::collections::HashMap;
use std::time::Duration;

use atlas_client::daemon_ctl;
use atlas_client::remote::RemoteBackend;
use atlas_core::backend::{StatusBackend, WorkflowBackend};
use atlas_core::models::{RunStatus, WorkflowRun};
use atlas_core::paths::AtlasPaths;
use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// How many finished runs one tick asks for: the daemon's own default for
/// `GET /api/v1/runs`, which the poller used to leave unset.
const RUNS_LIMIT: usize = 50;

struct State {
    /// The pending-memory count as of the last successful `/status` read, or `None`
    /// before the first one. `None` only seeds the baseline: comparing the first read
    /// against a starting `0` would report the whole pre-existing queue as growth, the
    /// same way a `None` seed for `since` (below) avoids reporting every run that
    /// finished before this app process existed.
    last_pending: Option<i64>,
    /// The start of the window `GET /api/v1/runs?since=` is asked about; advanced to
    /// "now" at the end of every tick regardless of whether anything was found.
    since: chrono::DateTime<chrono::Utc>,
    /// Whether the last tick found the daemon unreachable, so the "reachable again"
    /// transition can be told apart from "still unreachable".
    was_unreachable: bool,
    /// `ui.notify.daemon_errors` as of the last successful settings read. Used to decide
    /// whether to notify on the very read that just failed, since that read is the one
    /// place this flag cannot itself be fetched fresh.
    notify_daemon_errors: bool,
}

/// Starts the poller. Fire-and-forget: nothing holds on to the returned task, the same
/// way `tauri_plugin_log` and the store's debounce run unsupervised for the life of the
/// app.
pub fn spawn<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut state = State { last_pending: None, since: chrono::Utc::now(), was_unreachable: false, notify_daemon_errors: false };
        loop {
            tick(&app, &mut state).await;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// A client for the daemon `daemon.json` names, with its token (SEC-5), read fresh on
/// every tick rather than cached: the port and the token both change across a restart,
/// and this is the same file the CLI and the `daemon_ensure` command already trust.
fn backend() -> Option<RemoteBackend> {
    let info = daemon_ctl::read_daemon_info(&AtlasPaths::discover())?;
    Some(RemoteBackend::with_token(info.port, Some(info.token?)))
}

fn show<R: Runtime>(app: &AppHandle<R>, body: &str) {
    let _ = app.notification().builder().title("Atlas").body(body).show();
}

/// Marks the daemon unreachable and, on the transition into that state, notifies with
/// whatever `ui.notify.daemon_errors` was last known to be.
fn mark_unreachable<R: Runtime>(app: &AppHandle<R>, state: &mut State) {
    if !state.was_unreachable {
        if state.notify_daemon_errors {
            show(app, "The daemon is unreachable");
        }
        state.was_unreachable = true;
    }
}

async fn tick<R: Runtime>(app: &AppHandle<R>, state: &mut State) {
    let now = chrono::Utc::now();
    let Some(daemon) = backend() else {
        mark_unreachable(app, state);
        return;
    };
    let Ok(settings) = daemon.get_settings().await else {
        mark_unreachable(app, state);
        return;
    };

    if state.was_unreachable {
        if state.notify_daemon_errors {
            show(app, "The daemon is reachable again");
        }
        state.was_unreachable = false;
    }

    let flag = |key: &str| settings.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    state.notify_daemon_errors = flag("ui.notify.daemon_errors");

    if flag("ui.notify.review_pending") {
        if let Ok(status) = daemon.status().await {
            let pending = status.memories_pending;
            // `None` means this is the first successful read (of the app's life, or
            // since the toggle was last turned on): seed the baseline rather than
            // notifying for whatever backlog already existed.
            if let Some(previous) = state.last_pending {
                if pending > previous {
                    show(app, &format!("{pending} memories waiting for review"));
                }
            }
            state.last_pending = Some(pending);
        }
    }

    // The cursor only advances on a tick that actually checked for runs, and only past
    // a successful fetch: while the toggle is off, or a request fails, `state.since`
    // stays put, so re-enabling (or the next successful tick) still asks about
    // everything since the last time this was actually checked rather than silently
    // skipping runs that finished in between.
    if flag("ui.notify.workflow_runs") {
        if let Ok(runs) = daemon.runs_since(state.since, RUNS_LIMIT).await {
            if !runs.is_empty() {
                let names: HashMap<Uuid, String> = daemon
                    .list_workflows(None)
                    .await
                    .map(|list| list.into_iter().map(|w| (w.id, w.name)).collect())
                    .unwrap_or_default();
                show(app, &workflow_runs_summary(&runs, &names));
            }
            state.since = now;
        }
    }
}

/// The name of the workflow `run` belongs to, or `"a workflow"` when the lookup did
/// not have it (the list could not be fetched, or the workflow was deleted since).
fn workflow_name(run: &WorkflowRun, names: &HashMap<Uuid, String>) -> String {
    names.get(&run.workflow_id).cloned().unwrap_or_else(|| "a workflow".to_string())
}

/// One line summarising every run this tick found: names the workflow when there is
/// exactly one failure to report, otherwise a plain count, mirroring the daemon's own
/// "N memories waiting for review" (a single figure, not a run-by-run list) shape.
fn workflow_runs_summary(runs: &[WorkflowRun], names: &HashMap<Uuid, String>) -> String {
    let failed: Vec<&WorkflowRun> = runs.iter().filter(|r| r.status == RunStatus::Failed).collect();
    match failed.as_slice() {
        [] if runs.len() == 1 => format!("Workflow {} finished", workflow_name(&runs[0], names)),
        [] => format!("{} workflow runs finished", runs.len()),
        [one] => format!("Workflow {} failed", workflow_name(one, names)),
        many => format!("Workflow {} and {} more failed", workflow_name(many[0], names), many.len() - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::models::TriggerKind;

    const W1: Uuid = Uuid::from_u128(1);
    const W2: Uuid = Uuid::from_u128(2);

    fn run(status: RunStatus, workflow_id: Uuid) -> WorkflowRun {
        WorkflowRun {
            id: Uuid::new_v4(),
            workflow_id,
            number: 1,
            trigger: TriggerKind::Manual,
            status,
            started_at: chrono::Utc::now(),
            finished_at: None,
            summary: None,
        }
    }

    fn workflows() -> HashMap<Uuid, String> {
        HashMap::from([(W1, "nightly-summary".to_string()), (W2, "release".to_string())])
    }

    #[test]
    fn one_finished_run_names_the_workflow() {
        let runs = vec![run(RunStatus::Success, W1)];
        assert_eq!(workflow_runs_summary(&runs, &workflows()), "Workflow nightly-summary finished");
    }

    #[test]
    fn one_failed_run_names_the_workflow() {
        let runs = vec![run(RunStatus::Failed, W1)];
        assert_eq!(workflow_runs_summary(&runs, &workflows()), "Workflow nightly-summary failed");
    }

    #[test]
    fn several_finished_runs_are_a_count() {
        let runs = vec![run(RunStatus::Success, W1), run(RunStatus::Success, W2)];
        assert_eq!(workflow_runs_summary(&runs, &workflows()), "2 workflow runs finished");
    }

    #[test]
    fn several_failed_runs_name_the_first_and_count_the_rest() {
        let runs = vec![run(RunStatus::Failed, W1), run(RunStatus::Failed, W2)];
        assert_eq!(workflow_runs_summary(&runs, &workflows()), "Workflow nightly-summary and 1 more failed");
    }

    #[test]
    fn a_run_for_an_unknown_workflow_falls_back_to_a_plain_name() {
        let runs = vec![run(RunStatus::Failed, Uuid::from_u128(9))];
        assert_eq!(workflow_runs_summary(&runs, &workflows()), "Workflow a workflow failed");
    }
}
