//! Memories: the JSON round trip, review, paging, scoping, facets and search.

mod common;
use common::*;

#[tokio::test]
async fn json_api_round_trip() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let st: serde_json::Value = c.get(format!("{base}/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["memories_active"], 0);
    assert_eq!(st["port"], d.port);
    let created: serde_json::Value = c.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"global","kind":"fact","text":"the runtime is bun","tags":["tooling"]})).send().await.unwrap().json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    let hits: serde_json::Value = c.post(format!("{base}/memories/search")).json(&serde_json::json!({"query":"which runtime"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(hits[0]["memory"]["id"], id);
    let r = c.post(format!("{base}/memories/{id}/forget")).json(&serde_json::json!({"reason":"test"})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let hits: serde_json::Value = c.post(format!("{base}/memories/search")).json(&serde_json::json!({"query":"runtime"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 0);
    let missing = c.get(format!("{base}/memories/{}", uuid::Uuid::new_v4())).send().await.unwrap();
    assert_eq!(missing.status(), 404);
    // ARCH-12: the body names the variant, so a client rebuilds it rather than guessing
    // from the status code.
    let missing_body: serde_json::Value = missing.json().await.unwrap();
    assert_eq!(missing_body["kind"], "not_found", "{missing_body}");

    let bad_create = c.post(format!("{base}/memories")).header("Content-Type", "application/json").body("{\"scope\":\"global\"").send().await.unwrap();
    assert_eq!(bad_create.status(), 400);
    let bad_create_body: serde_json::Value = bad_create.json().await.unwrap();
    assert!(bad_create_body["error"].as_str().is_some(), "{bad_create_body}");
    assert_eq!(bad_create_body["kind"], "invalid", "{bad_create_body}");

    let bad_get = c.get(format!("{base}/memories/not-a-uuid")).send().await.unwrap();
    assert_eq!(bad_get.status(), 400);
    let bad_get_body: serde_json::Value = bad_get.json().await.unwrap();
    assert!(bad_get_body["error"].as_str().is_some(), "{bad_get_body}");

    let bad_forget = c.post(format!("{base}/memories/{id}/forget")).header("Content-Type", "application/json").body("{garbage").send().await.unwrap();
    assert_eq!(bad_forget.status(), 400);
    let bad_forget_body: serde_json::Value = bad_forget.json().await.unwrap();
    assert!(bad_forget_body["error"].as_str().is_some(), "{bad_forget_body}");

    let untyped_forget = c.post(format!("{base}/memories/{id}/forget")).body("{\"reason\":\"test\"}").send().await.unwrap();
    assert_eq!(untyped_forget.status(), 400, "a body without a JSON content type must be refused");

    let daemon_json = std::fs::read_to_string(d.home.path().join("daemon.json")).unwrap();
    assert!(daemon_json.contains(&format!("\"port\":{}", d.port)) || daemon_json.contains(&format!("\"port\": {}", d.port)));
}

/// `memories_pending` counts memories set to `pending`, separately from
/// `memories_active`, so the desktop app's notification poller can tell the two apart.
#[tokio::test]
async fn status_reports_the_pending_memory_count() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let st: serde_json::Value = c.get(format!("{base}/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["memories_pending"], 0, "{st}");

    let created: serde_json::Value = c.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"global","kind":"fact","text":"pending fact"})).send().await.unwrap().json().await.unwrap();
    let id = created["id"].as_str().unwrap();
    let set = c.post(format!("{base}/memories/{id}/status")).json(&serde_json::json!({"status":"pending"})).send().await.unwrap();
    assert_eq!(set.status(), 200);

    let st: serde_json::Value = c.get(format!("{base}/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["memories_pending"], 1, "{st}");
    assert_eq!(st["memories_active"], 0, "{st}");
}

/// Memories captured for review land as `pending`: invisible to recall until a status
/// change accepts them, at which point they become recallable.
#[tokio::test]
async fn pending_memories_can_be_listed_and_accepted() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let m: serde_json::Value = c.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"global","kind":"insight","text":"the cache is warmed on boot","status":"pending"})).send().await.unwrap().json().await.unwrap();
    let id = m["id"].as_str().unwrap().to_string();
    assert_eq!(m["status"], "pending");

    let hits: serde_json::Value = c.post(format!("{base}/memories/search")).json(&serde_json::json!({"query":"cache warmed"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 0, "a pending memory is not recallable");

    let pending: serde_json::Value = c.get(format!("{base}/memories?status=pending")).send().await.unwrap().json().await.unwrap();
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(pending[0]["id"], id);
    let active: serde_json::Value = c.get(format!("{base}/memories")).send().await.unwrap().json().await.unwrap();
    assert_eq!(active.as_array().unwrap().len(), 0, "status defaults to active");

    let accepted = c.post(format!("{base}/memories/{id}/status")).json(&serde_json::json!({"status":"active"})).send().await.unwrap();
    assert_eq!(accepted.status(), 200);
    let hits: serde_json::Value = c.post(format!("{base}/memories/search")).json(&serde_json::json!({"query":"cache warmed"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(hits[0]["memory"]["id"], id, "accepting a memory adds it to the search index");

    let rejected = c.post(format!("{base}/memories/{id}/status")).json(&serde_json::json!({"status":"rejected"})).send().await.unwrap();
    assert_eq!(rejected.status(), 200);
    let hits: serde_json::Value = c.post(format!("{base}/memories/search")).json(&serde_json::json!({"query":"cache warmed"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 0, "rejecting removes it from the index again");

    assert_eq!(c.get(format!("{base}/memories?status=bogus")).send().await.unwrap().status(), 400);
    assert_eq!(c.post(format!("{base}/memories/{id}/status")).json(&serde_json::json!({"status":"bogus"})).send().await.unwrap().status(), 400);
}

/// `GET /memories` takes `limit` and `offset`, windowing the newest-first list; without
/// them the whole set comes back as before, and `limit` is capped at 1000.
#[tokio::test]
async fn memories_can_be_listed_a_page_at_a_time() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    for i in 0..4 {
        let r = c.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"global","kind":"fact","text":format!("fact {i}")})).send().await.unwrap();
        assert_eq!(r.status(), 201);
    }
    let all: serde_json::Value = c.get(format!("{base}/memories?status=active")).send().await.unwrap().json().await.unwrap();
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 4);

    let page: serde_json::Value = c.get(format!("{base}/memories?status=active&limit=2&offset=1")).send().await.unwrap().json().await.unwrap();
    let page = page.as_array().unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0]["id"], all[1]["id"]);
    assert_eq!(page[1]["id"], all[2]["id"]);

    let first: serde_json::Value = c.get(format!("{base}/memories?limit=1")).send().await.unwrap().json().await.unwrap();
    assert_eq!(first.as_array().unwrap().len(), 1);
    assert_eq!(first[0]["id"], all[0]["id"]);

    let capped: serde_json::Value = c.get(format!("{base}/memories?limit=99999")).send().await.unwrap().json().await.unwrap();
    assert_eq!(capped.as_array().unwrap().len(), 4, "an oversized limit is capped, not refused");
    assert_eq!(c.get(format!("{base}/memories?limit=two")).send().await.unwrap().status(), 400);
}

// ---- search ----

/// `GET /search` fans a query out across kinds: a distinctive task word finds only
/// the task group, a distinctive memory word finds only the memory group, an unknown
/// `kinds` value is a 400, and `kinds=` limits which groups can appear at all.
#[tokio::test]
async fn global_search_finds_tasks_and_memories_and_validates_kinds() {
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let pid = p["id"].as_str().unwrap().to_string();

    let task: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"project_id": pid, "title": "fix the flibbertigibbet bug"}))
        .send().await.unwrap().json().await.unwrap();
    let task_key = task["key"].as_str().unwrap().to_string();

    let memory: serde_json::Value = c.post(format!("{base}/memories"))
        .json(&serde_json::json!({"scope": "global", "kind": "fact", "text": "the wobblesnark runtime is bun"}))
        .send().await.unwrap().json().await.unwrap();
    let memory_id = memory["id"].as_str().unwrap().to_string();

    let task_hits: serde_json::Value = c.get(format!("{base}/search?q=flibbertigibbet")).send().await.unwrap().json().await.unwrap();
    let kinds: Vec<&str> = task_hits["groups"].as_array().unwrap().iter().map(|g| g["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["task"], "{task_hits}");
    assert_eq!(task_hits["groups"][0]["items"][0]["reference"], task_key, "{task_hits}");
    assert!(task_hits["took_ms"].is_u64(), "{task_hits}");
    assert_eq!(task_hits["total"], 1, "{task_hits}");

    let mem_hits: serde_json::Value = c.get(format!("{base}/search?q=wobblesnark")).send().await.unwrap().json().await.unwrap();
    let kinds: Vec<&str> = mem_hits["groups"].as_array().unwrap().iter().map(|g| g["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["memory"], "{mem_hits}");
    assert_eq!(mem_hits["groups"][0]["items"][0]["id"], memory_id, "{mem_hits}");

    let bad = c.get(format!("{base}/search?q=x&kinds=bogus")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let body: serde_json::Value = bad.json().await.unwrap();
    assert!(body["error"].as_str().is_some(), "{body}");

    let both: serde_json::Value = c.get(format!("{base}/search?q=flibbertigibbet&kinds=task,memory")).send().await.unwrap().json().await.unwrap();
    let both_kinds: Vec<&str> = both["groups"].as_array().unwrap().iter().map(|g| g["kind"].as_str().unwrap()).collect();
    assert_eq!(both_kinds, vec!["task"], "{both}: the task word has no memory hit, so only the task group appears");

    let memory_only: serde_json::Value = c.get(format!("{base}/search?q=flibbertigibbet&kinds=memory")).send().await.unwrap().json().await.unwrap();
    assert!(memory_only["groups"].as_array().unwrap().is_empty(), "restricting to kinds=memory must drop the task hit: {memory_only}");

    let empty: serde_json::Value = c.get(format!("{base}/search?q=")).send().await.unwrap().json().await.unwrap();
    assert!(empty["groups"].as_array().unwrap().is_empty());
    assert_eq!(empty["total"], 0);
}

/// `scope=project_only` narrows a listing to the project's own memories; the default
/// still widens to the global ones, and asking to narrow with no project is a 400.
#[tokio::test]
async fn memories_can_be_listed_for_one_project_only() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "global", "kind": "fact", "text": "a global memory"
    })).send().await.unwrap();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "project", "project_id": id, "kind": "fact", "text": "a project memory"
    })).send().await.unwrap();

    let widened: serde_json::Value = c.get(format!("{base}/memories?project_id={id}")).send().await.unwrap().json().await.unwrap();
    let texts: Vec<&str> = widened.as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap()).collect();
    assert!(texts.contains(&"a global memory") && texts.contains(&"a project memory"), "{texts:?}");
    // The absent, blank and explicit `all` spellings all mean the same thing.
    for query in ["", "&scope=", "&scope=all"] {
        let all: serde_json::Value = c.get(format!("{base}/memories?project_id={id}{query}")).send().await.unwrap().json().await.unwrap();
        assert_eq!(all.as_array().unwrap().len(), 2, "'{query}': {all}");
    }

    let narrowed = c.get(format!("{base}/memories?project_id={id}&scope=project_only")).send().await.unwrap();
    assert_eq!(narrowed.status(), 200);
    let narrowed: serde_json::Value = narrowed.json().await.unwrap();
    let texts: Vec<&str> = narrowed.as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["a project memory"], "the global memory must be excluded");

    let no_project = c.get(format!("{base}/memories?scope=project_only")).send().await.unwrap();
    assert_eq!(no_project.status(), 400);
    let body: serde_json::Value = no_project.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("project_id"), "{body}");

    let bad = c.get(format!("{base}/memories?project_id={id}&scope=nonsense")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
}

