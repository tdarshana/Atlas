//! Workflows and their runs.

mod common;
use common::*;
use std::time::Duration;

/// A two-action workflow runs both actions in order against the stub model, and each
/// finished step carries its output and at least one INFO log line.
#[tokio::test]
async fn workflow_run_executes_two_actions_and_succeeds() {
    let stub = stub_llm_with_reply("step done").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let workflow = create_workflow(&c, &base, "release", &["tag the release", "publish the release"], false, false).await;
    let wid = workflow["id"].as_str().unwrap();

    let run = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap();
    assert_eq!(run.status(), 202);
    let run: serde_json::Value = run.json().await.unwrap();
    assert_eq!(run["number"], 1, "{run}");
    let run_id = run["id"].as_str().unwrap().to_string();

    let detail = wait_for_run(&c, &base, &run_id).await;
    assert_eq!(detail["run"]["status"], "success", "{detail}");
    assert_eq!(detail["run"]["summary"]["steps"], 2, "{detail}");
    let steps = detail["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{detail}");
    for step in steps {
        assert_eq!(step["status"], "success", "{step}");
        assert_eq!(step["output"], "step done", "{step}");
        let log = step["log"].as_array().unwrap();
        assert!(log.iter().any(|l| l["level"] == "INFO"), "{step}");
    }

    // The export is plain text, one `ts level [step] text` line per log line.
    let export = c.get(format!("{base}/runs/{run_id}/export")).send().await.unwrap();
    assert_eq!(export.status(), 200);
    let text = export.text().await.unwrap();
    assert!(text.contains("[step0]") && text.contains("[step1]"), "{text}");
}

/// An action whose agent carries a `model_hint` runs on that model; an agent saved
/// without one, and the unsaved `desktop` fallback, both run on the extraction model.
#[tokio::test]
async fn workflow_actions_run_on_the_agents_default_model_when_it_has_one() {
    let (stub, models) = stub_llm_recording_models("step done").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);
    for (name, hint) in [("planner", Some("planner-model")), ("reviewer", None)] {
        let models = hint.map(|h| serde_json::json!({"default": h})).unwrap_or_else(|| serde_json::json!({}));
        let r = c.post(format!("{base}/agents")).json(&serde_json::json!({
            "name": name, "role": "d", "instructions": "Do it.", "models": models,
        })).send().await.unwrap();
        assert_eq!(r.status(), 201, "{}", r.text().await.unwrap());
    }

    let mut graph = linear_workflow_graph(&["plan", "review", "wrap up"], false, false);
    graph["nodes"][1]["data"]["agent"] = "planner".into();
    graph["nodes"][2]["data"]["agent"] = "reviewer".into();
    let r = c.post(format!("{base}/workflows"))
        .json(&serde_json::json!({"name": "hinted", "trigger": {"kind": "manual"}, "graph": graph, "enabled": true}))
        .send().await.unwrap();
    assert_eq!(r.status(), 201);
    let workflow: serde_json::Value = r.json().await.unwrap();
    let wid = workflow["id"].as_str().unwrap();

    let run: serde_json::Value = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let detail = wait_for_run(&c, &base, run["id"].as_str().unwrap()).await;
    assert_eq!(detail["run"]["status"], "success", "{detail}");
    assert_eq!(*models.lock().unwrap(), vec!["planner-model", "stub", "stub"]);
}

