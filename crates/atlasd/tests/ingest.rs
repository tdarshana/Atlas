//! Transcript ingest and the extraction worker.

mod common;
use common::*;
use std::time::Duration;

/// The whole opt-in extraction path: settings turn it on, `POST /ingest` queues a
/// job, the worker asks the model, and the candidates land as pending memories
/// stamped with the extractor. Replaying the same transcript stores nothing.
#[tokio::test]
async fn ingest_extracts_candidates_and_skips_duplicates_on_replay() {
    let stub = stub_llm().await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let transcript = serde_json::json!({"text": "user: we use bun\nassistant: noted", "source_tool": "test"});
    let queued = c.post(format!("{base}/ingest")).json(&transcript).send().await.unwrap();
    assert_eq!(queued.status(), 202);
    let body: serde_json::Value = queued.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap_or_else(|| panic!("no job_id: {body}")).to_string();

    let job = wait_for_job(&c, &base, &job_id).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["result"]["inserted"], 2, "{job}");
    assert_eq!(job["result"]["skipped_duplicates"], 0, "{job}");

    // The route does not hand the transcript back: `jobs` rows are never pruned, so
    // re-serving the payload would leave a second readable copy of the conversation
    // behind every job id. Its length stands in for it, and the rest of the payload,
    // which is what a caller follows a job by, is still there.
    let text = transcript["text"].as_str().unwrap();
    assert!(job["payload"]["text"].is_null(), "the transcript must not be served back: {job}");
    assert_eq!(job["payload"]["chars"], text.chars().count(), "{job}");
    assert_eq!(job["payload"]["source_tool"], "test", "{job}");

    // Neither candidate clears the default auto-accept threshold of 1.0, so both wait
    // for review rather than becoming recallable straight away.
    let pending: serde_json::Value = c.get(format!("{base}/memories?status=pending")).send().await.unwrap().json().await.unwrap();
    let rows = pending.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{pending}");
    for m in rows {
        assert_eq!(m["source_agent"], "extractor", "{m}");
        assert_eq!(m["source_tool"], "test", "{m}");
    }
    assert!(rows.iter().any(|m| m["text"] == "the project uses bun"), "{pending}");
    assert!(rows.iter().any(|m| m["text"] == "deploy target is fly.io"), "{pending}");
    let active: serde_json::Value = c.get(format!("{base}/memories")).send().await.unwrap().json().await.unwrap();
    assert!(active.as_array().unwrap().is_empty(), "nothing is auto-accepted by default: {active}");

    let again: serde_json::Value = c.post(format!("{base}/ingest")).json(&transcript).send().await.unwrap().json().await.unwrap();
    let second = wait_for_job(&c, &base, again["job_id"].as_str().unwrap()).await;
    assert_eq!(second["status"], "done", "{second}");
    assert_eq!(second["result"]["skipped_duplicates"], 2, "{second}");
    assert_eq!(second["result"]["inserted"], 0, "{second}");
    let pending: serde_json::Value = c.get(format!("{base}/memories?status=pending")).send().await.unwrap().json().await.unwrap();
    assert_eq!(pending.as_array().unwrap().len(), 2, "a replay must not double the review queue: {pending}");

    assert_eq!(c.get(format!("{base}/jobs/{}", uuid::Uuid::new_v4())).send().await.unwrap().status(), 404);

    // A blank transcript is refused before a job is queued, so no model call is spent
    // on nothing. Whitespace only counts as blank.
    for blank in ["", "   \n\t "] {
        let empty = c.post(format!("{base}/ingest")).json(&serde_json::json!({"text": blank, "source_tool": "test"})).send().await.unwrap();
        assert_eq!(empty.status(), 400, "a blank transcript must not be queued");
        let body: serde_json::Value = empty.json().await.unwrap();
        assert!(body["error"].as_str().is_some(), "{body}");
    }
    let jobs_after: serde_json::Value = c.get(format!("{base}/memories?status=pending")).send().await.unwrap().json().await.unwrap();
    assert_eq!(jobs_after.as_array().unwrap().len(), 2, "a refused ingest stores nothing: {jobs_after}");
}

/// Extraction is off until it is switched on and fully configured, and each of
/// those states answers 409 with the same error, so nothing is ever queued that
/// the worker could not run.
#[tokio::test]
async fn ingest_is_refused_while_extraction_is_disabled() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let transcript = serde_json::json!({"text": "user: we use bun\nassistant: noted", "source_tool": "test"});

    let refused = c.post(format!("{base}/ingest")).json(&transcript).send().await.unwrap();
    assert_eq!(refused.status(), 409);
    let body: serde_json::Value = refused.json().await.unwrap();
    assert_eq!(body["error"], "extraction is disabled", "{body}");

    // Enabled but with no endpoint or model is still disabled, not a 500 later.
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({"extraction.enabled": true})).send().await.unwrap();
    assert_eq!(put.status(), 200);
    let half = c.post(format!("{base}/ingest")).json(&transcript).send().await.unwrap();
    assert_eq!(half.status(), 409);
    let body: serde_json::Value = half.json().await.unwrap();
    assert_eq!(body["error"], "extraction is disabled", "{body}");
}

