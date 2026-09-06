//! Test-only harness shared by the `atlasd` and `atlas-cli` integration tests (ATL-439).
//! Nothing here is compiled into a binary: the crate is a dev-dependency only.
//!
//! The daemon harness takes the binary as an argument, since `CARGO_BIN_EXE_<name>` is
//! only handed to the crate that builds that binary; each crate's `tests/common` wraps
//! [`Daemon::spawn`] with its own path.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// How long a daemon gets to write `daemon.json`, and then again to answer `/status`:
/// `READY_ATTEMPTS` polls `READY_STEP` apart. Every test in the file boots its own daemon
/// and they all start at once, so under a full parallel run a single boot has been seen to
/// take well past twenty seconds; sixty per phase leaves room without hiding a daemon that
/// really never comes up.
pub const READY_ATTEMPTS: u32 = 600;

pub const READY_STEP: Duration = Duration::from_millis(100);

pub fn ready_budget() -> String { format!("{}s", READY_ATTEMPTS as u128 * READY_STEP.as_millis() / 1000) }

/// A running `atlasd` child on its own temporary home, killed on drop. `port` is the
/// one it bound and `token` the secret from its `daemon.json`.
pub struct Daemon { pub child: Child, pub port: u16, pub token: String, pub home: tempfile::TempDir }

impl Drop for Daemon { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

impl Daemon {
    /// A client that presents this daemon's token (SEC-5) on every request, the way
    /// every real client does; tests about the token itself build bare clients instead.
    pub fn client(&self) -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(TOKEN_HEADER, self.token.parse().unwrap());
        reqwest::Client::builder().default_headers(headers).build().unwrap()
    }
}

/// The header every `/api/v1` request carries (SEC-5).
pub const TOKEN_HEADER: &str = "X-Atlas-Token";

/// Lower-case hex SHA-256 of `token`, the form `status` reports it in.
pub fn sha256_hex(token: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A daemon with extra environment variables, for the settings the daemon reads at
    /// call time rather than from its arguments (`ATLAS_SYNC_HOME`).
    ///
    /// Every test in the file starts its own daemon and they run at once, so each is asked
    /// to bind an ephemeral port (`--port 0`) rather than a port picked in the test process
    /// and handed over: with nine tests racing, two could otherwise be handed the same
    /// number and one daemon would fail to bind it. The real port is read back from
    /// `daemon.json`, which the daemon writes only once it holds the port.
    impl Daemon {
        /// Boots `bin` (an `atlasd`) on an ephemeral port under a fresh home with extra
        /// environment, and waits until it answers.
        pub async fn spawn(bin: &Path, env: &[(&str, &str)]) -> Daemon {
        let home = tempfile::tempdir().unwrap();
        // Every daemon here gets an empty `ATLAS_SYNC_HOME` of its own, before the caller's
        // own environment (which wins, since `envs` is applied after this). That is the
        // home skill discovery reads, and no test may read the user's real
        // `~/.claude/skills` or write into their home.
        let sync_home = home.path().join("sync-home");
        std::fs::create_dir_all(&sync_home).unwrap();
        let child = Command::new(bin)
            .args(["--port", "0", "--home", home.path().to_str().unwrap(), "--no-embed"])
            .env("ATLAS_SYNC_HOME", &sync_home)
            .envs(env.iter().copied())
            .stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
        let daemon_json = home.path().join("daemon.json");
        // The wait has to cover a slow start under load, same budget as the readiness poll
        // below. A daemon that never writes the file fails here, where the reason is plain,
        // rather than as a "port unknown" error further down.
        let mut found = None;
        for _ in 0..READY_ATTEMPTS {
            if let Ok(s) = std::fs::read_to_string(&daemon_json) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let (Some(p), Some(t)) = (v["port"].as_u64(), v["token"].as_str()) { found = Some((p as u16, t.to_string())); break; }
                }
            }
            tokio::time::sleep(READY_STEP).await;
        }
        let (port, token) = found.unwrap_or_else(|| panic!("daemon.json had no port and token within {}", ready_budget()));
        let d = Daemon { child, port, token, home };
        let client = d.client();
        // A daemon that never answers fails here, where the reason is plain, rather than as
        // a connection error inside the test body.
        let mut up = false;
        for _ in 0..READY_ATTEMPTS {
            if client.get(format!("http://127.0.0.1:{port}/api/v1/status")).send().await.is_ok() { up = true; break; }
            tokio::time::sleep(READY_STEP).await;
        }
        assert!(up, "atlasd did not answer on port {port} within {}", ready_budget());
        d
    }
}

