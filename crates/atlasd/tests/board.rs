//! The task board: tasks, stages, counts, scoping and the TASKS.md mirror.

mod common;
use common::*;

/// Every 400 the API returns carries the `{"error": string}` body, query strings included:
/// axum's own rejections are plain text, so the extractors have to own the mapping.
#[tokio::test]
async fn bad_query_strings_are_json_errors() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let bad_project = c.get(format!("{base}/memories?project_id=nope")).send().await.unwrap();
    assert_eq!(bad_project.status(), 400);
    let body: serde_json::Value = bad_project.json().await.unwrap();
    assert!(body["error"].as_str().is_some(), "{body}");

    let bad_docs = c.get(format!("{base}/practices?project_id=nope")).send().await.unwrap();
    assert_eq!(bad_docs.status(), 400);
    let body: serde_json::Value = bad_docs.json().await.unwrap();
    assert!(body["error"].as_str().is_some(), "{body}");

    // A blank filter is a caller who left it empty, not a bad status.
    let blank = c.get(format!("{base}/memories?status=")).send().await.unwrap();
    assert_eq!(blank.status(), 200);
    assert_eq!(blank.json::<serde_json::Value>().await.unwrap().as_array().unwrap().len(), 0);

    // A file is not a project root, however well the path resolves.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-dir.txt");
    std::fs::write(&file, "x").unwrap();
    let not_dir = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": file})).send().await.unwrap();
    assert_eq!(not_dir.status(), 400);
    let body: serde_json::Value = not_dir.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("is not a directory"), "{body}");
}

/// `POST /tasks` answers 201 with a key matching `^[A-Z0-9]+-\d+$`; `GET /tasks?stage=`
/// filters by stage; `ready=true` excludes a task blocked by an open task and includes
/// it once the blocker reaches a done stage; moving to an unknown stage is a 400
/// naming the valid ones.
#[tokio::test]
async fn board_tasks_ready_query_and_stage_moves() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let created = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "the blocker"})).send().await.unwrap();
    assert_eq!(created.status(), 201);
    let blocker: serde_json::Value = created.json().await.unwrap();
    assert!(looks_like_a_task_key(blocker["key"].as_str().unwrap()), "{blocker}");
    assert_eq!(blocker["stage"], "Backlog");
    let blocker_key = blocker["key"].as_str().unwrap().to_string();

    let dependent: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"title": "the dependent", "blocked_by": [blocker_key]}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(dependent["ready"], false, "{dependent}");
    let dependent_key = dependent["key"].as_str().unwrap().to_string();

    let backlog: serde_json::Value = c.get(format!("{base}/tasks?stage=Backlog")).send().await.unwrap().json().await.unwrap();
    let keys: Vec<&str> = backlog.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&blocker_key.as_str()) && keys.contains(&dependent_key.as_str()), "{backlog:?}");

    let ready: serde_json::Value = c.get(format!("{base}/tasks?ready=true")).send().await.unwrap().json().await.unwrap();
    let ready_keys: Vec<&str> = ready.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert!(ready_keys.contains(&blocker_key.as_str()), "{ready:?}");
    assert!(!ready_keys.contains(&dependent_key.as_str()), "the dependent must not be ready while its blocker is open: {ready:?}");

    let bad_move = c.post(format!("{base}/tasks/{blocker_key}/move")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"stage": "Nope"})).send().await.unwrap();
    assert_eq!(bad_move.status(), 400);
    let body: serde_json::Value = bad_move.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Backlog"), "{body}");

    let moved = c.post(format!("{base}/tasks/{blocker_key}/move")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"stage": "Done"})).send().await.unwrap();
    assert_eq!(moved.status(), 200);
    let moved: serde_json::Value = moved.json().await.unwrap();
    assert_eq!(moved["stage"], "Done");
    assert!(!moved["closed_at"].is_null(), "a done stage stamps closed_at: {moved}");

    let ready_after: serde_json::Value = c.get(format!("{base}/tasks?ready=true")).send().await.unwrap().json().await.unwrap();
    let ready_after_keys: Vec<&str> = ready_after.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert!(ready_after_keys.contains(&dependent_key.as_str()), "the dependent should be ready once its blocker is done: {ready_after:?}");
}

