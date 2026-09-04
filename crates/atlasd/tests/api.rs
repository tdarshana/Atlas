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
    // Every daemon here gets an empty `ATLAS_SYNC_HOME` of its own, before the caller's
    // own environment (which wins, since `envs` is applied after this). That is the
    // home skill discovery reads, and no test may read the user's real
    // `~/.claude/skills` or write into their home.
    let sync_home = home.path().join("sync-home");
    std::fs::create_dir_all(&sync_home).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_atlasd"))
        .args(["--port", "0", "--home", home.path().to_str().unwrap(), "--no-embed"])
        .env("ATLAS_SYNC_HOME", &sync_home)
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

/// `memories_pending` counts memories set to `pending`, separately from
/// `memories_active`, so the desktop app's notification poller can tell the two apart.
#[tokio::test]
async fn status_reports_the_pending_memory_count() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

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
    // project_connect and memory_review are disabled by default (mcp.disabled_tools),
    // so they are not in this list; see atlas-mcp's own gating tests for that.
    for t in ["memory_remember", "memory_search", "memory_forget", "status", "project_context", "agent_list",
              "agent_get", "agent_save", "practice_list", "practice_get", "workflow_list", "workflow_get"] {
        assert!(body.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {body}");
    }

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_remember","arguments":{"text":"mcp round trip works","kind":"insight"}}})).await;
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

/// `GET /api/v1/mcp/status` before any client has connected: the static parts (the
/// transports, the 32-row tools table with the two defaults disabled, resources and
/// prompts) are already there, and no client has registered yet. Then one HTTP
/// `tools/call` over the same session `mcp_over_http_lists_and_calls_tools` drives
/// registers that session and counts the call.
#[tokio::test]
async fn mcp_status_reports_transports_counts_and_an_http_client_after_a_call() {
    let d = start().await;
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let status: serde_json::Value = c.get(format!("{base}/mcp/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["transports"]["stdio"]["command"], "atlas mcp", "{status}");
    assert!(status["transports"]["http"]["url"].as_str().unwrap().ends_with("/mcp"), "{status}");
    assert!(!status["transports"]["http"]["protocol_version"].as_str().unwrap().is_empty(), "{status}");
    let tools = status["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 32, "{tools:?}");
    let disabled: Vec<&str> = tools.iter().filter(|t| t["enabled"].as_bool() == Some(false)).map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(disabled.len(), 2, "{disabled:?}");
    assert!(disabled.contains(&"project_connect") && disabled.contains(&"memory_review"), "{disabled:?}");
    assert_eq!(status["counts"]["tools"], 32, "{status}");
    assert_eq!(status["counts"]["resources"].as_u64().unwrap(), status["resources"].as_array().unwrap().len() as u64, "{status}");
    assert_eq!(status["counts"]["prompts"].as_u64().unwrap(), status["prompts"].as_array().unwrap().len() as u64, "{status}");
    assert!(status["clients"].as_array().unwrap().is_empty(), "{status}");

    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"status-test","version":"9.9"}}}))
        .send().await.unwrap();
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}})).await;

    let status: serde_json::Value = c.get(format!("{base}/mcp/status")).send().await.unwrap().json().await.unwrap();
    let clients = status["clients"].as_array().unwrap();
    assert_eq!(clients.len(), 1, "{clients:?}");
    assert_eq!(clients[0]["transport"], "http", "{clients:?}");
    assert_eq!(clients[0]["client_name"], "status-test", "{clients:?}");
    assert_eq!(clients[0]["client_version"], "9.9", "{clients:?}");
    assert_eq!(clients[0]["tool_calls"], 1, "{clients:?}");
    assert_eq!(status["counts"]["clients"], 1, "{status}");
}