/// Builds a small git-backed fixture project: a Next.js/React `package.json`, a README,
/// a TypeScript source file, a gitignored `node_modules` dir, one commit, and an `origin`
/// remote. Copied from `atlas-core/tests/common/mod.rs`, which an integration test in
/// another crate cannot reach.
pub fn fixture_repo(dir: &Path) {
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
pub fn repo_free_tempdir() -> tempfile::TempDir {
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

pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Atlas Test").env("GIT_AUTHOR_EMAIL", "atlas-test@example.com")
        .env("GIT_COMMITTER_NAME", "Atlas Test").env("GIT_COMMITTER_EMAIL", "atlas-test@example.com")
        .stdout(Stdio::null()).stderr(Stdio::null()).status()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed");
}

/// One JSON-RPC round trip over streamable HTTP. The reply may come back as a bare JSON
/// body or as an SSE stream, so the raw text is returned and callers assert against it.
pub async fn rpc(c: &reqwest::Client, url: &str, session: &Option<String>, body: serde_json::Value) -> String {
    let mut req = c.post(url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json");
    if let Some(s) = session { req = req.header("mcp-session-id", s); }
    req.json(&body).send().await.unwrap().text().await.unwrap()
}

/// The JSON-RPC response inside an `rpc` reply, whether it arrived bare or as SSE frames.
pub fn rpc_json(body: &str) -> serde_json::Value {
    if let Ok(v) = serde_json::from_str(body) { return v; }
    let frame = body.lines().filter_map(|l| l.strip_prefix("data: "))
        .find_map(|d| serde_json::from_str::<serde_json::Value>(d).ok().filter(|v| v.get("result").is_some() || v.get("error").is_some()));
    frame.unwrap_or_else(|| panic!("no JSON-RPC result in reply: {body}"))
}

/// The text a `tools/call` returned, parsed back into JSON. Tool results carry their
/// payload as a JSON *string*, so it takes two parses to reach the data.
pub fn tool_json(body: &str) -> serde_json::Value {
    let reply = rpc_json(body);
    let text = reply["result"]["content"][0]["text"].as_str()
        .unwrap_or_else(|| panic!("tool result had no text content: {reply}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("tool result text was not JSON ({e}): {text}"))
}

/// What the stub model returns for any prompt: the two candidates the ingest tests
/// expect, neither confident enough to clear the default auto-accept bar of 1.0.
pub const STUB_CANDIDATES: &str = r#"[{"text":"the project uses bun","kind":"fact","tags":["tooling"],"confidence":0.9},{"text":"deploy target is fly.io","kind":"decision","tags":["infra"],"confidence":0.7}]"#;

/// An OpenAI-compatible chat endpoint in the test process, so the daemon's
/// extraction runs end to end without reaching the network. Returns its base url,
/// `/v1`, which is what `extraction.base_url` is set to. Answers every prompt with
/// `reply`, whatever it was.
pub async fn stub_llm_with_reply(reply: &str) -> String { stub_llm_with_delay(reply, Duration::ZERO).await }

/// The same stub, holding each request open for `delay` first. The worker drains the
/// queue one job at a time, so a slow first job is what keeps a second one queued long
/// enough for a test to change the daemon's settings underneath it.
pub async fn stub_llm_with_delay(reply: &str, delay: Duration) -> String {
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
pub async fn stub_llm() -> String { stub_llm_with_reply(STUB_CANDIDATES).await }

/// Polls a job until it stops running, for up to 10 s. The worker calls out to the
/// model, so the result is never there on the first read.
pub async fn wait_for_job(c: &reqwest::Client, base: &str, job_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let job: serde_json::Value = c.get(format!("{base}/jobs/{job_id}")).send().await.unwrap().json().await.unwrap();
        if job["status"] == "done" || job["status"] == "failed" { return job; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("job {job_id} did not finish within 10s");
}

// ---- board ----

/// True for a key like `ATL-12` or `ATLAS-7`: one or more uppercase letters or
/// digits, a dash, then one or more digits. Written by hand rather than pulling in
/// the `regex` crate for a single check.
pub fn looks_like_a_task_key(key: &str) -> bool {
    let Some((prefix, seq)) = key.rsplit_once('-') else { return false };
    !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && !seq.is_empty() && seq.chars().all(|c| c.is_ascii_digit())
}

// ---- workflows (Phase 9): runner, scheduler surface, routes, MCP ----

/// A linear graph: one manual trigger, one action per entry in `instructions` (named
/// `step0`, `step1`, ...), one output with the given flags. Every action runs as the
/// `desktop` fallback agent, which needs no saved `Agent` row.
pub fn linear_workflow_graph(instructions: &[&str], propose_memories: bool, file_tasks: bool) -> serde_json::Value {
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
pub async fn create_workflow(c: &reqwest::Client, base: &str, name: &str, instructions: &[&str], propose_memories: bool, file_tasks: bool) -> serde_json::Value {
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
pub async fn wait_for_run(c: &reqwest::Client, base: &str, run_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let detail: serde_json::Value = c.get(format!("{base}/runs/{run_id}")).send().await.unwrap().json().await.unwrap();
        if matches!(detail["run"]["status"].as_str(), Some("success") | Some("failed") | Some("cancelled")) {
            return detail;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("run {run_id} did not finish within 10s");
}

/// A stub model that records the `model` field of every request body, in order, and
/// answers `reply` to all of them.
pub async fn stub_llm_recording_models(reply: &str) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let models = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = models.clone();
    let content = reply.to_string();
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let content = content.clone();
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(body["model"].as_str().unwrap_or("").to_string());
                axum::Json(serde_json::json!({"choices": [{"message": {"content": content}}]}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1"), models)
}

// ---- plugin MCP tools (Phase 13b) ----

/// Stands in for the desktop app: opens the plugin channel, then answers every request
/// frame with `answer` (which sees the frame and returns the reply body). The join
/// handle finishes when the socket closes.
pub async fn plugin_app(
    d: &Daemon,
    answer: impl Fn(serde_json::Value) -> serde_json::Value + Send + 'static,
) -> (tokio::task::JoinHandle<()>, tokio::sync::oneshot::Sender<()>) {
    use futures_util::{SinkExt, StreamExt};
    let url = format!("ws://127.0.0.1:{}/api/v1/mcp/plugin-channel?token={}", d.port, d.token);
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

/// The CLI tests' harness: a temporary `ATLAS_HOME` and a free port, plus the promise that whatever
/// daemon the test started is stopped again. Stopping in `Drop` and not at the
/// end of the test body is the point: a failed assertion unwinds, and without
/// this an atlasd would outlive the run and hold its DuckDB file open.
pub struct CliDaemon {
    pub home: tempfile::TempDir,
    /// An empty stand-in for the user's own home. The daemon reads the agents'
    /// skills and MCP server configs from `ATLAS_SYNC_HOME`, so without this a
    /// test that lists either would read the developer's real `~/.claude.json`.
    pub sync_home: tempfile::TempDir,
    pub port: u16,
    atlas: PathBuf,
}

impl CliDaemon {
    /// A harness around the CLI binary at `atlas`, which starts and stops its own daemon.
    pub fn new(atlas: &Path) -> Self {
        let home = tempfile::tempdir().unwrap();
        let sync_home = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        Self { home, sync_home, port, atlas: atlas.to_path_buf() }
    }

    /// An `atlas` command already pointed at this home and port. `PATH` carries
    /// the build directory so the CLI can find the `atlasd` next to it.
    pub fn cmd(&self) -> Command {
        let exe = self.atlas.as_path();
        let mut c = Command::new(exe);
        c.env("ATLAS_HOME", self.home.path())
            .env("ATLAS_SYNC_HOME", self.sync_home.path())
            .env("ATLAS_PORT", self.port.to_string())
            .env("ATLAS_NO_EMBED", "1")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    exe.parent().unwrap().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        c
    }

    /// Asks the CLI to stop the daemon, then makes sure it is really gone.
    /// Both halves ignore their errors: this runs while a test may already be
    /// panicking, and there may be no daemon to stop at all.
    pub fn stop(&self) {
        let _ = self.cmd().args(["daemon", "stop"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        self.kill_leftover();
    }

    /// An HTTP client for the daemon this harness started, carrying the token its
    /// `daemon.json` holds (SEC-5) the way the CLI itself does. Call it once the daemon
    /// is up; before that there is no file and the client sends no token.
    pub fn client(&self) -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        let token = std::fs::read_to_string(self.home.path().join("daemon.json")).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["token"].as_str().map(str::to_string));
        if let Some(value) = token.and_then(|t| t.parse().ok()) {
            headers.insert("X-Atlas-Token", value);
        }
        reqwest::Client::builder().default_headers(headers).build().unwrap()
    }

    /// The fallback for a daemon that `daemon stop` could not reach: signal the
    /// pid `daemon.json` names, but only once `ps` agrees it is still an atlasd.
    /// The file outlives a crash, so the pid in it may have been recycled.
    fn kill_leftover(&self) {
        let Ok(text) = std::fs::read_to_string(self.home.path().join("daemon.json")) else { return };
        let Ok(info) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        let Some(pid) = info["pid"].as_u64() else { return };
        let is_atlasd = Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("atlasd"))
            .unwrap_or(false);
        if is_atlasd {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
}

impl Drop for CliDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}