/// A list route answers with a list. `ready=1` is the same ask as `ready=true`, and a
/// value that is neither is a filter left off rather than a 400; `include_done` reads
/// the same way.
#[tokio::test]
async fn board_list_flags_take_true_or_one_and_never_answer_400() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let open: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "open"})).send().await.unwrap().json().await.unwrap();
    let closed: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "closed"})).send().await.unwrap().json().await.unwrap();
    let closed_key = closed["key"].as_str().unwrap().to_string();
    c.post(format!("{base}/tasks/{closed_key}/move")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"stage": "Done"})).send().await.unwrap();

    for (query, want) in [("ready=1", 1_usize), ("ready=true", 1), ("ready=0", 2), ("ready=yes", 2), ("ready=", 2)] {
        let r = c.get(format!("{base}/tasks?{query}&include_done=1")).send().await.unwrap();
        assert_eq!(r.status(), 200, "{query} should not be a 400");
        let list: serde_json::Value = r.json().await.unwrap();
        assert_eq!(list.as_array().unwrap().len(), want, "{query}: {list}");
    }

    // `include_done=1` is what let the done task into those counts; without it only
    // the open task comes back.
    let list: serde_json::Value = c.get(format!("{base}/tasks")).send().await.unwrap().json().await.unwrap();
    let keys: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec![open["key"].as_str().unwrap()]);
}

/// PERF-5 (ATL-307): `GET /tasks?brief=1` is the board's listing. Its rows carry an
/// empty `description` and no `source_ref`, and `GET /tasks/{key}` still answers
/// with the full task.
#[tokio::test]
async fn brief_task_list_leaves_the_description_out() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let created: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"title": "brief me", "description": "the whole story"}))
        .send().await.unwrap().json().await.unwrap();
    let key = created["key"].as_str().unwrap().to_string();

    let full: serde_json::Value = c.get(format!("{base}/tasks")).send().await.unwrap().json().await.unwrap();
    assert_eq!(full[0]["description"], "the whole story", "{full}");

    for query in ["brief=1", "brief=true"] {
        let r = c.get(format!("{base}/tasks?{query}")).send().await.unwrap();
        assert_eq!(r.status(), 200, "{query}");
        let brief: serde_json::Value = r.json().await.unwrap();
        assert_eq!(brief[0]["key"], key, "{query}: {brief}");
        assert_eq!(brief[0]["description"], "", "{query}: {brief}");
        assert!(brief[0]["source_ref"].is_null(), "{query}: {brief}");
    }

    let detail: serde_json::Value = c.get(format!("{base}/tasks/{key}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(detail["task"]["description"], "the whole story", "{detail}");
}

/// `scope=global` is the literal global board: tasks with no project at all, not a
/// bare `project_id`-less request, which leaves every project's tasks in. It is
/// refused alongside `project_id`.
#[tokio::test]
async fn tasks_scope_global_keeps_just_the_project_less_tasks() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    let global: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "no project"})).send().await.unwrap().json().await.unwrap();
    c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "has a project", "project_id": id})).send().await.unwrap();

    let list: serde_json::Value = c.get(format!("{base}/tasks?scope=global")).send().await.unwrap().json().await.unwrap();
    let keys: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec![global["key"].as_str().unwrap()], "{list}");

    let unfiltered: serde_json::Value = c.get(format!("{base}/tasks")).send().await.unwrap().json().await.unwrap();
    assert_eq!(unfiltered.as_array().unwrap().len(), 2, "no filter still shows every task: {unfiltered}");

    let both = c.get(format!("{base}/tasks?scope=global&project_id={id}")).send().await.unwrap();
    assert_eq!(both.status(), 400);
    let body: serde_json::Value = both.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("project_id") && body["error"].as_str().unwrap().contains("scope=global"), "{body}");

    let bad = c.get(format!("{base}/tasks?scope=nonsense")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
}

