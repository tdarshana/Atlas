//! Projects: connect, sync, access rules, frameworks, extraction overrides and the log.

mod common;
use common::*;
use std::time::Duration;

/// `DELETE /projects/{id}` answers 204 and drops the project from the list. The memories
/// scoped to it stay: nothing is hard-deleted from `memories`.
#[tokio::test]
async fn projects_can_be_deleted_without_losing_their_memories() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());

    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let pid = p["id"].as_str().unwrap().to_string();
    let m = c.post(format!("{base}/memories"))
        .json(&serde_json::json!({"scope":"project","project_id": pid,"kind":"fact","text":"kept after the project goes"}))
        .send().await.unwrap();
    assert_eq!(m.status(), 201);
    let mid = m.json::<serde_json::Value>().await.unwrap()["id"].as_str().unwrap().to_string();

    let gone = c.delete(format!("{base}/projects/{pid}?actor=test")).send().await.unwrap();
    assert_eq!(gone.status(), 204);

    let projects: serde_json::Value = c.get(format!("{base}/projects")).send().await.unwrap().json().await.unwrap();
    assert!(projects.as_array().unwrap().is_empty(), "{projects}");
    assert_eq!(c.get(format!("{base}/projects/{pid}")).send().await.unwrap().status(), 404);
    // A repeat delete is a 404, not a silent success.
    assert_eq!(c.delete(format!("{base}/projects/{pid}")).send().await.unwrap().status(), 404);
    // The memory survives its project.
    let kept: serde_json::Value = c.get(format!("{base}/memories/{mid}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(kept["id"], mid, "{kept}");
    assert_eq!(kept["status"], "active");

    // A malformed id is a 400 from the path extractor, not a 500.
    assert_eq!(c.delete(format!("{base}/projects/not-a-uuid")).send().await.unwrap().status(), 400);
}

#[tokio::test]
async fn projects_agents_docs_and_sync() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());

    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    assert_eq!(p["name"], "fixture");
    assert!(p["profile"]["frameworks"].as_array().unwrap().iter().any(|f| f == "next"), "{p}");
    let pid = p["id"].as_str().unwrap().to_string();

    let projects: serde_json::Value = c.get(format!("{base}/projects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(projects.as_array().unwrap().len(), 1);
    let one: serde_json::Value = c.get(format!("{base}/projects/{pid}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(one["id"], pid);
    let refreshed: serde_json::Value = c.post(format!("{base}/projects/{pid}/refresh")).send().await.unwrap().json().await.unwrap();
    assert_eq!(refreshed["name"], "fixture");

    // The agent routes and the `/personas` alias answer the same rows (ATL-427).
    let a: serde_json::Value = c.post(format!("{base}/agents")).json(&serde_json::json!({"name":"Reviewer","role":"Reviews PRs","instructions":"Be strict.","tags":[]})).send().await.unwrap().json().await.unwrap();
    assert_eq!(a["slug"], "reviewer");
    let agents: serde_json::Value = c.get(format!("{base}/agents")).send().await.unwrap().json().await.unwrap();
    assert_eq!(agents.as_array().unwrap().len(), 1);
    let aliased: serde_json::Value = c.get(format!("{base}/personas")).send().await.unwrap().json().await.unwrap();
    assert_eq!(aliased, agents, "the old path is an alias");
    let roster: serde_json::Value = c.put(format!("{base}/projects/{pid}/agents")).json(&serde_json::json!([{"persona_id": a["id"], "is_default": true, "position": 0}])).send().await.unwrap().json().await.unwrap();
    assert_eq!(roster.as_array().unwrap().len(), 1, "{roster}");

    let r = c.post(format!("{base}/practices")).json(&serde_json::json!({"name":"commits","body":"imperative","tags":[],"project_id": pid})).send().await.unwrap();
    assert_eq!(r.status(), 201);
    // `/workflows` now serves the real, graph-shaped workflow API (Phase 9), not a
    // Markdown document: see the dedicated `workflow_*` tests below for that surface.
    let practices: serde_json::Value = c.get(format!("{base}/practices?project_id={pid}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(practices[0]["name"], "commits");

    c.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"project","project_id": pid,"kind":"decision","text":"fixture deploys to fly.io"})).send().await.unwrap();

    let ctx: serde_json::Value = c.post(format!("{base}/projects/context")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    assert_eq!(ctx["project"]["id"], pid);
    assert_eq!(ctx["practices"][0]["name"], "commits");
    // `ctx["workflows"]` still reads the retired Markdown-document workflow kind
    // (`ProjectContext.workflows: Vec<Doc>`), which the Phase 9 migration empties for
    // good; it is not the new graph-shaped workflow API.
    assert!(ctx["workflows"].as_array().unwrap().is_empty(), "{ctx}");
    assert!(ctx["memories"].as_array().unwrap().iter().any(|h| h["memory"]["text"].as_str().unwrap().contains("fly.io")), "{ctx}");

    let targets = serde_json::json!(["claude", "codex", "agents_md", "claude_md"]);
    let check: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "global": false, "targets": targets, "check_only": true})).send().await.unwrap().json().await.unwrap();
    assert_eq!(check["created"], 3, "{check}");
    assert!(!repo.path().join(".claude/agents/reviewer.md").exists(), "check_only must not write");

    let rep: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "global": false, "targets": targets, "check_only": false})).send().await.unwrap().json().await.unwrap();
    assert_eq!(rep["created"], 3);
    assert!(repo.path().join(".claude/agents/reviewer.md").exists(), "the roster agent is exported as a subagent file");
    assert!(!repo.path().join(".codex/agents").exists(), "Codex gets no per-agent file");
    let agents_md = std::fs::read_to_string(repo.path().join("AGENTS.md")).unwrap();
    assert!(agents_md.contains("## Agent: Reviewer"), "{agents_md}");
    assert!(agents_md.contains("fixture"), "the block names the connected project: {agents_md}");

    let again: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "global": false, "targets": targets, "check_only": false})).send().await.unwrap().json().await.unwrap();
    assert_eq!(again["unchanged"], 3);

    let del = c.delete(format!("{base}/practices/commits")).send().await.unwrap();
    assert_eq!(del.status(), 204);
    assert_eq!(c.get(format!("{base}/practices/commits")).send().await.unwrap().status(), 404);
    // Delete takes the id, not the slug; the roster row goes with it.
    let agent_id = a["id"].as_str().unwrap();
    assert_eq!(c.delete(format!("{base}/agents/{agent_id}")).send().await.unwrap().status(), 204);
    assert_eq!(c.get(format!("{base}/agents/reviewer")).send().await.unwrap().status(), 404);

    let bad = c.post(format!("{base}/agents")).json(&serde_json::json!({"name":"","instructions":"y"})).send().await.unwrap();
    assert_eq!(bad.status(), 400, "an empty name is refused");
    let no_root = c.post(format!("{base}/sync")).json(&serde_json::json!({"global": false, "targets": targets, "check_only": true})).send().await.unwrap();
    assert_eq!(no_root.status(), 400, "a project sync needs a root");
}