/// The actor for `POST /ingest` comes from `X-Atlas-Actor` when present, falls back
/// to the deprecated `source_tool` body field, and is a 400 naming both ways to send
/// it when neither is there. The header wins when both are sent.
#[tokio::test]
async fn ingest_actor_comes_from_the_header_over_the_deprecated_body_field() {
    let stub = stub_llm().await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();

    // The header wins over a body field that disagrees with it.
    let queued = c.post(format!("{base}/ingest")).header("X-Atlas-Actor", "from-header")
        .json(&serde_json::json!({"text": "user: a\nassistant: b", "source_tool": "from-body"}))
        .send().await.unwrap();
    assert_eq!(queued.status(), 202);
    let body: serde_json::Value = queued.json().await.unwrap();
    let job = wait_for_job(&c, &base, body["job_id"].as_str().unwrap()).await;
    assert_eq!(job["payload"]["source_tool"], "from-header", "{job}");

    // No body field at all still works from the header alone.
    let header_only = c.post(format!("{base}/ingest")).header("X-Atlas-Actor", "header-only")
        .json(&serde_json::json!({"text": "user: c\nassistant: d"}))
        .send().await.unwrap();
    assert_eq!(header_only.status(), 202, "{:?}", header_only.text().await);

    // Neither the header nor the deprecated body field is a 400, not a queued job
    // with no attribution.
    let neither = c.post(format!("{base}/ingest")).json(&serde_json::json!({"text": "user: e\nassistant: f"})).send().await.unwrap();
    assert_eq!(neither.status(), 400);
    let body: serde_json::Value = neither.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("actor"), "{body}");
}

/// `POST /ingest` refuses a transcript over the 1,000,000 character cap with 413
/// before it ever reaches the queue, so no oversized body can spend a model call. The
/// cap now lives in the backend, which is what MCP reaches too, and the route maps
/// `AtlasError::TooLarge` onto the status.
#[tokio::test]
async fn ingest_refuses_a_transcript_over_the_size_cap() {
    let stub = stub_llm().await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let text = "x".repeat(1_000_001);
    let big = c.post(format!("{base}/ingest")).json(&serde_json::json!({"text": text, "source_tool": "test"})).send().await.unwrap();
    assert_eq!(big.status(), 413);
    let body: serde_json::Value = big.json().await.unwrap();
    assert_eq!(body["error"], "transcript too large", "{body}");
}

/// A job queued while extraction was on, and run after it was switched off, ends
/// `failed` with the disabled message. The worker re-reads the settings at the top of
/// every job, so the alternative would be a job stuck `queued` for ever behind a 202
/// its caller is still polling.
#[tokio::test]
async fn a_job_queued_before_extraction_was_disabled_fails_rather_than_hanging() {
    // The first job holds the worker on the model call for two seconds, which is what
    // keeps the second one queued while the settings change lands.
    let stub = stub_llm_with_delay(STUB_CANDIDATES, Duration::from_secs(2)).await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let queue = |text: &str| {
        let body = serde_json::json!({"text": text, "source_tool": "test"});
        c.post(format!("{base}/ingest")).json(&body).send()
    };
    let slow: serde_json::Value = queue("user: the first transcript").await.unwrap().json().await.unwrap();
    let waiting: serde_json::Value = queue("user: the second transcript").await.unwrap().json().await.unwrap();
    let waiting_id = waiting["job_id"].as_str().unwrap_or_else(|| panic!("no job_id: {waiting}")).to_string();
    assert!(slow["job_id"].is_string(), "{slow}");

    let off = c.put(format!("{base}/settings")).json(&serde_json::json!({"extraction.enabled": false})).send().await.unwrap();
    assert_eq!(off.status(), 200);

    let job = wait_for_job(&c, &base, &waiting_id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"], "extraction is disabled", "{job}");
}

/// The gate and the worker resolve one project from one root. A `project_root` naming
/// a subdirectory of the repository is still that project: its override decides which
/// endpoint the transcript goes to (the global settings stay off throughout), and its
/// `memory_writers` decides who may queue one at all.
#[tokio::test]
async fn ingest_resolves_the_project_from_a_subdirectory_for_both_the_gate_and_the_worker() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());
    let subdir = dir.path().join("src");
    let llm = stub_llm().await;

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    c.put(format!("{base}/projects/{id}/extraction")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"enabled": true, "base_url": llm, "model": "project-model", "api_key": "sk-project"}))
        .send().await.unwrap();

    // The global settings are off, so a transcript from a directory Atlas does not know
    // has nowhere to go: this is the control for the case below.
    let elsewhere = repo_free_tempdir();
    let unknown = c.post(format!("{base}/ingest")).json(&serde_json::json!({
        "text": "we chose bun", "source_tool": "codex", "project_root": elsewhere.path()
    })).send().await.unwrap();
    assert_eq!(unknown.status(), 409, "no project, and the global settings are off");

    // The same transcript from inside the repository resolves to the project, so the
    // project's own endpoint runs it.
    let queued = c.post(format!("{base}/ingest")).json(&serde_json::json!({
        "text": "we chose bun", "source_tool": "codex", "project_root": subdir
    })).send().await.unwrap();
    assert_eq!(queued.status(), 202, "a subdirectory is still the project");
    let job_id = queued.json::<serde_json::Value>().await.unwrap()["job_id"].as_str().unwrap().to_string();
    let job = wait_for_job(&c, &base, &job_id).await;
    assert_eq!(job["status"], "done", "{job}");
    // The gate recorded the same project the worker then used, so the memories are the
    // project's, not global.
    let mine: serde_json::Value = c.get(format!("{base}/memories?status=pending&project_id={id}&scope=project_only")).send().await.unwrap().json().await.unwrap();
    assert_eq!(mine.as_array().unwrap().len(), 2, "{mine}");

    // And the allow-list is checked against that same project, from a subdirectory too.
    c.put(format!("{base}/projects/{id}/agent-access")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"memory_writers": ["claude-code"]})).send().await.unwrap();
    let refused = c.post(format!("{base}/ingest")).json(&serde_json::json!({
        "text": "another transcript", "source_tool": "codex", "project_root": subdir
    })).send().await.unwrap();
    assert_eq!(refused.status(), 409);
    let body: serde_json::Value = refused.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("may not write memories"), "{body}");
}