/// `/tasks/counts` reads `project_id`/`scope` exactly like `/tasks` does: a bare
/// request counts every project's tasks (not just the project-less ones), `scope=
/// global` narrows to just the project-less ones, and `project_id` together with
/// `scope=global` is refused the same way.
#[tokio::test]
async fn task_counts_follows_the_same_scope_rules_as_task_list() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "in a project", "project_id": id})).send().await.unwrap();
    c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "no project"})).send().await.unwrap();

    let sum_backlog = |counts: &serde_json::Value| -> i64 {
        counts.as_array().unwrap().iter().find(|c| c["stage"] == "Backlog").unwrap()["count"].as_i64().unwrap()
    };

    let bare: serde_json::Value = c.get(format!("{base}/tasks/counts")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sum_backlog(&bare), 2, "a bare request should count every project's tasks: {bare}");

    let global: serde_json::Value = c.get(format!("{base}/tasks/counts?scope=global")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sum_backlog(&global), 1, "scope=global should count just the project-less task: {global}");

    let scoped: serde_json::Value = c.get(format!("{base}/tasks/counts?project_id={id}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sum_backlog(&scoped), 1, "project_id should count just that project's task: {scoped}");

    let both = c.get(format!("{base}/tasks/counts?scope=global&project_id={id}")).send().await.unwrap();
    assert_eq!(both.status(), 400);
}

/// A stale `expected_updated_at` on `PATCH` is a 409; claiming a task alice holds
/// fails for bob with 409 and succeeds with `force`; a comment lands as an event in
/// `GET /tasks/{key}` carrying the actor from the `X-Atlas-Actor` header.
#[tokio::test]
async fn board_stale_update_claim_conflict_and_comment_events() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let created: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "take this"})).send().await.unwrap().json().await.unwrap();
    let key = created["key"].as_str().unwrap().to_string();
    let updated_at = created["updated_at"].as_str().unwrap().to_string();

    let stale = c.patch(format!("{base}/tasks/{key}")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"title": "renamed", "expected_updated_at": "2000-01-01T00:00:00Z"})).send().await.unwrap();
    assert_eq!(stale.status(), 409);

    let ok = c.patch(format!("{base}/tasks/{key}")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"title": "renamed", "expected_updated_at": updated_at})).send().await.unwrap();
    assert_eq!(ok.status(), 200);

    let claimed = c.post(format!("{base}/tasks/{key}/claim")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({})).send().await.unwrap();
    assert_eq!(claimed.status(), 200);
    let claimed: serde_json::Value = claimed.json().await.unwrap();
    assert_eq!(claimed["assignee"], "alice");
    assert_eq!(claimed["stage"], "In Progress", "claiming from the first stage should advance it: {claimed}");

    let bob_fails = c.post(format!("{base}/tasks/{key}/claim")).header("X-Atlas-Actor", "bob").json(&serde_json::json!({})).send().await.unwrap();
    assert_eq!(bob_fails.status(), 409);

    let bob_forces = c.post(format!("{base}/tasks/{key}/claim")).header("X-Atlas-Actor", "bob").json(&serde_json::json!({"force": true})).send().await.unwrap();
    assert_eq!(bob_forces.status(), 200);
    let bob_forces: serde_json::Value = bob_forces.json().await.unwrap();
    assert_eq!(bob_forces["assignee"], "bob");

    let commented = c.post(format!("{base}/tasks/{key}/comment")).header("X-Atlas-Actor", "carol").json(&serde_json::json!({"body": "looking into it"})).send().await.unwrap();
    assert_eq!(commented.status(), 200);

    let detail: serde_json::Value = c.get(format!("{base}/tasks/{key}")).send().await.unwrap().json().await.unwrap();
    let events = detail["events"].as_array().unwrap();
    let last = events.last().unwrap();
    assert_eq!(last["actor"], "carol", "{detail}");
    assert_eq!(last["kind"], "commented", "{detail}");
    assert_eq!(last["body"], "looking into it", "{detail}");
}