/// A global sync must write into the home `ATLAS_SYNC_HOME` names, not the real one, and
/// must write the per-tool files only: home has no project to name in a managed block
/// and no roster, so with extraction on that is the two transcript hooks.
#[tokio::test]
async fn global_sync_honours_the_sync_home_override() {
    let home = tempfile::tempdir().unwrap();
    let d = start_with_env(&[("ATLAS_SYNC_HOME", home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({"extraction.enabled": true})).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let targets = serde_json::json!(["claude", "codex", "claude_hook", "codex_hook", "agents_md", "claude_md"]);
    let rep: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"global": true, "targets": targets, "check_only": false})).send().await.unwrap().json().await.unwrap();
    assert_eq!(rep["created"], 2, "{rep}");
    assert_eq!(rep["skipped"], 2, "the two managed-block targets are reported, not dropped: {rep}");

    assert!(home.path().join(".claude/settings.json").exists(), "no Claude hook under the override home");
    assert!(home.path().join(".codex/config.toml").exists(), "no Codex hook under the override home");
    assert!(!home.path().join(".claude/agents").exists(), "no roster, so no subagent files");
    assert!(!home.path().join("AGENTS.md").exists(), "a global sync must not write AGENTS.md");
    assert!(!home.path().join("CLAUDE.md").exists(), "a global sync must not write CLAUDE.md");

    // `SyncRequest` has no `home` field: only `ATLAS_SYNC_HOME`, set on the daemon
    // process, can redirect a global sync. A `home` key in the request body is an
    // unknown field that serde silently ignores, so it must not steer the write.
    let elsewhere = tempfile::tempdir().unwrap();
    let rep2: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({
        "global": true, "home": elsewhere.path(), "targets": targets, "check_only": false,
    })).send().await.unwrap().json().await.unwrap();
    assert_eq!(rep2["created"], 0, "{rep2}");
    assert_eq!(rep2["unchanged"], 2, "a second sync still lands in the ATLAS_SYNC_HOME override: {rep2}");
    assert!(!elsewhere.path().join(".claude/settings.json").exists(), "a `home` field in the request body must not redirect the sync");
    assert!(!elsewhere.path().join(".codex/config.toml").exists(), "a `home` field in the request body must not redirect the sync");
}

