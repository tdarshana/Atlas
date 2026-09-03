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
// A daemon that is not running yet, or that restarts mid-poll, only ever surfaces here
// as a request failure: the loop below never returns and never panics on one, so a
// restart is invisible to anything but the `daemon_errors` notification itself.

use std::time::Duration;

use atlas_cli::daemon_ctl;
use atlas_core::paths::AtlasPaths;
use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

const POLL_INTERVAL: Duration = Duration::from_secs(30);

struct State {
    /// The pending-memory count as of the last tick; a notification fires only when the
    /// new count is greater than this.
    last_pending: i64,
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
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap_or_default();
        let mut state = State { last_pending: 0, since: chrono::Utc::now(), was_unreachable: false, notify_daemon_errors: false };
        loop {
            tick(&app, &client, &mut state).await;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// The daemon's base API url, read fresh from `daemon.json` on every tick rather than
/// cached: the port can change across a restart, and this is the same file the CLI and
/// the `daemon_ensure` command already trust for it.
fn base_url() -> Option<String> {
    let info = daemon_ctl::daemon_info(&AtlasPaths::discover())?;
    let port = info.get("port")?.as_u64()?;
    Some(format!("http://127.0.0.1:{port}/api/v1"))
}

async fn get_json(client: &reqwest::Client, url: &str) -> Option<serde_json::Value> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json().await.ok()
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

async fn tick<R: Runtime>(app: &AppHandle<R>, client: &reqwest::Client, state: &mut State) {
    let now = chrono::Utc::now();
    let Some(base) = base_url() else {
        mark_unreachable(app, state);
        return;
    };
    let Some(settings) = get_json(client, &format!("{base}/settings")).await else {
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
        if let Some(status) = get_json(client, &format!("{base}/status")).await {
            let pending = status.get("memories_pending").and_then(|v| v.as_i64()).unwrap_or(0);
            if pending > state.last_pending {
                show(app, &format!("{pending} memories waiting for review"));
            }
            state.last_pending = pending;
        }
    }

    if flag("ui.notify.workflow_runs") {
        let since_rfc3339 = state.since.to_rfc3339();
        let url = format!("{base}/runs?since={}", urlencoding_light(&since_rfc3339));
        if let Some(serde_json::Value::Array(runs)) = get_json(client, &url).await {
            if !runs.is_empty() {
                let workflows = get_json(client, &format!("{base}/workflows")).await;
                show(app, &workflow_runs_summary(&runs, workflows.as_ref()));
            }
        }
    }

    state.since = now;
}

/// Percent-encodes just the characters `to_rfc3339` can produce that a query string
/// would otherwise misread (`+` decodes as a space; `:` is safe unescaped but encoding
/// it changes nothing a server-side parser cares about, so only `+` is handled here).
fn urlencoding_light(s: &str) -> String {
    s.replace('+', "%2B")
}

/// The name of the workflow `run["workflow_id"]` belongs to, or `"a workflow"` when the
/// lookup list is missing or does not have it (a run for a workflow deleted since).
fn workflow_name(run: &serde_json::Value, workflows: Option<&serde_json::Value>) -> String {
    let id = run.get("workflow_id").and_then(|v| v.as_str()).unwrap_or_default();
    workflows
        .and_then(|w| w.as_array())
        .and_then(|list| list.iter().find(|w| w.get("id").and_then(|v| v.as_str()) == Some(id)))
        .and_then(|w| w.get("name")).and_then(|v| v.as_str())
        .unwrap_or("a workflow")
        .to_string()
}

/// One line summarising every run this tick found: names the workflow when there is
/// exactly one failure to report, otherwise a plain count, mirroring the daemon's own
/// "N memories waiting for review" (a single figure, not a run-by-run list) shape.
fn workflow_runs_summary(runs: &[serde_json::Value], workflows: Option<&serde_json::Value>) -> String {
    let failed: Vec<&serde_json::Value> = runs.iter().filter(|r| r.get("status").and_then(|v| v.as_str()) == Some("failed")).collect();
    match failed.as_slice() {
        [] if runs.len() == 1 => format!("Workflow {} finished", workflow_name(&runs[0], workflows)),
        [] => format!("{} workflow runs finished", runs.len()),
        [one] => format!("Workflow {} failed", workflow_name(one, workflows)),
        many => format!("Workflow {} and {} more failed", workflow_name(many[0], workflows), many.len() - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: &str, workflow_id: &str) -> serde_json::Value {
        serde_json::json!({"id": "r1", "workflow_id": workflow_id, "status": status})
    }

    fn workflows() -> serde_json::Value {
        serde_json::json!([{"id": "w1", "name": "nightly-summary"}, {"id": "w2", "name": "release"}])
    }

    #[test]
    fn one_finished_run_names_the_workflow() {
        let runs = vec![run("success", "w1")];
        assert_eq!(workflow_runs_summary(&runs, Some(&workflows())), "Workflow nightly-summary finished");
    }

    #[test]
    fn one_failed_run_names_the_workflow() {
        let runs = vec![run("failed", "w1")];
        assert_eq!(workflow_runs_summary(&runs, Some(&workflows())), "Workflow nightly-summary failed");
    }

    #[test]
    fn several_finished_runs_are_a_count() {
        let runs = vec![run("success", "w1"), run("success", "w2")];
        assert_eq!(workflow_runs_summary(&runs, Some(&workflows())), "2 workflow runs finished");
    }

    #[test]
    fn several_failed_runs_name_the_first_and_count_the_rest() {
        let runs = vec![run("failed", "w1"), run("failed", "w2")];
        assert_eq!(workflow_runs_summary(&runs, Some(&workflows())), "Workflow nightly-summary and 1 more failed");
    }

    #[test]
    fn a_run_for_an_unknown_workflow_falls_back_to_a_plain_name() {
        let runs = vec![run("failed", "gone")];
        assert_eq!(workflow_runs_summary(&runs, Some(&workflows())), "Workflow a workflow failed");
    }

    #[test]
    fn urlencoding_light_escapes_only_the_plus() {
        assert_eq!(urlencoding_light("2026-09-04T10:00:00+00:00"), "2026-09-04T10:00:00%2B00:00");
    }
}