/// The stdio shim's own registration routes, exercised directly rather than through a
/// real `atlas mcp` process (the CLI test `stdio_shim_registers_with_the_daemon_and_
/// appears_in_mcp_status` covers that end to end): register, heartbeat with a reported
/// call count, a heartbeat for an unknown id is a 404, an unknown transport (or the
/// "http" transport, which registers itself over `record_http_call` instead) is a 400,
/// and unregister drops the entry immediately.
#[tokio::test]
async fn mcp_clients_route_registers_heartbeats_and_unregisters() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let created: serde_json::Value = c.post(format!("{base}/mcp/clients"))
        .json(&serde_json::json!({"id": "test-id", "transport": "stdio", "client_name": "claude-code", "client_version": "1.0"}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(created["id"], "test-id", "{created}");
    assert_eq!(created["tool_calls"], 0, "{created}");

    let status: serde_json::Value = c.get(format!("{base}/mcp/status")).send().await.unwrap().json().await.unwrap();
    let clients = status["clients"].as_array().unwrap();
    assert_eq!(clients.len(), 1, "{clients:?}");
    assert_eq!(clients[0]["transport"], "stdio", "{clients:?}");

    let heartbeat = c.put(format!("{base}/mcp/clients/test-id")).json(&serde_json::json!({"tool_calls": 5})).send().await.unwrap();
    assert_eq!(heartbeat.status(), 200);
    let heartbeat_body: serde_json::Value = heartbeat.json().await.unwrap();
    assert_eq!(heartbeat_body["tool_calls"], 5, "{heartbeat_body}");

    let missing = c.put(format!("{base}/mcp/clients/no-such-id")).json(&serde_json::json!({"tool_calls": 1})).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    let bad_transport = c.post(format!("{base}/mcp/clients")).json(&serde_json::json!({"id": "x", "transport": "carrier-pigeon", "client_name": "y"})).send().await.unwrap();
    assert_eq!(bad_transport.status(), 400);

    // This route is the stdio shim's own explicit registration; an HTTP session is
    // picked up on its first tool call instead, so a local caller cannot use this
    // route to plant a row that claims to be an HTTP session.
    let http_transport = c.post(format!("{base}/mcp/clients")).json(&serde_json::json!({"id": "y", "transport": "http", "client_name": "z"})).send().await.unwrap();
    assert_eq!(http_transport.status(), 400);

    let deleted = c.delete(format!("{base}/mcp/clients/test-id")).send().await.unwrap();
    assert_eq!(deleted.status(), 204);
    let status: serde_json::Value = c.get(format!("{base}/mcp/status")).send().await.unwrap().json().await.unwrap();
    assert!(status["clients"].as_array().unwrap().is_empty(), "{status}");
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
        .header("Access-Control-Request-Headers", "content-type, x-atlas-actor")
        .send().await.unwrap();
    assert!(preflight.status().is_success(), "{}", preflight.status());
    assert_eq!(preflight.headers().get("access-control-allow-origin").unwrap(), "tauri://localhost");
    let allow_headers = preflight.headers().get("access-control-allow-headers").unwrap_or_else(|| panic!("no access-control-allow-headers")).to_str().unwrap().to_ascii_lowercase();
    assert!(allow_headers.contains("content-type"), "{allow_headers}");
    assert!(allow_headers.contains("x-atlas-actor"), "the board actor header must pass preflight: {allow_headers}");

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
async fn stub_llm_with_reply(reply: &str) -> String { stub_llm_with_delay(reply, Duration::ZERO).await }

/// The same stub, holding each request open for `delay` first. The worker drains the
/// queue one job at a time, so a slow first job is what keeps a second one queued long
/// enough for a test to change the daemon's settings underneath it.
async fn stub_llm_with_delay(reply: &str, delay: Duration) -> String {
    let content = reply.to_string();
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move || {
            let content = content.clone();
            async move {
                tokio::time::sleep(delay).await;
                axum::Json(serde_json::json!({"choices": [{"message": {"content": content}}]}))
            }
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

/// The actor for `POST /ingest` comes from `X-Atlas-Actor` when present, falls back
/// to the deprecated `source_tool` body field, and is a 400 naming both ways to send
/// it when neither is there. The header wins when both are sent.
#[tokio::test]
async fn ingest_actor_comes_from_the_header_over_the_deprecated_body_field() {
    let stub = stub_llm().await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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

/// The daemon has no authentication of its own, so any process that can reach the
/// loopback port could otherwise repoint `extraction.base_url` and then have the
/// daemon send the stored key to a host of its choosing. Moving the endpoint without
/// supplying a new key drops the stored one, and the PUT says so.
#[tokio::test]
async fn changing_the_base_url_clears_the_stored_key() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
    let put = |body: serde_json::Value| c.put(format!("{base}/settings")).json(&body).send();

    let configured: serde_json::Value = put(serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": "https://api.deepseek.com", "extraction.model": "m", "extraction.api_key": "sk-real",
    })).await.unwrap().json().await.unwrap();
    assert_eq!(configured["extraction.api_key"], "***", "{configured}");
    assert!(configured["extraction.api_key_cleared"].is_null(), "a key entered with its endpoint is kept: {configured}");

    // The same endpoint again, trailing slash and all, is not a move.
    let same: serde_json::Value = put(serde_json::json!({"extraction.base_url": "https://api.deepseek.com/"})).await.unwrap().json().await.unwrap();
    assert!(same["extraction.api_key_cleared"].is_null(), "{same}");
    assert_eq!(same["extraction.api_key"], "***", "{same}");

    // Somewhere new, with no key of its own: the old key does not follow it there.
    let moved: serde_json::Value = put(serde_json::json!({"extraction.base_url": "http://attacker.example/v1"})).await.unwrap().json().await.unwrap();
    assert_eq!(moved["extraction.api_key_cleared"], true, "{moved}");
    assert_eq!(moved["extraction.api_key"], "", "the masked key is gone because there is no key: {moved}");
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

// ---- board ----

/// True for a key like `ATL-12` or `ATLAS-7`: one or more uppercase letters or
/// digits, a dash, then one or more digits. Written by hand rather than pulling in
/// the `regex` crate for a single check.
fn looks_like_a_task_key(key: &str) -> bool {
    let Some((prefix, seq)) = key.rsplit_once('-') else { return false };
    !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && !seq.is_empty() && seq.chars().all(|c| c.is_ascii_digit())
}

/// `POST /tasks` answers 201 with a key matching `^[A-Z0-9]+-\d+$`; `GET /tasks?stage=`
/// filters by stage; `ready=true` excludes a task blocked by an open task and includes
/// it once the blocker reaches a done stage; moving to an unknown stage is a 400
/// naming the valid ones.
#[tokio::test]
async fn board_tasks_ready_query_and_stage_moves() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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

/// `scope=global` is the literal global board: tasks with no project at all, not a
/// bare `project_id`-less request, which leaves every project's tasks in. It is
/// refused alongside `project_id`.
#[tokio::test]
async fn tasks_scope_global_keeps_just_the_project_less_tasks() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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
    let c = reqwest::Client::new();

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

// ---- Phase 8: the project hub ----

/// `PATCH /projects/{id}` renames the project, its board key and every task key on
/// its board, and refuses a malformed key with a 400 and an unknown project with a 404.
#[tokio::test]
async fn project_patch_renames_the_board_key_and_validates_it() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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

/// Task MCP-A: `GET /projects/{id}/mcp` before any override shows every tool
/// `enabled_here`, only this project's own `atlas://` resources, and the `connect`
/// shape; `PUT /projects/{id}/mcp/tools` writes the override (reflected on the very
/// next `GET`, and on the resolved-call gate a live MCP session meets), refuses an
/// unknown tool name with a 400, and 404s an unknown project.
#[tokio::test]
async fn project_mcp_route_reports_and_gates_a_project_override() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let c = reqwest::Client::new();
    let dir = repo_free_tempdir();
    fixture_repo(dir.path());

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": dir.path()})).send().await.unwrap().json().await.unwrap();
    let id = project["id"].as_str().unwrap().to_string();

    let before: serde_json::Value = c.get(format!("{base}/projects/{id}/mcp")).send().await.unwrap().json().await.unwrap();
    assert_eq!(before["connect"]["stdio"]["command"], "atlas mcp", "{before}");
    assert!(before["connect"]["http"]["url"].as_str().unwrap().ends_with("/mcp"), "{before}");
    assert_eq!(before["connect"]["project_root"], project["root_path"], "{before}");
    let tools = before["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 32, "{tools:?}");
    let task_move = tools.iter().find(|t| t["name"] == "task_move").unwrap();
    assert_eq!(task_move["enabled_globally"], true, "{task_move}");
    assert_eq!(task_move["enabled_here"], true, "{task_move}");
    // `project_connect` is disabled by default globally, so it must read as such here too.
    let connect_tool = tools.iter().find(|t| t["name"] == "project_connect").unwrap();
    assert_eq!(connect_tool["enabled_globally"], false, "{connect_tool}");
    assert_eq!(connect_tool["enabled_here"], false, "{connect_tool}");
    let resources = before["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 3, "only this project's context, practices and board: {resources:?}");
    assert!(resources.iter().all(|r| r["uri"].as_str().unwrap().contains(project["name"].as_str().unwrap())), "{resources:?}");
    assert!(before["clients"].as_array().unwrap().is_empty(), "{before}");

    let bad = c.put(format!("{base}/projects/{id}/mcp/tools")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["no_such_tool"]})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let bad_body: serde_json::Value = bad.json().await.unwrap();
    assert!(bad_body["error"].as_str().unwrap().contains("no_such_tool"), "{bad_body}");

    let put = c.put(format!("{base}/projects/{id}/mcp/tools")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["task_move"]})).send().await.unwrap();
    assert_eq!(put.status(), 200);
    let put_body: serde_json::Value = put.json().await.unwrap();
    assert_eq!(put_body["mcp_disabled_tools"], serde_json::json!(["task_move"]), "{put_body}");

    let after: serde_json::Value = c.get(format!("{base}/projects/{id}/mcp")).send().await.unwrap().json().await.unwrap();
    let task_move = after["tools"].as_array().unwrap().iter().find(|t| t["name"] == "task_move").unwrap().clone();
    assert_eq!(task_move["enabled_globally"], true, "{task_move}");
    assert_eq!(task_move["enabled_here"], false, "{task_move}");

    // A plugin's tools belong on this tab too, carrying `source` so the desktop's
    // `Plugin` badge lights, and meeting both gates the same way a built-in does.
    let registered = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({
        "tools": [{
            "name": "count",
            "description": "Counts the things.",
            "args": {"type": "object", "properties": {"of": {"type": "string"}}, "required": ["of"]},
            "scope": "read",
        }],
    })).send().await.unwrap();
    assert_eq!(registered.status(), 204, "register failed");

    let with_plugin: serde_json::Value = c.get(format!("{base}/projects/{id}/mcp")).send().await.unwrap().json().await.unwrap();
    let plugin_row = with_plugin["tools"].as_array().unwrap().iter()
        .find(|t| t["name"] == "plugin__hello_world__count")
        .unwrap_or_else(|| panic!("the project tab has no plugin row: {with_plugin}")).clone();
    assert_eq!(plugin_row["source"], "plugin:hello-world", "{plugin_row}");
    assert_eq!(plugin_row["scope"], "read", "{plugin_row}");
    assert_eq!(plugin_row["args"], "of*", "{plugin_row}");
    assert_eq!(plugin_row["enabled_globally"], true, "{plugin_row}");
    assert_eq!(plugin_row["enabled_here"], true, "{plugin_row}");
    let builtin_row = with_plugin["tools"].as_array().unwrap().iter().find(|t| t["name"] == "memory_remember").unwrap();
    assert_eq!(builtin_row["source"], "builtin", "{builtin_row}");

    let put = c.put(format!("{base}/projects/{id}/mcp/tools")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["task_move", "plugin__hello_world__count"]})).send().await.unwrap();
    assert_eq!(put.status(), 200, "disabling a plugin tool per project failed: {}", put.text().await.unwrap());
    let gated: serde_json::Value = c.get(format!("{base}/projects/{id}/mcp")).send().await.unwrap().json().await.unwrap();
    let plugin_row = gated["tools"].as_array().unwrap().iter().find(|t| t["name"] == "plugin__hello_world__count").unwrap().clone();
    assert_eq!(plugin_row["enabled_globally"], true, "{plugin_row}");
    assert_eq!(plugin_row["enabled_here"], false, "the project override must reach a plugin tool: {plugin_row}");

    let missing = c.get(format!("{base}/projects/{}/mcp", uuid::Uuid::new_v4())).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    // The gate a live call actually meets: `task_move` is refused when the call
    // resolves to this project, over the same MCP session the daemon serves at `/mcp`.
    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"gate-test","version":"1.0"}}}))
        .send().await.unwrap();
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;
    let root = dir.path().to_string_lossy().to_string();
    let call = rpc(&c, &url, &session, serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"task_move","arguments":{"key":"NOPE-1","stage":"Done","project_root": root}}
    })).await;
    assert_eq!(rpc_json(&call)["error"]["code"], -32601, "{call}");

    // A tool this project never disabled still resolves normally against it (past the
    // gate, into the tool's own not-found error for a bogus key rather than method-not-found).
    let allowed = rpc(&c, &url, &session, serde_json::json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"task_get","arguments":{"key":"NOPE-1"}}
    })).await;
    assert_ne!(rpc_json(&allowed)["error"]["code"], -32601, "{allowed}");
}