/// The transcript hooks are installed only once extraction is switched on, and the
/// daemon decides that from its own settings: `POST /sync` is unauthenticated, so the
/// request must not be able to ask for a hook the user never enabled.
#[tokio::test]
async fn transcript_hooks_follow_the_extraction_setting() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let d = start_with_env(&[("ATLAS_SYNC_HOME", home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    // A project hook is per-machine, so it goes in the settings file Claude Code keeps
    // out of git, not the shared one a `git commit -a` would ship to the whole team.
    let claude_hook = repo.path().join(".claude/settings.local.json");
    let shared_settings = repo.path().join(".claude/settings.json");
    let codex_hook = home.path().join(".codex/config.toml");

    let sync = || c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path()})).send();
    let rep: serde_json::Value = sync().await.unwrap().json().await.unwrap();
    assert!(!claude_hook.exists(), "extraction is off by default, so no Stop hook: {rep}");
    assert!(!codex_hook.exists(), "extraction is off by default, so no notify: {rep}");

    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({"extraction.enabled": true})).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let rep: serde_json::Value = sync().await.unwrap().json().await.unwrap();
    assert!(claude_hook.exists(), "enabling extraction should install the Stop hook: {rep}");
    assert!(codex_hook.exists(), "enabling extraction should install the Codex notify: {rep}");
    assert!(!shared_settings.exists(), "the hook must not land in the repository's shared, committed settings file: {rep}");
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&claude_hook).unwrap()).unwrap();
    assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "atlas ingest --tool claude-code --hook-stdin");
    assert!(std::fs::read_to_string(&codex_hook).unwrap().contains(r#"notify = ["atlas", "ingest", "--tool", "codex", "--hook-arg"]"#));

    // `--check` reports the hooks like any other op once they are in place.
    let rep: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "check_only": true})).send().await.unwrap().json().await.unwrap();
    let kinds: Vec<&str> = rep["ops"].as_array().unwrap().iter().filter_map(|o| o["kind"].as_str()).collect();
    assert!(kinds.contains(&"claude_hook") && kinds.contains(&"codex_hook"), "check should report the hook ops: {rep}");
    assert_eq!(rep["created"].as_u64().unwrap() + rep["updated"].as_u64().unwrap(), 0, "a second pass has nothing to do: {rep}");
}