/// `GET /api/v1/runs?since=&limit=` lists finished runs across every workflow, newest
/// first, and a `since` set to just after the run finished excludes it: the notification
/// poller's own use of the parameter.
#[tokio::test]
async fn all_runs_route_lists_runs_finished_after_since() {
    let stub = stub_llm_with_reply("step done").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();

    // `Z`-suffixed rather than the `+00:00` offset form `to_rfc3339` defaults to: a raw
    // `+` in a query string is form-decoded as a space, which would corrupt the value.
    let before = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    let workflow = create_workflow(&c, &base, "nightly-summary", &["do the thing"], false, false).await;
    let wid = workflow["id"].as_str().unwrap();
    let run: serde_json::Value = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();
    let detail = wait_for_run(&c, &base, &run_id).await;
    assert_eq!(detail["run"]["status"], "success", "{detail}");

    // No `since`: the run is in the default (epoch-anchored) window.
    let all: serde_json::Value = c.get(format!("{base}/runs")).send().await.unwrap().json().await.unwrap();
    let rows = all.as_array().unwrap();
    assert!(rows.iter().any(|r| r["id"] == run_id), "{all}");

    // `since` set before the run started: still included.
    let since_before: serde_json::Value = c.get(format!("{base}/runs?since={before}")).send().await.unwrap().json().await.unwrap();
    assert!(since_before.as_array().unwrap().iter().any(|r| r["id"] == run_id), "{since_before}");

    // `since` set after the run finished: excluded.
    let after = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let since_after: serde_json::Value = c.get(format!("{base}/runs?since={after}")).send().await.unwrap().json().await.unwrap();
    assert!(!since_after.as_array().unwrap().iter().any(|r| r["id"] == run_id), "{since_after}");

    // `limit` is honoured.
    let limited: serde_json::Value = c.get(format!("{base}/runs?limit=0")).send().await.unwrap().json().await.unwrap();
    assert_eq!(limited.as_array().unwrap().len(), 0, "{limited}");

    // A `since` that does not parse as RFC 3339 is a 400, the same `ApiQuery` rejection
    // every other malformed query parameter in this file gets.
    let bad_since = c.get(format!("{base}/runs?since=not-a-date")).send().await.unwrap();
    assert_eq!(bad_since.status(), 400);
    let bad_since_body: serde_json::Value = bad_since.json().await.unwrap();
    assert!(bad_since_body["error"].as_str().is_some(), "{bad_since_body}");
}

/// The output node's trailing JSON block turns into a pending memory (its confidence
/// is below the default auto-accept threshold of 1.0) stamped `workflow/<name>`, and a
/// filed task `created_by` the same identity.
#[tokio::test]
async fn workflow_output_proposes_a_pending_memory_and_files_a_task() {
    let reply = "Done.\n\n```json\n{\"memories\": [{\"text\": \"the deploy target is fly.io\", \"kind\": \"fact\", \"confidence\": 0.4}], \"tasks\": [{\"title\": \"tag the release\"}]}\n```";
    let stub = stub_llm_with_reply(reply).await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();

    let workflow = create_workflow(&c, &base, "wrap-up", &["summarise the release"], true, true).await;
    let wname = workflow["name"].as_str().unwrap().to_string();
    let wid = workflow["id"].as_str().unwrap();

    let run: serde_json::Value = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();
    let detail = wait_for_run(&c, &base, &run_id).await;
    assert_eq!(detail["run"]["status"], "success", "{detail}");
    assert_eq!(detail["run"]["summary"]["memories_proposed"], 1, "{detail}");
    assert_eq!(detail["run"]["summary"]["tasks_filed"], 1, "{detail}");

    let pending: serde_json::Value = c.get(format!("{base}/memories?status=pending")).send().await.unwrap().json().await.unwrap();
    let rows = pending.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{pending}");
    assert_eq!(rows[0]["source_agent"], format!("workflow/{wname}"), "{pending}");
    assert_eq!(rows[0]["source_tool"], "workflow", "{pending}");
    assert_eq!(rows[0]["text"], "the deploy target is fly.io");

    let tasks: serde_json::Value = c.get(format!("{base}/tasks")).send().await.unwrap().json().await.unwrap();
    let trows = tasks.as_array().unwrap();
    assert_eq!(trows.len(), 1, "{tasks}");
    assert_eq!(trows[0]["created_by"], format!("workflow/{wname}"), "{tasks}");
    assert_eq!(trows[0]["title"], "tag the release", "{tasks}");
}

/// A workflow run against a daemon with extraction off fails on its first action, and
/// the step's log carries the same "extraction is disabled" text `POST /ingest`
/// answers 409 with, as an ERR line.
#[tokio::test]
async fn workflow_run_fails_with_an_err_line_when_extraction_is_disabled() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let workflow = create_workflow(&c, &base, "no-model", &["do the thing"], false, false).await;
    let wid = workflow["id"].as_str().unwrap();

    let run: serde_json::Value = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();
    let detail = wait_for_run(&c, &base, &run_id).await;
    assert_eq!(detail["run"]["status"], "failed", "{detail}");
    let steps = detail["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1, "{detail}");
    let log = steps[0]["log"].as_array().unwrap();
    assert!(
        log.iter().any(|l| l["level"] == "ERR" && l["text"].as_str().unwrap().contains("extraction is disabled")),
        "{steps:?}"
    );
}