/// The three frameworks routes (Phase 12): the inventory-plus-documents listing, a
/// document's text by path, and importing tasks or decisions — including the 400 for
/// an unknown `{kind}` and the 409 an agent not on `agent_access` gets from an import.
#[tokio::test]
async fn frameworks_routes_round_trip_400_and_gate_import() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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


/// The gate and the worker resolve one project from one root. A `project_root` naming
/// a subdirectory of the repository is still that project: its override decides which
/// endpoint the transcript goes to (the global settings stay off throughout), and its
/// `memory_writers` decides who may queue one at all.
#[tokio::test]
async fn ingest_resolves_the_project_from_a_subdirectory_for_both_the_gate_and_the_worker() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

/// `scope=project_only` narrows a listing to the project's own memories; the default
/// still widens to the global ones, and asking to narrow with no project is a 400.
#[tokio::test]
async fn memories_can_be_listed_for_one_project_only() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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

/// `require_review` reaches the extraction worker, not just `POST /memories`: a
/// transcript an agent ingests into a project that requires review lands `pending`
/// however confident the model was, while the user's own hands still auto-accept.
#[tokio::test]
async fn require_review_holds_back_extracted_memories() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

/// The global `access.*` defaults fill in a project's unset `agent_access` fields; the
/// project's own value, once set, wins over the default outright. `require_review` is a
/// floor: a project that never set its own flag still lands an agent's memory `pending`
/// once the global default turns it on. `GET /projects/{id}/access` reports the
/// project's own rule, the global defaults and the two resolved together.
#[tokio::test]
async fn agent_access_defaults_are_inherited_and_overridable_per_project() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

/// `POST /memories/search` takes the same narrowing the listing does, under
/// `list_scope`: the project Memories tab's search box must not mix the global
/// memories back in, and the field must not collide with `scope`, which still names
/// the memory's own scope.
#[tokio::test]
async fn memory_search_can_be_narrowed_to_one_project() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

// ---- workflows (Phase 9): runner, scheduler surface, routes, MCP ----