/// A project sync must refuse the `ATLAS_SYNC_HOME` override too, not just the real home
/// directory, or a caller could set a project root there and have it treated as a project.
#[tokio::test]
async fn project_sync_refuses_the_sync_home_override_too() {
    let sync_home = tempfile::tempdir().unwrap();
    fixture_repo(sync_home.path());
    let d = start_with_env(&[("ATLAS_SYNC_HOME", sync_home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let refused = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": sync_home.path(), "check_only": true})).send().await.unwrap();
    assert_eq!(refused.status(), 400, "the ATLAS_SYNC_HOME override must not double as a project root");
    let body: serde_json::Value = refused.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("not a project root"), "{body}");
}

/// `POST /sync` writes files and has no authentication, so it must refuse a root that is
/// not a git repository, the filesystem root included.
#[tokio::test]
async fn sync_refuses_a_root_that_is_not_a_repository() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let slash = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": "/", "check_only": true})).send().await.unwrap();
    assert_eq!(slash.status(), 400, "the filesystem root is not a project root");

    let plain = repo_free_tempdir();
    let not_git = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": plain.path(), "check_only": true})).send().await.unwrap();
    assert_eq!(not_git.status(), 400, "a directory with no git repository is not a sync target");
    let body: serde_json::Value = not_git.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("git repository"), "{body}");

    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let ok = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "check_only": true})).send().await.unwrap();
    assert_eq!(ok.status(), 200, "a real repository still syncs");
}

/// `POST /projects/{id}/refresh` enqueues a `project_summary` job when extraction is
/// enabled, and the worker writes the model's reply into `profile.summary`, visible
/// through `GET /projects/{id}` once the job finishes.
#[tokio::test]
async fn refresh_project_enqueues_a_summary_job_when_extraction_is_enabled() {
    let stub = stub_llm_with_reply("A Rust workspace.").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());

    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let pid = p["id"].as_str().unwrap().to_string();

    let refreshed: serde_json::Value = c.post(format!("{base}/projects/{pid}/refresh")).send().await.unwrap().json().await.unwrap();
    assert!(refreshed["profile"]["summary"].is_null(), "the summary is written by the worker, not synchronously: {refreshed}");

    let mut summary = None;
    for _ in 0..100 {
        let project: serde_json::Value = c.get(format!("{base}/projects/{pid}")).send().await.unwrap().json().await.unwrap();
        if let Some(s) = project["profile"]["summary"].as_str() {
            summary = Some(s.to_string());
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(summary.as_deref(), Some("A Rust workspace."), "the project summary was not written within 10s");
}

// ---- Phase 8: the project hub ----

/// `PATCH /projects/{id}` renames the project, its board key and every task key on
/// its board, and refuses a malformed key with a 400 and an unknown project with a 404.
#[tokio::test]
async fn project_patch_renames_the_board_key_and_validates_it() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    let task: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"project_id": id, "title": "renamed with the board"})).send().await.unwrap().json().await.unwrap();
    assert!(task["key"].as_str().unwrap().ends_with("-1"), "{task}");

    let patched = c.patch(format!("{base}/projects/{id}")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"name": "Renamed", "board_key": "zed", "git_remote": null})).send().await.unwrap();
    assert_eq!(patched.status(), 200);
    let patched: serde_json::Value = patched.json().await.unwrap();
    assert_eq!(patched["name"], "Renamed");
    assert_eq!(patched["board_key"], "ZED");
    assert!(patched["git_remote"].is_null(), "an explicit null clears the remote: {patched}");

    let moved: serde_json::Value = c.get(format!("{base}/tasks/ZED-1")).send().await.unwrap().json().await.unwrap();
    assert_eq!(moved["task"]["title"], "renamed with the board");

    let bad = c.patch(format!("{base}/projects/{id}")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"board_key": "a-b"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let body: serde_json::Value = bad.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("board key"), "{body}");

    let missing = c.patch(format!("{base}/projects/{}", uuid::Uuid::new_v4())).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"name": "nope"})).send().await.unwrap();
    assert_eq!(missing.status(), 404);
}