/// `PUT /board/stages` refuses a list under the two-stage minimum; a project
/// override makes `GET /board/stages?project_id=` report `overridden: true`, and
/// clearing it with `stages: null` restores the global list and `overridden: false`.
#[tokio::test]
async fn board_stage_administration() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let too_few = c.put(format!("{base}/board/stages")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"stages": [{"name": "Only", "done": true}]})).send().await.unwrap();
    assert_eq!(too_few.status(), 400);

    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let pid = p["id"].as_str().unwrap().to_string();

    let before: serde_json::Value = c.get(format!("{base}/board/stages?project_id={pid}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(before["overridden"], false, "{before}");

    let overridden = c.put(format!("{base}/projects/{pid}/stages")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"stages": [{"name": "To Do", "done": false}, {"name": "Shipped", "done": true}]})).send().await.unwrap();
    assert_eq!(overridden.status(), 200);
    let overridden: serde_json::Value = overridden.json().await.unwrap();
    assert_eq!(overridden["overridden"], true, "{overridden}");
    assert_eq!(overridden["stages"][0]["name"], "To Do");

    let after: serde_json::Value = c.get(format!("{base}/board/stages?project_id={pid}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["overridden"], true, "{after}");

    let cleared = c.put(format!("{base}/projects/{pid}/stages")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"stages": null})).send().await.unwrap();
    assert_eq!(cleared.status(), 200);
    let cleared: serde_json::Value = cleared.json().await.unwrap();
    assert_eq!(cleared["overridden"], false, "{cleared}");
}

/// `DELETE` answers 204 and a following `GET` is 404; `X-Atlas-Actor` over 64
/// characters is a 400; a board route without a loopback `Host` is 403, same as
/// every other route.
#[tokio::test]
async fn board_delete_actor_header_limit_and_loopback_guard() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let created: serde_json::Value = c.post(format!("{base}/tasks")).json(&serde_json::json!({"title": "throwaway"})).send().await.unwrap().json().await.unwrap();
    let key = created["key"].as_str().unwrap().to_string();

    let deleted = c.delete(format!("{base}/tasks/{key}")).header("X-Atlas-Actor", "alice").send().await.unwrap();
    assert_eq!(deleted.status(), 204);
    let gone = c.get(format!("{base}/tasks/{key}")).send().await.unwrap();
    assert_eq!(gone.status(), 404);

    let long_actor = "a".repeat(65);
    let refused = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", long_actor).json(&serde_json::json!({"title": "x"})).send().await.unwrap();
    assert_eq!(refused.status(), 400);
    let body: serde_json::Value = refused.json().await.unwrap();
    assert!(body["error"].as_str().is_some(), "{body}");

    let rebound = c.get(format!("{base}/tasks")).header("Host", "evil.example").send().await.unwrap();
    assert_eq!(rebound.status(), 403, "the loopback guard must cover the board routes too");
}

/// `GET /tasks/counts` reports every stage of the effective list, zero-count stages
/// included, for the dashboard.
#[tokio::test]
async fn board_task_counts_cover_every_stage() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    c.post(format!("{base}/tasks")).json(&serde_json::json!({"title": "one"})).send().await.unwrap();
    c.post(format!("{base}/tasks")).json(&serde_json::json!({"title": "two"})).send().await.unwrap();

    let counts: serde_json::Value = c.get(format!("{base}/tasks/counts")).send().await.unwrap().json().await.unwrap();
    let rows = counts.as_array().unwrap();
    assert_eq!(rows.len(), 4, "Backlog, In Progress, Testing, Done: {counts}");
    let backlog = rows.iter().find(|r| r["stage"] == "Backlog").unwrap();
    assert_eq!(backlog["count"], 2, "{counts}");
    let done = rows.iter().find(|r| r["stage"] == "Done").unwrap();
    assert_eq!(done["count"], 0, "a stage with no tasks is still reported: {counts}");
}