/// A linear graph: one manual trigger, one action per entry in `instructions` (named
/// `step0`, `step1`, ...), one output with the given flags. Every action runs as the
/// `desktop` fallback agent, which needs no saved `Agent` row.
fn linear_workflow_graph(instructions: &[&str], propose_memories: bool, file_tasks: bool) -> serde_json::Value {
    let mut nodes = vec![serde_json::json!({"id": "t", "kind": "trigger", "position": {"x": 0.0, "y": 0.0}, "data": {"kind": "manual"}})];
    let mut edges = vec![];
    let mut prev = "t".to_string();
    for (i, text) in instructions.iter().enumerate() {
        let id = format!("a{i}");
        nodes.push(serde_json::json!({
            "id": id, "kind": "action", "position": {"x": 240.0 * (i as f64 + 1.0), "y": 0.0},
            "data": {"name": format!("step{i}"), "instructions": text, "agent": "desktop", "practices": [], "memories": null},
        }));
        edges.push(serde_json::json!({"id": format!("{prev}-{id}"), "source": prev, "target": id}));
        prev = id;
    }
    nodes.push(serde_json::json!({
        "id": "o", "kind": "output", "position": {"x": 240.0 * (instructions.len() as f64 + 1.0), "y": 0.0},
        "data": {"propose_memories": propose_memories, "file_tasks": file_tasks},
    }));
    edges.push(serde_json::json!({"id": format!("{prev}-o"), "source": prev, "target": "o"}));
    serde_json::json!({"nodes": nodes, "edges": edges})
}

/// Creates a manual-trigger workflow with a `linear_workflow_graph` and answers with
/// the created `Workflow` JSON.
async fn create_workflow(c: &reqwest::Client, base: &str, name: &str, instructions: &[&str], propose_memories: bool, file_tasks: bool) -> serde_json::Value {
    let graph = linear_workflow_graph(instructions, propose_memories, file_tasks);
    let r = c
        .post(format!("{base}/workflows"))
        .json(&serde_json::json!({"name": name, "trigger": {"kind": "manual"}, "graph": graph, "enabled": true}))
        .send()
        .await
        .unwrap();
    let status = r.status();
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(status, 201, "{body}");
    body
}

/// Polls `GET /runs/{id}` until the run reaches success, failed or cancelled, for up
/// to 10 s, and answers with `{ run, steps }`.
async fn wait_for_run(c: &reqwest::Client, base: &str, run_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let detail: serde_json::Value = c.get(format!("{base}/runs/{run_id}")).send().await.unwrap().json().await.unwrap();
        if matches!(detail["run"]["status"].as_str(), Some("success") | Some("failed") | Some("cancelled")) {
            return detail;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("run {run_id} did not finish within 10s");
}

/// A two-action workflow runs both actions in order against the stub model, and each
/// finished step carries its output and at least one INFO log line.
#[tokio::test]
async fn workflow_run_executes_two_actions_and_succeeds() {
    let stub = stub_llm_with_reply("step done").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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

/// `GET /api/v1/runs?since=&limit=` lists finished runs across every workflow, newest
/// first, and a `since` set to just after the run finished excludes it: the notification
/// poller's own use of the parameter.
#[tokio::test]
async fn all_runs_route_lists_runs_finished_after_since() {
    let stub = stub_llm_with_reply("step done").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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
    let c = reqwest::Client::new();
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

/// `workflow_run` and `workflow_status` are listed, a run started by name is followed
/// to success, and `workflow_run` on a name nothing was ever saved under is
/// `invalid_params` (JSON-RPC -32602), not an internal error.
#[tokio::test]
async fn mcp_workflow_tools_run_and_report_status() {
    let stub = stub_llm_with_reply("ok").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let c = reqwest::Client::new();
    c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    let workflow = create_workflow(&c, &base, "mcp-flow", &["do the thing"], false, false).await;
    let wname = workflow["name"].as_str().unwrap().to_string();

    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    assert!(init.status().is_success(), "initialize failed: {}", init.status());
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    for t in ["workflow_run", "workflow_status"] {
        assert!(body.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {body}");
    }

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workflow_run","arguments":{"name":"does-not-exist"}}})).await;
    let reply = rpc_json(&body);
    assert_eq!(reply["error"]["code"], -32602, "{reply}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"workflow_run","arguments":{"name": wname}}})).await;
    let started = tool_json(&body);
    assert_eq!(started["number"], 1, "{started}");
    let run_id = started["run_id"].as_str().unwrap().to_string();

    let mut last = serde_json::Value::Null;
    for _ in 0..100 {
        let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"workflow_status","arguments":{"run_id": run_id}}})).await;
        last = tool_json(&body);
        if matches!(last["run"]["status"].as_str(), Some("success") | Some("failed") | Some("cancelled")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(last["run"]["status"], "success", "{last}");
    let steps = last["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1, "{last}");
    assert_eq!(steps[0]["name"], "step0", "{last}");
    assert_eq!(steps[0]["status"], "success", "{last}");
}

/// `workflow_list` (MCP) answers with the `WorkflowRepo` summary shape: name,
/// trigger, action count, enabled, last status, not the retired workflow-document
/// listing; `workflow_get` answers with the full workflow, graph included.
#[tokio::test]
async fn mcp_list_and_get_workflow_read_the_workflow_repo() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let c = reqwest::Client::new();
    let workflow = create_workflow(&c, &base, "repo-backed", &["one", "two"], false, false).await;
    let wname = workflow["name"].as_str().unwrap().to_string();

    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"workflow_list","arguments":{}}})).await;
    let listed = tool_json(&body);
    let row = listed.as_array().unwrap().iter().find(|w| w["name"] == wname).expect("the workflow in the listing");
    assert_eq!(row["trigger"], "manual", "{row}");
    assert_eq!(row["action_count"], 2, "{row}");
    assert_eq!(row["enabled"], true, "{row}");
    assert_eq!(row["last_status"], serde_json::Value::Null, "{row}");
    assert!(row.get("graph").is_none(), "the listing must not carry the full graph: {row}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workflow_get","arguments":{"name": wname}}})).await;
    let got = tool_json(&body);
    assert_eq!(got["name"], wname, "{got}");
    assert_eq!(got["graph"]["nodes"].as_array().unwrap().len(), 4, "{got}");
}