/// `PUT /projects/{id}/agent-access` stores the rules and the daemon then enforces
/// them: a tool that is not on `task_movers` gets a 409, the desktop never does.
#[tokio::test]
async fn agent_access_is_stored_and_enforced() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    assert!(project["agent_access"]["memory_writers"].is_null(), "a fresh project admits anyone: {project}");

    let saved = c.put(format!("{base}/projects/{id}/agent-access")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"memory_writers": ["claude-code"], "task_movers": ["claude-code"], "require_review": true}))
        .send().await.unwrap();
    assert_eq!(saved.status(), 200);
    let saved: serde_json::Value = saved.json().await.unwrap();
    assert_eq!(saved["agent_access"]["task_movers"][0], "claude-code");
    assert_eq!(saved["agent_access"]["require_review"], true);

    let task: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"project_id": id, "title": "guarded"})).send().await.unwrap().json().await.unwrap();
    let key = task["key"].as_str().unwrap().to_string();

    let refused = c.post(format!("{base}/tasks/{key}/move")).header("X-Atlas-Actor", "codex")
        .json(&serde_json::json!({"stage": "In Progress"})).send().await.unwrap();
    assert_eq!(refused.status(), 409);
    let refused: serde_json::Value = refused.json().await.unwrap();
    let name = project["name"].as_str().unwrap();
    assert_eq!(refused["error"], format!("actor 'codex' may not move tasks in project {name}"), "{refused}");

    let allowed = c.post(format!("{base}/tasks/{key}/move")).header("X-Atlas-Actor", "claude-code/reviewer")
        .json(&serde_json::json!({"stage": "In Progress"})).send().await.unwrap();
    assert_eq!(allowed.status(), 200);

    // `require_review` holds an agent's memory back; the desktop's goes straight in.
    let held = c.post(format!("{base}/memories?actor=codex")).json(&serde_json::json!({
        "scope": "project", "project_id": id, "kind": "fact", "text": "held for review"
    })).send().await.unwrap();
    assert_eq!(held.status(), 409, "codex is not on memory_writers either");
    let held: serde_json::Value = held.json().await.unwrap();
    assert_eq!(held["error"], format!("actor 'codex' may not write memories in project {name}"), "{held}");
    let reviewed: serde_json::Value = c.post(format!("{base}/memories?actor=claude-code")).json(&serde_json::json!({
        "scope": "project", "project_id": id, "kind": "fact", "text": "held for review"
    })).send().await.unwrap().json().await.unwrap();
    assert_eq!(reviewed["status"], "pending", "{reviewed}");

    let missing = c.put(format!("{base}/projects/{}/agent-access", uuid::Uuid::new_v4())).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"require_review": false})).send().await.unwrap();
    assert_eq!(missing.status(), 404);
}