/// A second `POST .../run` while the first run of the same workflow is still queued
/// or running is a 409, not a second run.
#[tokio::test]
async fn a_second_run_of_the_same_workflow_while_one_is_pending_is_a_conflict() {
    let stub = stub_llm_with_delay("slow reply", Duration::from_secs(2)).await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    let workflow = create_workflow(&c, &base, "slow", &["take a while"], false, false).await;
    let wid = workflow["id"].as_str().unwrap();

    let first = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap();
    assert_eq!(first.status(), 202);

    let second = c.post(format!("{base}/workflows/{wid}/run")).json(&serde_json::json!({})).send().await.unwrap();
    let status = second.status();
    let body: serde_json::Value = second.json().await.unwrap();
    assert_eq!(status, 409, "{body}");
}

/// Cancelling a run that is still queued behind another workflow's slow run leaves it
/// `cancelled` with no steps at all: the worker sees the cancellation before it ever
/// starts the first action.
#[tokio::test]
async fn cancelling_a_queued_run_is_skipped_by_the_worker() {
    let stub = stub_llm_with_delay("slow reply", Duration::from_secs(2)).await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();

    // Two different workflows: the worker runs one job at a time, so the occupier's
    // slow model call is what keeps the victim's job genuinely `queued` long enough to
    // cancel it before the worker ever looks at it.
    let occupier = create_workflow(&c, &base, "occupier", &["take a while"], false, false).await;
    let occupier_run: serde_json::Value =
        c.post(format!("{base}/workflows/{}/run", occupier["id"].as_str().unwrap())).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();

    let victim = create_workflow(&c, &base, "victim", &["never runs"], false, false).await;
    let victim_run: serde_json::Value =
        c.post(format!("{base}/workflows/{}/run", victim["id"].as_str().unwrap())).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let victim_run_id = victim_run["id"].as_str().unwrap().to_string();

    let cancelled: serde_json::Value = c.post(format!("{base}/runs/{victim_run_id}/cancel")).send().await.unwrap().json().await.unwrap();
    assert_eq!(cancelled["status"], "cancelled", "{cancelled}");

    let detail: serde_json::Value = c.get(format!("{base}/runs/{victim_run_id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(detail["run"]["status"], "cancelled", "{detail}");
    assert!(detail["steps"].as_array().unwrap().is_empty(), "the worker must not have started it: {detail}");

    // The occupier still finishes normally once its slow call returns.
    let occupier_run_id = occupier_run["id"].as_str().unwrap().to_string();
    let finished = wait_for_run(&c, &base, &occupier_run_id).await;
    assert_eq!(finished["run"]["status"], "success", "{finished}");
}

/// A cancel that lands while the *last* action's model call is still in flight must
/// still end the run `cancelled`, not `success`: there is no further loop iteration
/// after the last action for the runner to notice the cancellation in, so it has to be
/// observed once more after the loop, before the run is closed out.
#[tokio::test]
async fn cancelling_during_the_last_step_still_ends_the_run_cancelled() {
    let stub = stub_llm_with_delay("slow reply", Duration::from_secs(2)).await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();

    let workflow = create_workflow(&c, &base, "late-cancel", &["step one", "step two"], false, false).await;
    let run: serde_json::Value =
        c.post(format!("{base}/workflows/{}/run", workflow["id"].as_str().unwrap())).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();

    // Poll until both steps have been appended: the second (last) step is appended
    // right before its slow `chat()` call starts, so seeing it means we are now inside
    // that call.
    let mut in_last_step = false;
    for _ in 0..100 {
        let detail: serde_json::Value = c.get(format!("{base}/runs/{run_id}")).send().await.unwrap().json().await.unwrap();
        if detail["steps"].as_array().unwrap().len() >= 2 {
            in_last_step = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(in_last_step, "the run never reached its second step within 5s");

    let cancelled: serde_json::Value = c.post(format!("{base}/runs/{run_id}/cancel")).send().await.unwrap().json().await.unwrap();
    assert_eq!(cancelled["status"], "cancelled", "{cancelled}");

    // The in-flight (discarded) second action's slow reply comes back and the runner
    // falls out of its loop somewhere in the next couple of seconds; poll a bounded
    // number of times over that window, rather than betting a single fixed margin on
    // the runner having settled by then, and fail the moment the outcome is silently
    // overwritten to `success`.
    for _ in 0..30 {
        let detail: serde_json::Value = c.get(format!("{base}/runs/{run_id}")).send().await.unwrap().json().await.unwrap();
        assert_eq!(detail["run"]["status"], "cancelled", "{detail}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