/// Phase 14: DuckDB (and, with an embedder loaded, ONNX) work runs on blocking
/// threads via `LocalBackend::blocking`, so the HTTP server keeps answering while a
/// large write runs. Fires 20 concurrent `/memories/search` requests alongside a
/// 200-row `/memories` batch and asserts every `/status` probe taken while the batch
/// is in flight answers within 500 ms; before this change the batch's DuckDB writes
/// ran directly on the async runtime's worker threads and could starve `/status`.
#[tokio::test]
async fn status_stays_responsive_under_concurrent_load() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    // Seed a few memories so the concurrent searches below have something to score.
    for i in 0..20 {
        let r = c.post(format!("{base}/memories"))
            .json(&serde_json::json!({"scope":"global","kind":"fact","text": format!("seed memory {i} about bun and duckdb")}))
            .send().await.unwrap();
        assert_eq!(r.status(), 201);
    }

    // A large batch of writes, running in the background while the probes below run.
    let write_base = base.clone();
    let writer = tokio::spawn(async move {
        let c = reqwest::Client::new();
        for i in 0..200 {
            let r = c.post(format!("{write_base}/memories"))
                .json(&serde_json::json!({"scope":"global","kind":"fact","text": format!("bulk memory {i} about the atlas daemon and its duckdb file")}))
                .send().await.unwrap();
            assert_eq!(r.status(), 201);
        }
    });

    // 20 concurrent searches, running alongside the batch above.
    let mut searchers = Vec::new();
    for _ in 0..20 {
        let search_base = base.clone();
        searchers.push(tokio::spawn(async move {
            let c = reqwest::Client::new();
            let r = c.post(format!("{search_base}/memories/search")).json(&serde_json::json!({"query":"bun duckdb"})).send().await.unwrap();
            assert_eq!(r.status(), 200);
        }));
    }

    // Probe /status while the batch and the searches are in flight, and record the
    // worst latency seen. At least 5 probes run regardless of how fast the batch
    // finishes, so the assertion below is never skipped by a vacuous loop.
    let mut worst = Duration::from_millis(0);
    let mut probes = 0usize;
    loop {
        let started = std::time::Instant::now();
        let r = c.get(format!("{base}/status")).send().await.unwrap();
        let elapsed = started.elapsed();
        assert_eq!(r.status(), 200);
        worst = worst.max(elapsed);
        assert!(elapsed < Duration::from_millis(500), "a /status probe took {elapsed:?} while a 200-row batch and 20 concurrent searches were running");
        probes += 1;
        if writer.is_finished() && probes >= 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    writer.await.unwrap();
    for s in searchers {
        s.await.unwrap();
    }

    eprintln!("worst /status latency under concurrent load ({probes} probes): {worst:?}");
}

// ---- plugin MCP tools (Phase 13b) ----

/// Stands in for the desktop app: opens the plugin channel, then answers every request
/// frame with `answer` (which sees the frame and returns the reply body). The join
/// handle finishes when the socket closes.
async fn plugin_app(
    port: u16,
    answer: impl Fn(serde_json::Value) -> serde_json::Value + Send + 'static,
) -> (tokio::task::JoinHandle<()>, tokio::sync::oneshot::Sender<()>) {
    use futures_util::{SinkExt, StreamExt};
    let url = format!("ws://127.0.0.1:{port}/api/v1/mcp/plugin-channel");
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.expect("plugin channel refused the upgrade");
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut close_rx => { let _ = socket.close(None).await; break; }
                frame = socket.next() => match frame {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        let request: serde_json::Value = serde_json::from_str(&text).unwrap();
                        let reply = answer(request);
                        socket.send(tokio_tungstenite::tungstenite::Message::text(reply.to_string())).await.unwrap();
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                },
            }
        }
    });
    (handle, close_tx)
}

/// A registered plugin tool is listed to an MCP client under its `plugin__` name, a call
/// is forwarded down the channel and its result comes back, the `mcp/status` report
/// names the plugin as the row's source, `mcp.disabled_tools` hides it the way it hides
/// a built-in, and a disconnect takes the tool with it.
#[tokio::test]
async fn plugin_tools_are_registered_listed_called_and_dropped_with_the_socket() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let c = reqwest::Client::new();

    let registered = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({
        "tools": [{
            "name": "count",
            "description": "Counts the things.",
            "args": {"type": "object", "properties": {"of": {"type": "string"}}, "required": ["of"]},
            "scope": "read",
        }],
    })).send().await.unwrap();
    assert_eq!(registered.status(), 204, "register failed");

    let listed: Vec<serde_json::Value> = c.get(format!("{base}/mcp/plugin-tools")).send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["plugin_id"], "hello-world", "the path's id is filled in: {listed:?}");

    // A malformed decl is refused before it can reach any MCP client's tool list.
    let bad = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({
        "tools": [{"name": "Not A Name", "description": "x", "args": {"type": "object"}, "scope": "read"}],
    })).send().await.unwrap();
    assert_eq!(bad.status(), 400, "a malformed tool name was accepted");

    // A tool name or plugin id that would make the MCP name ambiguous is refused too.
    let ambiguous_name = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({
        "tools": [{"name": "b__count", "description": "x", "args": {"type": "object"}, "scope": "read"}],
    })).send().await.unwrap();
    assert_eq!(ambiguous_name.status(), 400);
    let message = ambiguous_name.json::<serde_json::Value>().await.unwrap()["error"].as_str().unwrap().to_string();
    assert!(message.contains("cannot contain \"__\""), "{message}");

    let ambiguous_id = c.put(format!("{base}/mcp/plugin-tools/hello--world")).json(&serde_json::json!({
        "tools": [{"name": "count", "description": "x", "args": {"type": "object"}, "scope": "read"}],
    })).send().await.unwrap();
    assert_eq!(ambiguous_id.status(), 400);
    let message = ambiguous_id.json::<serde_json::Value>().await.unwrap()["error"].as_str().unwrap().to_string();
    assert!(message.contains("cannot contain \"--\""), "{message}");

    // An empty set is an unregister, and it still refuses an id it would never store.
    let empty_bad_id = c.put(format!("{base}/mcp/plugin-tools/Not%20An%20Id")).json(&serde_json::json!({"tools": []})).send().await.unwrap();
    assert_eq!(empty_bad_id.status(), 400, "an empty set skipped the id check");

    // The one good registration above is still the only thing in the registry.
    let listed: Vec<serde_json::Value> = c.get(format!("{base}/mcp/plugin-tools")).send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.len(), 1, "a refused registration changed the registry: {listed:?}");

    let (app, close_app) = plugin_app(d.port, |request| {
        assert_eq!(request["plugin_id"], "hello-world", "{request}");
        assert_eq!(request["tool"], "count", "{request}");
        assert_eq!(request["args"]["of"], "sheep", "the caller's arguments reach the app: {request}");
        serde_json::json!({"id": request["id"], "ok": true, "result": {"count": 3}})
    }).await;

    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    assert!(init.status().is_success(), "initialize failed: {}", init.status());
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let _ = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    assert!(body.contains("\"name\":\"plugin__hello_world__count\""), "tools/list missing the plugin tool: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"plugin__hello_world__count","arguments":{"of":"sheep"}}})).await;
    assert_eq!(tool_json(&body)["count"], 3, "{body}");

    // The status report carries the plugin row with its source.
    let status: serde_json::Value = c.get(format!("{base}/mcp/status")).send().await.unwrap().json().await.unwrap();
    let row = status["tools"].as_array().unwrap().iter()
        .find(|t| t["name"] == "plugin__hello_world__count")
        .unwrap_or_else(|| panic!("mcp/status has no plugin row: {status}"));
    assert_eq!(row["source"], "plugin:hello-world", "{row}");
    assert_eq!(row["scope"], "read", "{row}");
    assert_eq!(row["enabled"], true, "{row}");
    assert_eq!(row["args"], "of*", "the schema's properties render as the args summary: {row}");
    let builtin = status["tools"].as_array().unwrap().iter().find(|t| t["name"] == "memory_remember").unwrap();
    assert_eq!(builtin["source"], "builtin", "{builtin}");

    // `mcp.disabled_tools` gates a plugin tool exactly like a built-in.
    let set = c.put(format!("{base}/settings")).json(&serde_json::json!({"mcp.disabled_tools": ["plugin__hello_world__count"]})).send().await.unwrap();
    assert_eq!(set.status(), 200, "settings write failed: {}", set.text().await.unwrap());
    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/list"})).await;
    assert!(!body.contains("plugin__hello_world__count"), "a disabled plugin tool is still listed: {body}");
    let set = c.put(format!("{base}/settings")).json(&serde_json::json!({"mcp.disabled_tools": []})).send().await.unwrap();
    assert_eq!(set.status(), 200);

    // The app goes away: its tools go with it, and a call says the plugin is not running.
    close_app.send(()).unwrap();
    app.await.unwrap();
    let mut gone = false;
    for _ in 0..100 {
        let listed: Vec<serde_json::Value> = c.get(format!("{base}/mcp/plugin-tools")).send().await.unwrap().json().await.unwrap();
        if listed.is_empty() { gone = true; break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone, "the registry still held the plugin's tools 5s after the socket closed");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/list"})).await;
    assert!(!body.contains("plugin__hello_world__count"), "tools/list still lists a disconnected plugin's tool: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
        "params":{"name":"plugin__hello_world__count","arguments":{}}})).await;
    let reply = rpc_json(&body);
    let message = reply["error"]["message"].as_str().unwrap_or_else(|| panic!("expected an error: {reply}"));
    assert!(message.contains("is not running"), "{reply}");
}