/// The three frameworks routes (Phase 12): the inventory-plus-documents listing, a
/// document's text by path, and importing tasks or decisions — including the 400 for
/// an unknown `{kind}` and the 409 an agent not on `agent_access` gets from an import.
#[tokio::test]
async fn frameworks_routes_round_trip_400_and_gate_import() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    git(dir.path(), &["init"]);
    std::fs::create_dir_all(dir.path().join("docs/superpowers/plans")).unwrap();
    std::fs::write(
        dir.path().join("docs/superpowers/plans/2026-01-01-fixture.md"),
        "# Fixture plan\n\n### Task 1: Do the thing\n\n- [ ] do it\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "# Fixture rules\n\nkeep this line\n").unwrap();

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();

    let listing = c.get(format!("{base}/projects/{id}/frameworks")).send().await.unwrap();
    assert_eq!(listing.status(), 200);
    let listing: serde_json::Value = listing.json().await.unwrap();
    assert_eq!(listing[0]["inventory"]["kind"], "superpowers", "{listing}");
    let path = listing[0]["documents"][0]["path"].as_str().unwrap().to_string();
    assert_eq!(path, "docs/superpowers/plans/2026-01-01-fixture.md");

    let doc = c.get(format!("{base}/projects/{id}/frameworks/superpowers/docs/{path}")).send().await.unwrap();
    assert_eq!(doc.status(), 200);
    let doc: serde_json::Value = doc.json().await.unwrap();
    assert_eq!(doc["content"], "# Fixture plan\n\n### Task 1: Do the thing\n\n- [ ] do it\n");

    let bad_kind = c.get(format!("{base}/projects/{id}/frameworks/bogus/docs/{path}")).send().await.unwrap();
    assert_eq!(bad_kind.status(), 400, "{}", bad_kind.text().await.unwrap());
    let bad_import = c.post(format!("{base}/projects/{id}/frameworks/bogus/import")).json(&serde_json::json!({"what": "tasks"})).send().await.unwrap();
    assert_eq!(bad_import.status(), 400);

    // Lock the project down, then an agent not on `task_movers`/`memory_writers` is
    // refused for both kinds of import; the user's own hands (no header default `api`)
    // are exempt regardless.
    let locked = c.put(format!("{base}/projects/{id}/agent-access")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"memory_writers": [], "task_movers": [], "require_review": false}))
        .send().await.unwrap();
    assert_eq!(locked.status(), 200);

    let denied_tasks = c.post(format!("{base}/projects/{id}/frameworks/superpowers/import")).header("X-Atlas-Actor", "codex")
        .json(&serde_json::json!({"what": "tasks"})).send().await.unwrap();
    assert_eq!(denied_tasks.status(), 409);
    let denied_tasks: serde_json::Value = denied_tasks.json().await.unwrap();
    assert!(denied_tasks["error"].as_str().unwrap().contains("may not move tasks"), "{denied_tasks}");

    let denied_decisions = c.post(format!("{base}/projects/{id}/frameworks/superpowers/import")).header("X-Atlas-Actor", "codex")
        .json(&serde_json::json!({"what": "decisions"})).send().await.unwrap();
    assert_eq!(denied_decisions.status(), 409);
    let denied_decisions: serde_json::Value = denied_decisions.json().await.unwrap();
    assert!(denied_decisions["error"].as_str().unwrap().contains("may not write memories"), "{denied_decisions}");

    // No `X-Atlas-Actor` header defaults to `api`, one of the exempt actors, so the
    // same locked-down project still admits the import.
    let imported = c.post(format!("{base}/projects/{id}/frameworks/superpowers/import"))
        .json(&serde_json::json!({"what": "tasks"})).send().await.unwrap();
    assert_eq!(imported.status(), 200, "{}", imported.text().await.unwrap());
    let imported: serde_json::Value = imported.json().await.unwrap();
    assert_eq!(imported["created"], 2, "the parent task and its one subtask: {imported}");

    // Importing again is idempotent: nothing new is created.
    let reimported: serde_json::Value = c.post(format!("{base}/projects/{id}/frameworks/superpowers/import"))
        .json(&serde_json::json!({"what": "tasks"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(reimported["created"], 0, "{reimported}");
    assert_eq!(reimported["skipped"], 2, "{reimported}");
}

/// `PUT /projects/{id}/extraction` masks the key on the way back, and
/// `POST /extraction/test?project_id=` reaches the project's own endpoint rather
/// than the global one.
#[tokio::test]
async fn project_extraction_override_masks_its_key_and_is_used_by_the_test_route() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());
    let project_llm = stub_llm_with_reply("PROJECT").await;

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    assert!(project["extraction"].is_null(), "no override by default: {project}");

    // The global settings stay off, so only the project's own switch can turn it on.
    let saved: serde_json::Value = c.put(format!("{base}/projects/{id}/extraction")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"enabled": true, "base_url": project_llm, "model": "project-model", "api_key": "sk-project"}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(saved["extraction"]["api_key"], "***", "{saved}");
    assert_eq!(saved["extraction"]["model"], "project-model");

    let global = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(global.status(), 409, "the global settings are still off");

    let scoped = c.post(format!("{base}/extraction/test?project_id={id}")).send().await.unwrap();
    assert_eq!(scoped.status(), 200);
    let scoped: serde_json::Value = scoped.json().await.unwrap();
    assert_eq!(scoped["reply"], "PROJECT", "{scoped}");

    // `null` clears the override and puts the project back on the global settings.
    let cleared: serde_json::Value = c.put(format!("{base}/projects/{id}/extraction")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::Value::Null).send().await.unwrap().json().await.unwrap();
    assert!(cleared["extraction"].is_null(), "{cleared}");
    assert_eq!(c.post(format!("{base}/extraction/test?project_id={id}")).send().await.unwrap().status(), 409);

    let missing = c.put(format!("{base}/projects/{}/extraction", uuid::Uuid::new_v4())).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::Value::Null).send().await.unwrap();
    assert_eq!(missing.status(), 404);
}