/// `require_review` reaches the extraction worker, not just `POST /memories`: a
/// transcript an agent ingests into a project that requires review lands `pending`
/// however confident the model was, while the user's own hands still auto-accept.
#[tokio::test]
async fn require_review_holds_back_extracted_memories() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    // One candidate per stub, each well above the auto-accept bar, and each a
    // different sentence so the worker's duplicate check never skips one.
    let candidate = |text: &str| format!(r#"[{{"text":"{text}","kind":"fact","tags":[],"confidence":0.95}}]"#);
    let before = stub_llm_with_reply(&candidate("the runtime here is bun")).await;
    let after = stub_llm_with_reply(&candidate("the deploy target here is fly.io")).await;
    let by_hand = stub_llm_with_reply(&candidate("the package manager here is bun")).await;

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();

    let configure = |url: &str| c.put(format!("{base}/settings")).header("X-Atlas-Actor", "desktop").json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": url, "extraction.model": "stub",
        "extraction.auto_accept_min_confidence": 0.5,
    })).send();
    let ingest = |tool: &str| c.post(format!("{base}/ingest")).json(&serde_json::json!({
        "text": "user: a transcript", "source_tool": tool, "project_root": dir.path()
    })).send();
    let run = |c: &reqwest::Client, base: String, queued: reqwest::Response| {
        let c = c.clone();
        async move {
            assert_eq!(queued.status(), 202);
            let job_id = queued.json::<serde_json::Value>().await.unwrap()["job_id"].as_str().unwrap().to_string();
            let job = wait_for_job(&c, &base, &job_id).await;
            assert_eq!(job["status"], "done", "{job}");
        }
    };
    let statuses = |status: &str| {
        let (c, base, id, status) = (c.clone(), base.clone(), id.clone(), status.to_string());
        async move {
            let rows: serde_json::Value = c.get(format!("{base}/memories?status={status}&project_id={id}&scope=project_only"))
                .send().await.unwrap().json().await.unwrap();
            rows.as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap().to_string()).collect::<Vec<_>>()
        }
    };

    // Control: with review off, an agent's confident candidate goes straight in.
    configure(&before).await.unwrap();
    run(&c, base.clone(), ingest("codex").await.unwrap()).await;
    assert_eq!(statuses("active").await, vec!["the runtime here is bun"]);

    // With review on, the same agent's candidate waits, whatever its confidence.
    c.put(format!("{base}/projects/{id}/agent-access")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"require_review": true})).send().await.unwrap();
    configure(&after).await.unwrap();
    run(&c, base.clone(), ingest("codex").await.unwrap()).await;
    assert_eq!(statuses("pending").await, vec!["the deploy target here is fly.io"]);
    assert_eq!(statuses("active").await, vec!["the runtime here is bun"], "the earlier memory is untouched");

    // The desktop is not an agent, so review does not hold its transcript back.
    configure(&by_hand).await.unwrap();
    run(&c, base.clone(), ingest("desktop").await.unwrap()).await;
    let mut active = statuses("active").await;
    active.sort();
    assert_eq!(active, vec!["the package manager here is bun", "the runtime here is bun"]);
}