/// The plugin's own failure reaches the MCP client as the message it sent, and a call
/// through `POST .../call` (the stdio shim's path) reaches the same channel.
#[tokio::test]
async fn a_plugin_error_and_the_call_route_both_carry_the_plugins_answer() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let registered = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({
        "tools": [
            {"name": "ok_tool", "description": "Works.", "args": {"type": "object"}, "scope": "read"},
            {"name": "bad_tool", "description": "Fails.", "args": {"type": "object"}, "scope": "write"},
        ],
    })).send().await.unwrap();
    assert_eq!(registered.status(), 204);

    let (app, close_app) = plugin_app(d.port, |request| {
        if request["tool"] == "bad_tool" {
            serde_json::json!({"id": request["id"], "ok": false, "error": "the plugin said no"})
        } else {
            serde_json::json!({"id": request["id"], "ok": true, "result": {"echo": request["args"].clone()}})
        }
    }).await;

    let ok = c.post(format!("{base}/mcp/plugin-tools/hello-world/ok_tool/call"))
        .json(&serde_json::json!({"args": {"n": 1}})).send().await.unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.json::<serde_json::Value>().await.unwrap()["echo"]["n"], 1);

    let bad = c.post(format!("{base}/mcp/plugin-tools/hello-world/bad_tool/call"))
        .json(&serde_json::json!({"args": {}})).send().await.unwrap();
    assert_eq!(bad.status(), 400, "a plugin error is the caller's to fix, not a 500");
    assert_eq!(bad.json::<serde_json::Value>().await.unwrap()["error"], "invalid input: the plugin said no");

    // A tool the plugin never declared is refused without troubling the app.
    let unknown = c.post(format!("{base}/mcp/plugin-tools/hello-world/no_such_tool/call"))
        .json(&serde_json::json!({"args": {}})).send().await.unwrap();
    assert_eq!(unknown.status(), 400);
    let message = unknown.json::<serde_json::Value>().await.unwrap()["error"].as_str().unwrap().to_string();
    assert!(message.contains("unknown plugin tool plugin__hello_world__no_such_tool"), "{message}");

    // A second connection replaces the first, and the daemon hangs the first one up
    // rather than leaving its task parked until that client notices.
    let (second_app, close_second) = plugin_app(d.port, |request| serde_json::json!({"id": request["id"], "ok": true, "result": {}})).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), app).await;
    assert!(closed.is_ok(), "the replaced connection was still open 5s after being replaced");
    closed.unwrap().unwrap();
    let _ = close_app.send(());

    // The replacement serves calls, so the swap left a working channel behind.
    let after = c.post(format!("{base}/mcp/plugin-tools/hello-world/ok_tool/call"))
        .json(&serde_json::json!({"args": {}})).send().await.unwrap();
    assert_eq!(after.status(), 200, "the replacement connection does not serve calls");

    // An empty tool set unregisters, the same as DELETE.
    let emptied = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({"tools": []})).send().await.unwrap();
    assert_eq!(emptied.status(), 204);
    let listed: Vec<serde_json::Value> = c.get(format!("{base}/mcp/plugin-tools")).send().await.unwrap().json().await.unwrap();
    assert!(listed.is_empty(), "an empty set left tools behind: {listed:?}");

    // Unregistering an id that holds nothing is still 204.
    let removed = c.delete(format!("{base}/mcp/plugin-tools/hello-world")).send().await.unwrap();
    assert_eq!(removed.status(), 204);

    close_second.send(()).unwrap();
    second_app.await.unwrap();
}