/// `GET /memories/facets` counts kinds and tags over the active set, scoped by
/// `project_id`/`scope` the same way `GET /memories` reads them, without a client
/// having to load every memory first.
#[tokio::test]
async fn memory_facets_counts_active_memories_scoped_like_the_list_route() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "global", "kind": "fact", "text": "a global memory", "tags": ["alpha"]
    })).send().await.unwrap();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "project", "project_id": id, "kind": "decision", "text": "a project memory", "tags": ["alpha", "beta"]
    })).send().await.unwrap();

    let widened: serde_json::Value = c.get(format!("{base}/memories/facets?project_id={id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(widened["total"], 2, "{widened}");
    assert_eq!(widened["kinds"]["fact"], 1, "{widened}");
    assert_eq!(widened["kinds"]["decision"], 1, "{widened}");
    assert_eq!(widened["tags"]["alpha"], 2, "{widened}");
    assert_eq!(widened["tags"]["beta"], 1, "{widened}");

    let narrowed: serde_json::Value = c.get(format!("{base}/memories/facets?project_id={id}&scope=project_only")).send().await.unwrap().json().await.unwrap();
    assert_eq!(narrowed["total"], 1, "{narrowed}");
    assert_eq!(narrowed["kinds"]["decision"], 1, "{narrowed}");
    assert!(narrowed["kinds"].get("fact").is_none(), "{narrowed}");

    let no_project = c.get(format!("{base}/memories/facets?scope=project_only")).send().await.unwrap();
    assert_eq!(no_project.status(), 400);

    let bad = c.get(format!("{base}/memories/facets?project_id={id}&scope=nonsense")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
}

/// `POST /memories/search` takes the same narrowing the listing does, under
/// `list_scope`: the project Memories tab's search box must not mix the global
/// memories back in, and the field must not collide with `scope`, which still names
/// the memory's own scope.
#[tokio::test]
async fn memory_search_can_be_narrowed_to_one_project() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "global", "kind": "fact", "text": "the global runtime is bun"
    })).send().await.unwrap();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "project", "project_id": id, "kind": "fact", "text": "the project runtime is bun"
    })).send().await.unwrap();

    let search = |body: serde_json::Value| c.post(format!("{base}/memories/search")).json(&body).send();

    // Absent, and the explicit `all`, both widen to the project plus the global rows.
    for list_scope in [serde_json::Value::Null, serde_json::Value::String("all".into())] {
        let mut body = serde_json::json!({"query": "runtime", "project_id": id});
        if !list_scope.is_null() { body["list_scope"] = list_scope.clone(); }
        let r = search(body).await.unwrap();
        assert_eq!(r.status(), 200);
        let hits: serde_json::Value = r.json().await.unwrap();
        assert_eq!(hits.as_array().unwrap().len(), 2, "{list_scope:?}: {hits}");
    }

    let narrowed = search(serde_json::json!({"query": "runtime", "project_id": id, "list_scope": "project_only"})).await.unwrap();
    assert_eq!(narrowed.status(), 200, "the search route must accept project_only");
    let narrowed: serde_json::Value = narrowed.json().await.unwrap();
    let texts: Vec<&str> = narrowed.as_array().unwrap().iter().map(|h| h["memory"]["text"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["the project runtime is bun"], "the global memory must be excluded");

    let no_project = search(serde_json::json!({"query": "runtime", "list_scope": "project_only"})).await.unwrap();
    assert_eq!(no_project.status(), 400);
    let body: serde_json::Value = no_project.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("project_id"), "{body}");

    // `scope` still means the memory's own scope, and the two are independent.
    let globals = search(serde_json::json!({"query": "runtime", "scope": "global"})).await.unwrap();
    assert_eq!(globals.status(), 200);
    let globals: serde_json::Value = globals.json().await.unwrap();
    assert_eq!(globals.as_array().unwrap().len(), 1, "{globals}");
}