/// `GET /projects/{id}/log` merges the sources and honours its filters, and
/// `/log/export` answers the same entries as JSON lines.
#[tokio::test]
async fn project_log_merges_sources_and_exports_json_lines() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();
    let task: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"project_id": id, "title": "logged"})).send().await.unwrap().json().await.unwrap();
    let key = task["key"].as_str().unwrap().to_string();
    c.post(format!("{base}/tasks/{key}/comment")).header("X-Atlas-Actor", "claude-code")
        .json(&serde_json::json!({"body": "a note about the deploy"})).send().await.unwrap();
    c.post(format!("{base}/memories?actor=desktop")).json(&serde_json::json!({
        "scope": "project", "project_id": id, "kind": "fact", "text": "the deploy target is fly.io"
    })).send().await.unwrap();

    // A real sync leaves the `sync` audit row the log reads as its `synced` entry.
    let synced = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap();
    assert_eq!(synced.status(), 200);

    let log = c.get(format!("{base}/projects/{id}/log")).send().await.unwrap();
    assert_eq!(log.status(), 200);
    let log: serde_json::Value = log.json().await.unwrap();
    let entries = log.as_array().unwrap();
    let kinds: Vec<&str> = entries.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    for want in ["connected", "created", "commented", "remembered", "synced"] {
        assert!(kinds.contains(&want), "missing {want} in {kinds:?}");
    }
    let commented = entries.iter().find(|e| e["kind"] == "commented").unwrap();
    assert_eq!(commented["source"], "claude-code");
    assert_eq!(commented["ref"]["type"], "task");
    assert_eq!(commented["ref"]["key"], key);

    let by_source: serde_json::Value = c.get(format!("{base}/projects/{id}/log?source=claude-code")).send().await.unwrap().json().await.unwrap();
    assert!(by_source.as_array().unwrap().iter().all(|e| e["source"] == "claude-code"), "{by_source}");
    let by_kind: serde_json::Value = c.get(format!("{base}/projects/{id}/log?kind=remembered&q=fly.io")).send().await.unwrap().json().await.unwrap();
    assert_eq!(by_kind.as_array().unwrap().len(), 1, "{by_kind}");
    let one: serde_json::Value = c.get(format!("{base}/projects/{id}/log?limit=1")).send().await.unwrap().json().await.unwrap();
    assert_eq!(one.as_array().unwrap().len(), 1);

    let export = c.get(format!("{base}/projects/{id}/log/export")).send().await.unwrap();
    assert_eq!(export.status(), 200);
    assert_eq!(export.headers().get("content-type").unwrap(), "application/x-ndjson");
    let text = export.text().await.unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), entries.len());
    for line in lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v["time"].is_string(), "{line}");
    }

    let missing = c.get(format!("{base}/projects/{}/log", uuid::Uuid::new_v4())).send().await.unwrap();
    assert_eq!(missing.status(), 404);
    // A bad `after` is a 400 naming the parameter, not a silently dropped filter.
    assert_eq!(c.get(format!("{base}/projects/{id}/log?after=yesterday")).send().await.unwrap().status(), 400);
}

