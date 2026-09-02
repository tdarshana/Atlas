use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Daemon { child: Child, port: u16, _home: tempfile::TempDir }
impl Drop for Daemon { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

fn free_port() -> u16 { std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port() }

async fn start() -> Daemon {
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_atlasd"))
        .args(["--port", &port.to_string(), "--home", home.path().to_str().unwrap(), "--no-embed"])
        .stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if client.get(format!("http://127.0.0.1:{port}/api/v1/status")).send().await.is_ok() { break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Daemon { child, port, _home: home }
}

/// Builds a small git-backed fixture project: a Next.js/React `package.json`, a README,
/// a TypeScript source file, a gitignored `node_modules` dir, one commit, and an `origin`
/// remote. Copied from `atlas-core/tests/common/mod.rs`, which an integration test in
/// another crate cannot reach.
fn fixture_repo(dir: &Path) {
    git(dir, &["init"]);
    std::fs::write(dir.join("package.json"), r#"{"name":"fixture","dependencies":{"next":"16.0.0","react":"19.0.0"}}"#).unwrap();
    std::fs::write(dir.join("README.md"), "# fixture\n\nA test fixture.\nSecond line.\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/index.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();
    std::fs::create_dir_all(dir.join("node_modules")).unwrap();
    std::fs::write(dir.join("node_modules/x.js"), "// ignored\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-m", "initial fixture"]);
    git(dir, &["remote", "add", "origin", "https://github.com/example/fixture.git"]);
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Atlas Test").env("GIT_AUTHOR_EMAIL", "atlas-test@example.com")
        .env("GIT_COMMITTER_NAME", "Atlas Test").env("GIT_COMMITTER_EMAIL", "atlas-test@example.com")
        .stdout(Stdio::null()).stderr(Stdio::null()).status()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed");
}

#[tokio::test]
async fn json_api_round_trip() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

    let bad_create = c.post(format!("{base}/memories")).header("Content-Type", "application/json").body("{\"scope\":\"global\"").send().await.unwrap();
    assert_eq!(bad_create.status(), 400);
    let bad_create_body: serde_json::Value = bad_create.json().await.unwrap();
    assert!(bad_create_body["error"].as_str().is_some(), "{bad_create_body}");

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

    let daemon_json = std::fs::read_to_string(d._home.path().join("daemon.json")).unwrap();
    assert!(daemon_json.contains(&format!("\"port\":{}", d.port)) || daemon_json.contains(&format!("\"port\": {}", d.port)));
}

/// One JSON-RPC round trip over streamable HTTP. The reply may come back as a bare JSON
/// body or as an SSE stream, so the raw text is returned and callers assert against it.
async fn rpc(c: &reqwest::Client, url: &str, session: &Option<String>, body: serde_json::Value) -> String {
    let mut req = c.post(url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json");
    if let Some(s) = session { req = req.header("mcp-session-id", s); }
    req.json(&body).send().await.unwrap().text().await.unwrap()
}

#[tokio::test]
async fn mcp_over_http_lists_and_calls_tools() {
    let d = start().await;
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let api = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    assert!(init.status().is_success(), "initialize failed: {}", init.status());
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    for t in ["remember", "recall", "forget", "status", "project_context", "connect_project", "list_agents",
              "get_agent", "save_agent", "list_practices", "get_practice", "list_workflows", "get_workflow"] {
        assert!(body.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {body}");
    }

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remember","arguments":{"text":"mcp round trip works","kind":"insight"}}})).await;
    assert!(body.contains("mcp round trip works"), "{body}");

    // Agents reach MCP through the same store the JSON API writes, so save one there and
    // expect it to show up as a resource and as a prompt.
    let saved = c.post(format!("{api}/agents")).json(&serde_json::json!({
        "name": "reviewer", "description": "Reviews diffs for regressions",
        "instructions": "Read the diff and name the riskiest change.", "tools": ["Read"], "tags": ["review"],
    })).send().await.unwrap();
    assert_eq!(saved.status(), 201, "save_agent failed");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":4,"method":"resources/list"})).await;
    assert!(body.contains("atlas://agents/reviewer"), "resources/list missing the agent: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"atlas://agents/reviewer"}})).await;
    assert!(body.contains("name: reviewer"), "resources/read did not return the Claude export: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":6,"method":"prompts/list"})).await;
    assert!(body.contains("\"name\":\"reviewer\""), "prompts/list missing the agent: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":7,"method":"prompts/get","params":{"name":"reviewer"}})).await;
    assert!(body.contains("Adopt the following agent role"), "prompts/get lost the preamble: {body}");
    assert!(body.contains("name the riskiest change"), "prompts/get lost the instructions: {body}");

    // project_context takes an explicit root, which is how an MCP client that cannot set
    // ATLAS_PROJECT_ROOT still scopes its session to a project.
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
        "params":{"name":"project_context","arguments":{"project_root": repo.path().to_str().unwrap()}}})).await;
    // The tool result is JSON inside a JSON string, so its quoting is escaped: match on
    // the key and value text rather than on quoted JSON.
    assert!(body.contains("frameworks"), "project_context returned no profile: {body}");
    assert!(body.contains("nextjs") || body.contains("next"), "project_context profile missing the detected framework: {body}");
}