/// `top_level=true` on `GET /tasks` keeps only parent-less tasks, and the same flag
/// on `GET /tasks/counts` narrows its per-stage counts to match, so the board's lanes
/// and its side panel counts agree once the desktop app asks for both.
#[tokio::test]
async fn top_level_narrows_the_task_list_and_its_counts_to_parents() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let parent: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice").json(&serde_json::json!({"title": "parent"})).send().await.unwrap().json().await.unwrap();
    let parent_key = parent["key"].as_str().unwrap().to_string();
    let child: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "alice")
        .json(&serde_json::json!({"title": "child", "parent": parent_key}))
        .send().await.unwrap().json().await.unwrap();
    let child_key = child["key"].as_str().unwrap().to_string();

    let all: serde_json::Value = c.get(format!("{base}/tasks")).send().await.unwrap().json().await.unwrap();
    assert_eq!(all.as_array().unwrap().len(), 2, "top_level left unset shows both: {all}");

    let top: serde_json::Value = c.get(format!("{base}/tasks?top_level=true")).send().await.unwrap().json().await.unwrap();
    let top_keys: Vec<&str> = top.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(top_keys, vec![parent_key.as_str()], "{top:?}");

    let subtasks: serde_json::Value = c.get(format!("{base}/tasks?top_level=false")).send().await.unwrap().json().await.unwrap();
    let subtask_keys: Vec<&str> = subtasks.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(subtask_keys, vec![child_key.as_str()], "{subtasks:?}");

    let counts_top: serde_json::Value = c.get(format!("{base}/tasks/counts?top_level=true")).send().await.unwrap().json().await.unwrap();
    let backlog_top = counts_top.as_array().unwrap().iter().find(|r| r["stage"] == "Backlog").unwrap()["count"].as_i64().unwrap();
    assert_eq!(backlog_top, 1, "just the parent: {counts_top}");

    let counts_bare: serde_json::Value = c.get(format!("{base}/tasks/counts")).send().await.unwrap().json().await.unwrap();
    let backlog_bare = counts_bare.as_array().unwrap().iter().find(|r| r["stage"] == "Backlog").unwrap()["count"].as_i64().unwrap();
    assert_eq!(backlog_bare, 2, "top_level left unset counts both: {counts_bare}");
}

/// `TASKS.md` is only planned once `board.mirror_tasks_md` is switched on, the same
/// unauthenticated-daemon-decides pattern the transcript hooks follow: `POST /sync` must
/// not take the mirror flag from the request itself.
#[tokio::test]
async fn tasks_md_mirror_follows_the_board_setting() {
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let sync_check = || c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "check_only": true})).send();
    let rep: serde_json::Value = sync_check().await.unwrap().json().await.unwrap();
    let kinds: Vec<&str> = rep["ops"].as_array().unwrap().iter().filter_map(|o| o["kind"].as_str()).collect();
    assert!(!kinds.contains(&"tasks_md"), "the mirror is off by default: {rep}");

    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({"board.mirror_tasks_md": true})).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let rep: serde_json::Value = sync_check().await.unwrap().json().await.unwrap();
    let op = rep["ops"].as_array().unwrap().iter().find(|o| o["kind"] == "tasks_md");
    assert!(op.is_some(), "enabling the setting should report a TasksMd op: {rep}");
    assert!(!repo.path().join("TASKS.md").exists(), "check_only must not write");
}
