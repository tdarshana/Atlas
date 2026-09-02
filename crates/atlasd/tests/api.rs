use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Daemon { child: Child, port: u16, _home: tempfile::TempDir }
impl Drop for Daemon { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

async fn start() -> Daemon { start_with_env(&[]).await }

/// A daemon with extra environment variables, for the settings the daemon reads at
/// call time rather than from its arguments (`ATLAS_SYNC_HOME`).
///
/// Every test in the file starts its own daemon and they run at once, so each is asked
/// to bind an ephemeral port (`--port 0`) rather than a port picked in the test process
/// and handed over: with nine tests racing, two could otherwise be handed the same
/// number and one daemon would fail to bind it. The real port is read back from
/// `daemon.json`, which the daemon writes only once it holds the port.
async fn start_with_env(env: &[(&str, &str)]) -> Daemon {
    let home = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_atlasd"))
        .args(["--port", "0", "--home", home.path().to_str().unwrap(), "--no-embed"])
        .envs(env.iter().copied())
        .stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
    let daemon_json = home.path().join("daemon.json");
    // The wait has to cover a slow start under load, same budget as the readiness poll
    // below. A daemon that never writes the file fails here, where the reason is plain,
    // rather than as a "port unknown" error further down.
    let mut port = None;
    for _ in 0..200 {
        if let Ok(s) = std::fs::read_to_string(&daemon_json) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(p) = v["port"].as_u64() { port = Some(p as u16); break; }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let port = port.unwrap_or_else(|| panic!("daemon.json had no port within 20s"));
    let client = reqwest::Client::new();
    // A daemon that never answers fails here, where the reason is plain, rather than as
    // a connection error inside the test body.
    let mut up = false;
    for _ in 0..200 {
        if client.get(format!("http://127.0.0.1:{port}/api/v1/status")).send().await.is_ok() { up = true; break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(up, "atlasd did not answer on port {port} within 20s");
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

/// A `tempfile::tempdir()` that is not itself inside a git repository. `TMPDIR` can sit
/// inside a repo (a checkout, a build sandbox), which would let `detect_root`'s upward
/// search find that enclosing repository instead of the fallback a test expects.
fn repo_free_tempdir() -> tempfile::TempDir {
    if git2::Repository::discover(std::env::temp_dir()).is_err() {
        return tempfile::tempdir().unwrap();
    }
    if git2::Repository::discover("/tmp").is_err() {
        return tempfile::Builder::new().prefix("atlas-").tempdir_in("/tmp").unwrap();
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).expect("HOME not set");
    let fallback = home.join(".atlas/tmp");
    std::fs::create_dir_all(&fallback).unwrap();
    tempfile::Builder::new().prefix("atlas-").tempdir_in(&fallback).unwrap()
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

/// The JSON-RPC response inside an `rpc` reply, whether it arrived bare or as SSE frames.
fn rpc_json(body: &str) -> serde_json::Value {
    if let Ok(v) = serde_json::from_str(body) { return v; }
    let frame = body.lines().filter_map(|l| l.strip_prefix("data: "))
        .find_map(|d| serde_json::from_str::<serde_json::Value>(d).ok().filter(|v| v.get("result").is_some() || v.get("error").is_some()));
    frame.unwrap_or_else(|| panic!("no JSON-RPC result in reply: {body}"))
}

/// The text a `tools/call` returned, parsed back into JSON. Tool results carry their
/// payload as a JSON *string*, so it takes two parses to reach the data.
fn tool_json(body: &str) -> serde_json::Value {
    let reply = rpc_json(body);
    let text = reply["result"]["content"][0]["text"].as_str()
        .unwrap_or_else(|| panic!("tool result had no text content: {reply}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("tool result text was not JSON ({e}): {text}"))
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
    let ctx = tool_json(&body);
    let frameworks = ctx["project"]["profile"]["frameworks"].as_array()
        .unwrap_or_else(|| panic!("project_context returned no profile: {ctx}"));
    assert!(frameworks.iter().any(|f| f == "next"), "profile missing the framework from package.json: {frameworks:?}");
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

/// The Tauri desktop app's webview sends one of these fixed origins depending on
/// platform (WKWebView/wry: `tauri://localhost`; WebView2: `http://tauri.localhost`),
/// plus its dev server origin `http://localhost:1420`. None of them may be rejected as
/// a browser origin.
#[tokio::test]
async fn accepts_tauri_webview_origins() {
    let d = start().await;
    let status = format!("http://127.0.0.1:{}/api/v1/status", d.port);
    let c = reqwest::Client::new();
    for origin in ["tauri://localhost", "http://tauri.localhost", "http://localhost:1420"] {
        let r = c.get(&status).header("Origin", origin).send().await.unwrap();
        assert_eq!(r.status(), 200, "{origin} should be allowed");
    }
}

/// The daemon never authenticates, so a browser or the Tauri webview only gets to read the
/// response if `Access-Control-Allow-Origin` echoes an allowed origin. The CORS allow list is
/// narrower than the request guard: a page on another loopback port has its request run (200)
/// but gets no allow-origin header, so the browser will not hand it the body; a non-loopback
/// origin never gets past the guard at all.
#[tokio::test]
async fn cors_headers_cover_allowed_and_reject_other_origins() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let ok = c.get(format!("{base}/status")).header("Origin", "http://localhost:1420").send().await.unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.headers().get("access-control-allow-origin").unwrap(), "http://localhost:1420");
    let vary = ok.headers().get("vary").unwrap_or_else(|| panic!("no vary header")).to_str().unwrap().to_ascii_lowercase();
    assert!(vary.contains("origin"), "{vary}");

    let preflight = c.request(reqwest::Method::OPTIONS, format!("{base}/memories/search"))
        .header("Origin", "tauri://localhost")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type")
        .send().await.unwrap();
    assert!(preflight.status().is_success(), "{}", preflight.status());
    assert_eq!(preflight.headers().get("access-control-allow-origin").unwrap(), "tauri://localhost");
    let allow_headers = preflight.headers().get("access-control-allow-headers").unwrap_or_else(|| panic!("no access-control-allow-headers")).to_str().unwrap().to_ascii_lowercase();
    assert!(allow_headers.contains("content-type"), "{allow_headers}");

    // A local page on another port is allowed through the guard, but must not be able to
    // read what came back: the request runs, the allow-origin header is absent.
    let other_port = c.get(format!("{base}/status")).header("Origin", "http://localhost:3000").send().await.unwrap();
    assert_eq!(other_port.status(), 200);
    assert!(other_port.headers().get("access-control-allow-origin").is_none(), "{:?}", other_port.headers());

    let evil = c.get(format!("{base}/status")).header("Origin", "https://evil.example").send().await.unwrap();
    assert_eq!(evil.status(), 403);
    assert!(evil.headers().get("access-control-allow-origin").is_none(), "{:?}", evil.headers());

    let bare = c.get(format!("{base}/status")).send().await.unwrap();
    assert_eq!(bare.status(), 200);
    assert!(bare.headers().get("access-control-allow-origin").is_none(), "{:?}", bare.headers());
}

/// `extraction.api_key` is masked on every GET and PUT response, a masked round trip
/// leaves the real key untouched, and an unknown key is rejected with 400.
#[tokio::test]
async fn settings_api_masks_the_key_and_validates_keys() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let put: serde_json::Value = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.api_key": "sk-test", "extraction.model": "deepseek-chat",
    })).send().await.unwrap().json().await.unwrap();
    assert_eq!(put["extraction.api_key"], "***", "{put}");
    assert_eq!(put["extraction.enabled"], true);
    assert_eq!(put["extraction.model"], "deepseek-chat");

    let get: serde_json::Value = c.get(format!("{base}/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(get["extraction.api_key"], "***", "{get}");
    assert_eq!(get["extraction.enabled"], true);

    // A masked round trip (the shape a GET->PUT client sends back) must not clobber the
    // real key: it stays masked and non-empty after another PUT that doesn't touch it.
    let put2: serde_json::Value = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.model": "x", "extraction.api_key": "***",
    })).send().await.unwrap().json().await.unwrap();
    assert_eq!(put2["extraction.api_key"], "***", "{put2}");
    assert_ne!(put2["extraction.api_key"], "", "{put2}");
    assert_eq!(put2["extraction.model"], "x");

    let bad = c.put(format!("{base}/settings")).json(&serde_json::json!({"bogus": 1})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let body: serde_json::Value = bad.json().await.unwrap();
    assert!(body["error"].as_str().is_some(), "{body}");
}

/// `DELETE /projects/{id}` answers 204 and drops the project from the list. The memories
/// scoped to it stay: nothing is hard-deleted from `memories`.
#[tokio::test]
async fn projects_can_be_deleted_without_losing_their_memories() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

/// A global sync must write into the home `ATLAS_SYNC_HOME` names, not the real one, and
/// must write agent files only: home has no project to name in a managed block.
#[tokio::test]
async fn global_sync_honours_the_sync_home_override() {
    let home = tempfile::tempdir().unwrap();
    let d = start_with_env(&[("ATLAS_SYNC_HOME", home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let saved = c.post(format!("{base}/agents")).json(&serde_json::json!({"name":"reviewer","description":"Reviews PRs","instructions":"Be strict.","tools":["Read"],"tags":[]})).send().await.unwrap();
    assert_eq!(saved.status(), 201);

    let targets = serde_json::json!(["claude", "codex", "agents_md", "claude_md"]);
    let rep: serde_json::Value = c.post(format!("{base}/sync")).json(&serde_json::json!({"global": true, "targets": targets, "check_only": false})).send().await.unwrap().json().await.unwrap();
    assert_eq!(rep["created"], 2, "{rep}");
    assert_eq!(rep["skipped"], 2, "the two managed-block targets are reported, not dropped: {rep}");

    assert!(home.path().join(".claude/agents/reviewer.md").exists(), "no Claude agent file under the override home");
    assert!(home.path().join(".codex/agents/reviewer.toml").exists(), "no Codex agent file under the override home");
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
    assert!(!elsewhere.path().join(".claude/agents/reviewer.md").exists(), "a `home` field in the request body must not redirect the sync");
    assert!(!elsewhere.path().join(".codex/agents/reviewer.toml").exists(), "a `home` field in the request body must not redirect the sync");
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
    let c = reqwest::Client::new();
    let claude_hook = repo.path().join(".claude/settings.json");
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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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

/// What the stub model returns for any prompt: the two candidates the ingest tests
/// expect, neither confident enough to clear the default auto-accept bar of 1.0.
const STUB_CANDIDATES: &str = r#"[{"text":"the project uses bun","kind":"fact","tags":["tooling"],"confidence":0.9},{"text":"deploy target is fly.io","kind":"decision","tags":["infra"],"confidence":0.7}]"#;

/// An OpenAI-compatible chat endpoint in the test process, so the daemon's
/// extraction runs end to end without reaching the network. Returns its base url,
/// `/v1`, which is what `extraction.base_url` is set to. Answers every prompt with
/// `reply`, whatever it was.
async fn stub_llm_with_reply(reply: &str) -> String {
    let content = reply.to_string();
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move || {
            let content = content.clone();
            async move { axum::Json(serde_json::json!({"choices": [{"message": {"content": content}}]})) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/v1")
}

/// The ingest tests' stub: always answers with `STUB_CANDIDATES`.
async fn stub_llm() -> String { stub_llm_with_reply(STUB_CANDIDATES).await }

/// Polls a job until it stops running, for up to 10 s. The worker calls out to the
/// model, so the result is never there on the first read.
async fn wait_for_job(c: &reqwest::Client, base: &str, job_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let job: serde_json::Value = c.get(format!("{base}/jobs/{job_id}")).send().await.unwrap().json().await.unwrap();
        if job["status"] == "done" || job["status"] == "failed" { return job; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("job {job_id} did not finish within 10s");
}

/// The whole opt-in extraction path: settings turn it on, `POST /ingest` queues a
/// job, the worker asks the model, and the candidates land as pending memories
/// stamped with the extractor. Replaying the same transcript stores nothing.
#[tokio::test]
async fn ingest_extracts_candidates_and_skips_duplicates_on_replay() {
    let stub = stub_llm().await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();
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

/// `POST /ingest` refuses a transcript over the 1,000,000 character cap with 413
/// before it ever reaches the queue, so no oversized body can spend a model call.
#[tokio::test]
async fn ingest_refuses_a_transcript_over_the_size_cap() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let text = "x".repeat(1_000_001);
    let big = c.post(format!("{base}/ingest")).json(&serde_json::json!({"text": text, "source_tool": "test"})).send().await.unwrap();
    assert_eq!(big.status(), 413);
    let body: serde_json::Value = big.json().await.unwrap();
    assert_eq!(body["error"], "transcript too large", "{body}");
}

/// `POST /extraction/test` gates on the same settings `POST /ingest` does (409 while
/// disabled or unconfigured), sends the connectivity check to the configured model
/// when it is, and reports a model-endpoint error as 400 rather than 500.
#[tokio::test]
async fn extraction_test_endpoint_checks_connectivity_and_reports_model_errors() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let disabled = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(disabled.status(), 409);
    let body: serde_json::Value = disabled.json().await.unwrap();
    assert_eq!(body["error"], "extraction is disabled", "{body}");

    let stub = stub_llm_with_reply("OK").await;
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let ok = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(ok.status(), 200);
    let body: serde_json::Value = ok.json().await.unwrap();
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["reply"], "OK", "{body}");

    // Point at an endpoint that will refuse the connection: a model-endpoint error
    // is a 400 with ok:false, not a 500.
    let put2 = c.put(format!("{base}/settings")).json(&serde_json::json!({"extraction.base_url": "http://127.0.0.1:1"})).send().await.unwrap();
    assert_eq!(put2.status(), 200);
    let failed = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(failed.status(), 400);
    let body: serde_json::Value = failed.json().await.unwrap();
    assert_eq!(body["ok"], false, "{body}");
    assert!(body["error"].as_str().is_some(), "{body}");
}

/// `POST /projects/{id}/refresh` enqueues a `project_summary` job when extraction is
/// enabled, and the worker writes the model's reply into `profile.summary`, visible
/// through `GET /projects/{id}` once the job finishes.
#[tokio::test]
async fn refresh_project_enqueues_a_summary_job_when_extraction_is_enabled() {
    let stub = stub_llm_with_reply("A Rust workspace.").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