/// The daemon has no authentication, so a page open in the user's browser must not be able
/// to reach it, on the JSON API or on /mcp.
#[tokio::test]
async fn rejects_browser_origins_and_non_loopback_hosts() {
    let d = start().await;
    let status = format!("http://127.0.0.1:{}/api/v1/status", d.port);
    let c = reqwest::Client::new();

    let cross = c.get(&status).header("Origin", "https://evil.example").send().await.unwrap();
    assert_eq!(cross.status(), 403);
    let body: serde_json::Value = cross.json().await.unwrap();
    assert_eq!(body["error"], "forbidden origin");

    let rebound = c.get(&status).header("Host", "evil.example").send().await.unwrap();
    assert_eq!(rebound.status(), 403);

    let mcp = c.post(format!("http://127.0.0.1:{}/mcp", d.port)).header("Origin", "https://evil.example")
        .header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    assert_eq!(mcp.status(), 403, "the guard must cover /mcp too");

    for origin in ["http://127.0.0.1", &format!("http://localhost:{}", d.port)] {
        assert!(c.get(&status).header("Origin", origin).send().await.unwrap().status().is_success(), "{origin} should be allowed");
    }
}

#[tokio::test]
async fn projects_agents_docs_and_sync() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

    let a: serde_json::Value = c.post(format!("{base}/agents")).json(&serde_json::json!({"name":"reviewer","description":"Reviews PRs","instructions":"Be strict.","tools":["Read"],"tags":[]})).send().await.unwrap().json().await.unwrap();
    assert_eq!(a["version"], 1);
    let agents: serde_json::Value = c.get(format!("{base}/agents")).send().await.unwrap().json().await.unwrap();
    assert_eq!(agents.as_array().unwrap().len(), 1);

    let r = c.post(format!("{base}/practices")).json(&serde_json::json!({"name":"commits","body":"imperative","tags":[],"project_id": pid})).send().await.unwrap();
    assert_eq!(r.status(), 201);
    let w = c.post(format!("{base}/workflows")).json(&serde_json::json!({"name":"release","body":"tag then publish","tags":[]})).send().await.unwrap();
    assert_eq!(w.status(), 201);
    let practices: serde_json::Value = c.get(format!("{base}/practices?project_id={pid}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(practices[0]["name"], "commits");

    c.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"project","project_id": pid,"kind":"decision","text":"fixture deploys to fly.io"})).send().await.unwrap();

    let ctx: serde_json::Value = c.post(format!("{base}/projects/context")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    assert_eq!(ctx["project"]["id"], pid);
    assert_eq!(ctx["practices"][0]["name"], "commits");
    assert_eq!(ctx["workflows"][0]["name"], "release");
    assert!(ctx["memories"].as_array().unwrap().iter().any(|h| h["memory"]["text"].as_str().unwrap().contains("fly.io")), "{ctx}");

    let targets = serde_json::json!(["claude", "codex", "agents_md", "claude_md"]);
    let check: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "global": false, "targets": targets, "check_only": true})).send().await.unwrap().json().await.unwrap();
    assert_eq!(check["created"], 4, "{check}");
    assert!(!repo.path().join(".claude/agents/reviewer.md").exists(), "check_only must not write");

    let rep: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "global": false, "targets": targets, "check_only": false})).send().await.unwrap().json().await.unwrap();
    assert_eq!(rep["created"], 4);
    assert!(repo.path().join(".claude/agents/reviewer.md").exists() && repo.path().join(".codex/agents/reviewer.toml").exists());
    let agents_md = std::fs::read_to_string(repo.path().join("AGENTS.md")).unwrap();
    assert!(agents_md.contains("- `reviewer`: Reviews PRs"), "{agents_md}");
    assert!(agents_md.contains("fixture"), "the block names the connected project: {agents_md}");

    let again: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"root": repo.path(), "global": false, "targets": targets, "check_only": false})).send().await.unwrap().json().await.unwrap();
    assert_eq!(again["unchanged"], 4);

    let del = c.delete(format!("{base}/practices/commits")).send().await.unwrap();
    assert_eq!(del.status(), 204);
    assert_eq!(c.get(format!("{base}/practices/commits")).send().await.unwrap().status(), 404);
    assert_eq!(c.delete(format!("{base}/agents/reviewer")).send().await.unwrap().status(), 204);

    let bad = c.post(format!("{base}/agents")).json(&serde_json::json!({"name":"Bad Name","description":"x","instructions":"y"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let no_root = c.post(format!("{base}/sync")).json(&serde_json::json!({"global": false, "targets": targets, "check_only": true})).send().await.unwrap();
    assert_eq!(no_root.status(), 400, "a project sync needs a root");
}

/// Memories captured for review land as `pending`: invisible to recall until a status
/// change accepts them, at which point they become recallable.
#[tokio::test]
async fn pending_memories_can_be_listed_and_accepted() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

/// Every 400 the API returns carries the `{"error": string}` body, query strings included:
/// axum's own rejections are plain text, so the extractors have to own the mapping.
#[tokio::test]
async fn bad_query_strings_are_json_errors() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

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