/// The global `access.*` defaults fill in a project's unset `agent_access` fields; the
/// project's own value, once set, wins over the default outright. `require_review` is a
/// floor: a project that never set its own flag still lands an agent's memory `pending`
/// once the global default turns it on. `GET /projects/{id}/access` reports the
/// project's own rule, the global defaults and the two resolved together.
#[tokio::test]
async fn agent_access_defaults_are_inherited_and_overridable_per_project() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();

    let put_settings = |body: serde_json::Value| c.put(format!("{base}/settings")).header("X-Atlas-Actor", "desktop").json(&body).send();
    let remember = |actor: &str, text: &str| {
        let (c, base, id, actor, text) = (c.clone(), base.clone(), id.clone(), actor.to_string(), text.to_string());
        async move {
            c.post(format!("{base}/memories?actor={actor}"))
                .json(&serde_json::json!({"scope": "project", "project_id": id, "kind": "fact", "text": text}))
                .send().await.unwrap()
        }
    };

    // The project's own `memory_writers` is unset; the global default admits only
    // `claude-code`.
    let set = put_settings(serde_json::json!({"access.memory_writers": ["claude-code"]})).await.unwrap();
    assert_eq!(set.status(), 200, "{}", set.text().await.unwrap());

    let refused = remember("codex", "codex writes under the default").await;
    assert_eq!(refused.status(), 409, "{}", refused.text().await.unwrap());
    let accepted = remember("claude-code", "claude-code writes under the default").await;
    assert_eq!(accepted.status(), 201, "{}", accepted.text().await.unwrap());

    // The project sets its own list, which wins over the default outright, flipping
    // both actors.
    let put_access = c.put(format!("{base}/projects/{id}/agent-access")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"memory_writers": ["codex"]})).send().await.unwrap();
    assert_eq!(put_access.status(), 200, "{}", put_access.text().await.unwrap());

    let now_accepted = remember("codex", "codex writes once the project admits it").await;
    assert_eq!(now_accepted.status(), 201, "{}", now_accepted.text().await.unwrap());
    let now_refused = remember("claude-code", "claude-code is refused once the project narrows to codex").await;
    assert_eq!(now_refused.status(), 409, "{}", now_refused.text().await.unwrap());

    // `GET /projects/{id}/access` shows all three shapes.
    let access: serde_json::Value = c.get(format!("{base}/projects/{id}/access")).send().await.unwrap().json().await.unwrap();
    assert_eq!(access["access"]["memory_writers"], serde_json::json!(["codex"]), "{access}");
    assert_eq!(access["defaults"]["memory_writers"], serde_json::json!(["claude-code"]), "{access}");
    assert_eq!(access["effective"]["memory_writers"], serde_json::json!(["codex"]), "{access}");

    // `access.require_review` is a floor: this project never set its own flag, and
    // still lands its agent's memory `pending` once the global default turns it on.
    let review_on = put_settings(serde_json::json!({"access.require_review": true})).await.unwrap();
    assert_eq!(review_on.status(), 200, "{}", review_on.text().await.unwrap());
    let pending = remember("codex", "codex's memory lands pending under the global floor").await;
    assert_eq!(pending.status(), 201, "{}", pending.text().await.unwrap());
    let pending_body: serde_json::Value = pending.json().await.unwrap();
    assert_eq!(pending_body["status"], "pending", "{pending_body}");
}