/// Skills (Phase 15) end to end over the JSON API: the global listing, a project
/// listing that widens to the project's own `SKILL.md` folders, an in-place edit that
/// lands on disk, and a disabled list that shows in `enabled_here` and in
/// `project_context`.
#[tokio::test]
async fn skills_list_edit_and_gate_per_project() {
    // The daemon reads its global skills from `ATLAS_SYNC_HOME`, never the real home.
    let sync_home = tempfile::tempdir().unwrap();
    let user_skill = sync_home.path().join(".claude/skills/greeter");
    std::fs::create_dir_all(&user_skill).unwrap();
    // A block scalar description, the shape real plugin skills use: it has to arrive
    // folded, not as a bare ">-".
    std::fs::write(user_skill.join("SKILL.md"), "---\nname: greeter\ndescription: >-\n  Greets a person\n  by name.\n---\n\nSay hello.\n").unwrap();
    let packaged = sync_home.path().join(".claude/plugins/cache/acme/tools/aaaa1111/skills/packaged");
    std::fs::create_dir_all(&packaged).unwrap();
    std::fs::write(packaged.join("SKILL.md"), "---\nname: packaged\ndescription: From a plugin.\n---\n\nPackaged body.\n").unwrap();

    let d = start_with_env(&[("ATLAS_SYNC_HOME", sync_home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    let repo = repo_free_tempdir();
    fixture_repo(repo.path());
    let deployer = repo.path().join(".claude/skills/deployer");
    std::fs::create_dir_all(&deployer).unwrap();
    std::fs::write(deployer.join("SKILL.md"), "---\nname: deployer\ndescription: Ships it.\n---\n\nRun the deploy.\n").unwrap();
    std::fs::write(deployer.join("checklist.md"), "one\n").unwrap();

    // Without a project, only the global skills are listed.
    let global: serde_json::Value = c.get(format!("{base}/skills")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = global["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["claude-user:greeter", "plugin:acme/tools/packaged"], "sorted by name: {global}");
    assert!(global["skills"][0]["enabled_here"].is_null(), "no project was named: {global}");
    assert_eq!(global["warnings"].as_array().unwrap().len(), 0, "{global}");
    assert_eq!(global["skills"][0]["description"], "Greets a person by name.", "a block scalar description arrives folded: {global}");
    let plugin_row = &global["skills"][1];
    assert_eq!(plugin_row["plugin"], "acme/tools", "{plugin_row}");
    assert_eq!(plugin_row["source"], "plugin", "{plugin_row}");

    // A plugin skill's id carries slashes and a colon. Both reach the route: raw, the
    // way `RemoteBackend` builds it, and percent-encoded, the way a browser client's
    // `encodeURIComponent` does.
    for addressed in ["plugin:acme/tools/packaged", "plugin%3Aacme%2Ftools%2Fpackaged"] {
        let one = c.get(format!("{base}/skills/{addressed}")).send().await.unwrap();
        assert_eq!(one.status(), 200, "{addressed} did not resolve");
        assert!(one.json::<serde_json::Value>().await.unwrap()["body"].as_str().unwrap().contains("Packaged body."), "{addressed}");
    }

    let created = c.post(format!("{base}/skills")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"name": "house-style", "description": "How we write.", "body": "# house style\n"})).send().await.unwrap();
    assert_eq!(created.status(), 201);
    let native: serde_json::Value = created.json().await.unwrap();
    let native_id = native["id"].as_str().unwrap().to_string();
    assert_eq!(native["source"], "native", "{native}");
    assert_eq!(native["scope"], "global", "{native}");
    assert_eq!(native["editable"], true, "{native}");

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    // A project widens the listing to its own roots and answers `enabled_here`.
    let listed: serde_json::Value = c.get(format!("{base}/skills?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = listed["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"claude-project:deployer") && ids.contains(&"claude-user:greeter") && ids.contains(&native_id.as_str()), "{ids:?}");
    assert!(listed["skills"].as_array().unwrap().iter().all(|s| s["enabled_here"] == true), "{listed}");

    // One skill by id, with its text and the files beside it.
    let one: serde_json::Value = c.get(format!("{base}/skills/claude-project:deployer?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    assert!(one["body"].as_str().unwrap().contains("Run the deploy."), "{one}");
    assert_eq!(one["files"], serde_json::json!(["checklist.md"]), "{one}");
    assert_eq!(one["plugin"], serde_json::Value::Null, "{one}");
    // The stored root is canonical, so only the tail is compared: on macOS the temp
    // directory reaches the daemon as `/private/var/...` and the test as `/var/...`.
    assert!(one["path"].as_str().unwrap().ends_with(".claude/skills/deployer"), "{one}");

    // An edit in place rewrites the file on disk and comes back with the new text.
    let edited = "---\nname: deployer\ndescription: Ships it, carefully.\n---\n\nRun the deploy twice.\n";
    let put = c.put(format!("{base}/skills/claude-project:deployer?project_id={project_id}")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"body": edited})).send().await.unwrap();
    assert_eq!(put.status(), 200);
    let put_body: serde_json::Value = put.json().await.unwrap();
    assert_eq!(put_body["description"], "Ships it, carefully.", "{put_body}");
    assert_eq!(std::fs::read_to_string(deployer.join("SKILL.md")).unwrap(), edited);

    // The same route edits a native skill's stored body.
    let put = c.put(format!("{base}/skills/{native_id}")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"body": "# house style, revised\n"})).send().await.unwrap();
    assert_eq!(put.status(), 200);
    assert_eq!(put.json::<serde_json::Value>().await.unwrap()["body"], "# house style, revised\n");

    // An unknown id is a 404, not a path read.
    let missing = c.get(format!("{base}/skills/claude-user:nope")).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    // The disabled list refuses an id no skill holds, then takes a real one.
    let bad = c.put(format!("{base}/projects/{project_id}/skills")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["claude-user:nope"]})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    assert!(bad.json::<serde_json::Value>().await.unwrap()["error"].as_str().unwrap().contains("nope"));

    let gated = c.put(format!("{base}/projects/{project_id}/skills")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["claude-project:deployer"]})).send().await.unwrap();
    assert_eq!(gated.status(), 200);
    assert_eq!(gated.json::<serde_json::Value>().await.unwrap()["skills_disabled"], serde_json::json!(["claude-project:deployer"]));

    let listed: serde_json::Value = c.get(format!("{base}/skills?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    let deployer_row = listed["skills"].as_array().unwrap().iter().find(|s| s["id"] == "claude-project:deployer").unwrap();
    assert_eq!(deployer_row["enabled_here"], false, "a disabled skill still lists, switched off: {listed}");

    // `project_context` carries only the skills that still apply.
    let ctx: serde_json::Value = c.post(format!("{base}/projects/context")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let context_ids: Vec<&str> = ctx["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(!context_ids.contains(&"claude-project:deployer"), "{context_ids:?}");
    assert!(context_ids.contains(&"claude-user:greeter") && context_ids.contains(&native_id.as_str()), "{context_ids:?}");

    // A native skill can be deleted; a discovered one never is.
    let refused = c.delete(format!("{base}/skills/claude-user:greeter")).header("X-Atlas-Actor", "desktop").send().await.unwrap();
    assert_eq!(refused.status(), 400);
    let removed = c.delete(format!("{base}/skills/{native_id}")).header("X-Atlas-Actor", "desktop").send().await.unwrap();
    assert_eq!(removed.status(), 204);
    assert!(std::fs::read_to_string(user_skill.join("SKILL.md")).is_ok(), "no file was removed");
}

/// The agents' MCP servers (Phase 16) end to end over the JSON API: the listing with its
/// per-agent flags, a project listing, a check that answers 200 with `ok: false` for a
/// server that will not start, the enable switch, an add and a remove. Every file these
/// routes read or write is inside the daemon's own `ATLAS_SYNC_HOME` or the test's
/// repository, never the user's home.
#[tokio::test]
async fn mcp_servers_list_check_toggle_add_and_remove() {
    let sync_home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(sync_home.path().join(".cursor")).unwrap();
    std::fs::write(
        sync_home.path().join(".cursor/mcp.json"),
        r#"{
  "mcpServers": {
    "cursor-one": {
      "command": "atlas-no-such-command-exists",
      "args": [],
      "env": { "CURSOR_TOKEN": "SECRET-DO-NOT-LEAK" }
    }
  }
}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(sync_home.path().join(".claude/plugins/cache/acme/tools/aaaa1111")).unwrap();
    std::fs::write(
        sync_home.path().join(".claude/plugins/cache/acme/tools/aaaa1111/.mcp.json"),
        r#"{"mcpServers": {"packaged": {"command": "packaged-server", "args": []}}}"#,
    )
    .unwrap();
    std::fs::write(sync_home.path().join(".claude/settings.json"), r#"{"enabledPlugins": {"tools@acme": true}}"#).unwrap();

    let d = start_with_env(&[("ATLAS_SYNC_HOME", sync_home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = reqwest::Client::new();

    // The global listing: every agent's user level, the plugin's server and Atlas.
    let list: serde_json::Value = c.get(format!("{base}/mcp/servers")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = list["servers"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["atlas", "cursor:user:cursor-one", "plugin:acme/tools:packaged"], "{list}");
    assert_eq!(list["warnings"].as_array().unwrap().len(), 0, "{list}");
    assert!(!list.to_string().contains("SECRET-DO-NOT-LEAK"), "a secret reached the wire: {list}");

    let atlas = &list["servers"][0];
    assert_eq!(atlas["is_atlas"], true, "{atlas}");
    assert_eq!(atlas["transport"], serde_json::json!({"kind": "stdio", "command": "atlas", "args": ["mcp"], "env_keys": []}), "{atlas}");
    assert_eq!(atlas["can_toggle"], false, "{atlas}");
    let cursor = &list["servers"][1];
    assert_eq!(cursor["transport"]["env_keys"], serde_json::json!(["CURSOR_TOKEN"]), "{cursor}");
    assert_eq!(cursor["can_toggle"], true, "{cursor}");
    assert_eq!(list["servers"][2]["plugin"], "acme/tools", "{list}");

    // A check of a command that is not there is an answer, not an error.
    let checked = c.post(format!("{base}/mcp/servers/cursor%3Auser%3Acursor-one/check")).send().await.unwrap();
    assert_eq!(checked.status(), 200);
    let checked: serde_json::Value = checked.json().await.unwrap();
    assert_eq!(checked["ok"], false, "{checked}");
    assert!(checked["error"].as_str().unwrap().contains("atlas-no-such-command-exists"), "{checked}");

    // A plugin server's id carries a slash, so it travels percent-encoded.
    let plugin_check = c.post(format!("{base}/mcp/servers/plugin%3Aacme%2Ftools%3Apackaged/check")).send().await.unwrap();
    assert_eq!(plugin_check.status(), 200, "an encoded slash reaches the route");
    assert_eq!(plugin_check.json::<serde_json::Value>().await.unwrap()["ok"], false);

    // Enable and disable, through Cursor's own switch.
    let off = c
        .put(format!("{base}/mcp/servers/cursor%3Auser%3Acursor-one/enabled"))
        .header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"enabled": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(off.status(), 200);
    assert_eq!(off.json::<serde_json::Value>().await.unwrap()["enabled"], false);
    let refused = c
        .put(format!("{base}/mcp/servers/atlas/enabled"))
        .json(&serde_json::json!({"enabled": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 400, "Atlas has no switch");

    // Add: a new server in the repository's own `.mcp.json`, which Atlas may create.
    let repo = repo_free_tempdir();
    fixture_repo(repo.path());
    let project: serde_json::Value =
        c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    let added = c
        .post(format!("{base}/mcp/servers"))
        .header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({
            "source": "claude",
            "scope": "project",
            "project_id": project_id,
            "name": "repo-one",
            "transport": {"kind": "stdio", "command": "repo-server", "args": ["--serve"], "env": {"REPO_TOKEN": "SECRET-DO-NOT-LEAK"}}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(added.status(), 201);
    let added: serde_json::Value = added.json().await.unwrap();
    assert_eq!(added["id"], "claude:project:repo-one", "{added}");
    assert_eq!(added["transport"]["env_keys"], serde_json::json!(["REPO_TOKEN"]), "{added}");
    assert!(!added.to_string().contains("SECRET-DO-NOT-LEAK"), "{added}");
    let written = std::fs::read_to_string(repo.path().join(".mcp.json")).unwrap();
    assert!(written.contains("REPO_TOKEN") && written.contains("repo-server"), "{written}");

    // A project listing shows it, switched off until it is approved.
    let listed: serde_json::Value =
        c.get(format!("{base}/mcp/servers?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    let row = listed["servers"].as_array().unwrap().iter().find(|s| s["id"] == "claude:project:repo-one").unwrap();
    assert_eq!(row["enabled"], false, "a repository server waits for approval: {listed}");
    assert_eq!(row["project_id"], project_id, "{row}");

    // Adding the same name again is a conflict, not a replacement.
    let again = c
        .post(format!("{base}/mcp/servers"))
        .json(&serde_json::json!({
            "source": "claude", "scope": "project", "project_id": project_id, "name": "repo-one",
            "transport": {"kind": "http", "url": "https://example.test/mcp"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 409);

    // Remove.
    let removed = c
        .delete(format!("{base}/mcp/servers/claude%3Aproject%3Arepo-one?project_id={project_id}"))
        .header("X-Atlas-Actor", "desktop")
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 204);
    assert!(!std::fs::read_to_string(repo.path().join(".mcp.json")).unwrap().contains("repo-server"));
    let gone = c
        .delete(format!("{base}/mcp/servers/claude%3Aproject%3Arepo-one?project_id={project_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 404);
}
