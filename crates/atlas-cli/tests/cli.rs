mod common;

use common::TestDaemon;

#[test]
fn remember_and_recall_via_cli_starting_daemon() {
    let daemon = TestDaemon::new();
    let out = daemon.cmd().args(["remember", "the deploy target is fly.io", "--kind", "decision", "--tag", "infra"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = daemon.cmd().args(["recall", "where do we deploy"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("fly.io"), "{s}");
    let out = daemon.cmd().args(["daemon", "status"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("memories_active"));
    assert!(daemon.cmd().args(["daemon", "stop"]).status().unwrap().success());
}

/// The whole project workflow from the command line: connect a repository, put an
/// agent on its roster, write the subagent file into the repository, confirm a
/// second sync has nothing to do, and export and re-import the library.
#[test]
fn project_agent_sync_export_and_import_round_trip() {
    let daemon = TestDaemon::new();
    let repo = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let run = |args: &[&str]| {
        let out = daemon.cmd().args(args).output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let run_stdin = |args: &[&str], input: &str| {
        use std::io::Write;
        let mut child = daemon.cmd().args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let repo_path = repo.path().to_str().unwrap();

    let (code, out, err) = run(&["project", "connect", repo_path]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("\"name\""), "connect should print the project as JSON: {out}");
    let connected: serde_json::Value = serde_json::from_str(&out).unwrap();
    let project_id = connected["id"].as_str().unwrap().to_string();
    let (code, out, err) = run(&["project", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("NAME"), "list should print a table: {out}");
    assert!(out.contains("ID"), "list should have an ID column: {out}");
    assert!(out.contains(&project_id[..8]), "list should print the project's short id: {out}");

    // Agents are created through the API (the CLI only lists and reads them) and
    // reach the repository through the project's roster.
    let api = format!("http://127.0.0.1:{}/api/v1", daemon.port);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let add_agent = |name: &str, instructions: &str| -> String {
        rt.block_on(async {
            let c = daemon.client();
            let created: serde_json::Value = c.post(format!("{api}/agents"))
                .json(&serde_json::json!({"name": name, "role": "Reviews", "instructions": instructions, "tags": ["qa"]}))
                .send().await.unwrap().json().await.unwrap();
            let roster: Vec<serde_json::Value> = c.get(format!("{api}/projects/{project_id}/agents")).send().await.unwrap().json().await.unwrap();
            let mut entries: Vec<serde_json::Value> = roster.iter().enumerate()
                .map(|(i, r)| serde_json::json!({"persona_id": r["persona_id"], "is_default": r["is_default"], "position": i}))
                .collect();
            entries.push(serde_json::json!({"persona_id": created["id"], "is_default": entries.is_empty(), "position": entries.len()}));
            let r = c.put(format!("{api}/projects/{project_id}/agents")).json(&entries).send().await.unwrap();
            assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
            created["slug"].as_str().unwrap().to_string()
        })
    };
    assert_eq!(add_agent("Reviewer", "Review the diff and report only real defects.\n"), "reviewer");

    let (code, out, err) = run(&["sync", "--project", repo_path]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("created"), "sync should report its tally: {out}");
    let agent_file = repo.path().join(".claude/agents/reviewer.md");
    assert!(agent_file.exists(), "sync should have written {}", agent_file.display());
    assert!(std::fs::read_to_string(&agent_file).unwrap().contains("Review the diff"));

    let (code, out, err) = run(&["sync", "--project", repo_path, "--check"]);
    assert_eq!(code, 0, "a second sync should have nothing to do: {out}{err}");
    assert!(out.contains("unchanged"), "check should list each op: {out}");

    // An agent put on the roster but not synced is what --check exists to catch, and
    // its non-zero exit is what lets a hook or a CI job fail on the difference.
    assert_eq!(add_agent("Linter", "Lint it.\n"), "linter");
    let (code, out, err) = run(&["sync", "--project", repo_path, "--check"]);
    assert_eq!(code, 1, "check should fail while a change is pending: {out}{err}");
    assert!(
        out.lines().any(|l| l.starts_with("create") && l.contains(".claude/agents/linter.md")),
        "check should name the file it would create: {out}"
    );

    // `atlas agent` reads the library; `atlas persona` is its alias for one release.
    let (code, out, err) = run(&["agent", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("reviewer") && out.contains("linter"), "agent list should show both slugs: {out}");
    let (code, out, err) = run(&["agent", "show", "reviewer"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Review the diff") && out.contains("\"qa\""), "agent show prints the bundle: {out}");
    let (code, out, _) = run(&["persona", "list"]);
    assert_eq!(code, 0);
    assert!(out.contains("reviewer"), "the persona alias still lists agents: {out}");

    // Practices and workflows, with the practice body read from standard input.
    let (code, _, err) = run_stdin(&["practice", "save", "commits", "--body-file", "-", "--project", repo_path], "Imperative mood, one change per commit.\n");
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["practice", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("commits"), "practice list should show the saved practice: {out}");
    let (code, out, err) = run(&["practice", "show", "commits"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Imperative mood"), "the body should have been read from stdin: {out}");

    // Workflows (Phase 9) are a graph-shaped API of their own now, not a document:
    // `atlas workflow` no longer has `save`/`delete`, just `list`/`show`/`run`/`runs`/
    // `log`/`cancel`. There is none to list yet, so this only checks the command wires
    // up and prints its table header.
    let (code, out, err) = run(&["workflow", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("NAME"), "workflow list should print a table: {out}");

    let (code, out, err) = run(&["project", "show", repo_path]);
    assert_eq!(code, 0, "{err}");
    let practices = out.split("\"practices\"").nth(1).unwrap_or_else(|| panic!("project show should list practices: {out}"));
    assert!(practices.contains("\"name\": \"commits\""), "the project-scoped practice should appear in the project context: {out}");

    // A memory gives the export a `memories.jsonl` line, and lets the re-import
    // below prove that an already-present memory is not stored a second time.
    let (code, _, err) = run(&["remember", "the deploy target is fly.io", "--kind", "decision"]);
    assert_eq!(code, 0, "{err}");

    let dump = work.path().join("dump");
    let (code, _, err) = run(&["export", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(!dump.join("agents").exists(), "agents live in the store and reach repositories through sync, not the export bundle");
    assert!(dump.join("practices/commits.md").exists(), "export should write practices/commits.md");
    assert!(std::fs::read_to_string(dump.join("memories.jsonl")).unwrap().contains("fly.io"));

    let (code, _, err) = run(&["import", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["practice", "show", "commits"]);
    assert_eq!(code, 0, "import should have kept the practice: {err}");
    assert!(out.contains("Imperative mood"), "{out}");
    let (_, out, _) = run(&["recall", "deploy target"]);
    assert_eq!(out.lines().filter(|l| l.contains("fly.io")).count(), 1, "import should not duplicate an existing memory: {out}");
}

/// The Claude Code Stop hook runs inside someone else's turn: with extraction off
/// it has to say so and still exit 0, or Claude Code reports the turn as failed.
#[test]
fn claude_code_stop_hook_is_quiet_when_extraction_is_off() {
    use std::io::Write;
    let daemon = TestDaemon::new();
    let work = tempfile::tempdir().unwrap();
    let transcript = work.path().join("session.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","message":{"content":"we deploy to fly.io"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Noted."}]}}"#,
            "\n",
        ),
    )
    .unwrap();
    let payload = serde_json::json!({
        "session_id": "s",
        "transcript_path": transcript,
        "cwd": work.path(),
        "hook_event_name": "Stop",
        "stop_hook_active": false,
    })
    .to_string();

    let mut child = daemon
        .cmd()
        .args(["ingest", "--tool", "claude-code", "--hook-stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "a hook must never fail the turn: {err}");
    assert!(err.contains("extraction is disabled"), "the hook should still say why nothing was queued: {err}");
}

/// The failure the hook exists to survive: no daemon, and none that can start. Port 1
/// is privileged, so atlasd exits at once and `ensure_daemon` reports it - through a
/// hook that still has to be exit 0 with the reason on stderr, or Claude Code marks
/// every turn as having a failed Stop hook.
#[test]
fn hook_mode_stays_quiet_when_the_daemon_cannot_be_reached() {
    use std::io::Write;
    let daemon = TestDaemon::new();
    let work = tempfile::tempdir().unwrap();
    let transcript = work.path().join("session.jsonl");
    std::fs::write(&transcript, "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n").unwrap();
    let payload = serde_json::json!({ "transcript_path": transcript, "cwd": work.path() }).to_string();

    let mut child = daemon
        .cmd()
        .env("ATLAS_PORT", "1")
        .args(["ingest", "--tool", "claude-code", "--hook-stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "an unreachable daemon must not fail the turn: {err}");
    assert!(err.starts_with("atlas: "), "the hook should still say what went wrong: {err:?}");
}

/// The same daemon, asked from a shell rather than a hook, has to fail instead:
/// a person typing `atlas ingest` wants a non-zero exit when nothing was queued.
#[test]
fn ingest_from_a_pipe_fails_when_extraction_is_off() {
    use std::io::Write;
    let daemon = TestDaemon::new();
    // Start the daemon first, so what this test observes is the refusal and not a
    // handshake that lost a race against a loaded machine.
    let started = daemon.cmd().args(["daemon", "start"]).output().unwrap();
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    let mut child = daemon
        .cmd()
        .args(["ingest", "--tool", "test"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // The child can refuse and exit before the parent finishes writing, which closes
    // the pipe: that is the very outcome being asserted, so a broken pipe here is not
    // a failure. Every other write error still is.
    if let Err(e) = child.stdin.take().unwrap().write_all(b"user: we deploy to fly.io\n") {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "unexpected stdin write error: {e}");
    }
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "an interactive ingest should fail: {err}");
    assert!(err.contains("extraction is disabled"), "{err}");
}

/// `atlas mcp` is how editors reach Atlas, so the stdio shim must complete an MCP
/// handshake and list the four tools without a client ever touching the HTTP API.
#[test]
fn mcp_stdio_shim_lists_tools() {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let daemon = TestDaemon::new();
    let mut child = daemon.cmd().arg("mcp")
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null())
        .spawn().unwrap();

    // Read on a thread so a shim that never answers costs the timeout, not the whole run.
    let out = BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || { for line in out.lines().map_while(Result::ok) { if tx.send(line).is_err() { break; } } });

    let mut stdin = child.stdin.take().unwrap();
    for msg in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ] { writeln!(stdin, "{msg}").unwrap(); }
    stdin.flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut listing = None;
    while listing.is_none() && Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(line) if line.contains("\"tools\":[") => listing = Some(line), // the list, not initialize's capabilities
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let listing = listing.expect("atlas mcp produced no tools/list response within 30s");
    for t in ["memory_remember", "memory_search", "memory_forget", "status"] { assert!(listing.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {listing}"); }
}

/// `atlas agent list` prints the library as a table, `atlas agent show` prints the
/// bundle as JSON, and `atlas project agents <root>` reads and edits the roster with
/// `--add`, `--default` and `--remove`. Agents are seeded over the HTTP API, since the
/// CLI has no `agent create`.
#[test]
fn agent_list_show_and_project_roster_via_the_cli() {
    let daemon = TestDaemon::new();
    assert!(daemon.cmd().args(["daemon", "start"]).status().unwrap().success());
    let repo = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let repo_path = repo.path().to_str().unwrap();
    let run = |args: &[&str]| {
        let out = daemon.cmd().args(args).output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };

    let base = format!("http://127.0.0.1:{}/api/v1", daemon.port);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let c = daemon.client();
        for (name, role) in [("Mobile Developer", "Builds the app"), ("Security Reviewer", "Reads every diff")] {
            let r = c.post(format!("{base}/personas")).json(&serde_json::json!({"name": name, "role": role, "summary": "One paragraph.", "skills": ["plugin:gone/gone/gone"]})).send().await.unwrap();
            assert_eq!(r.status(), 201, "{name}");
        }
    });

    let (code, out, err) = run(&["agent", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("NAME") && out.contains("SLUG") && out.contains("ROLE"), "list should print a table: {out}");
    assert!(out.contains("mobile-developer") && out.contains("Reads every diff"), "{out}");

    let (code, out, err) = run(&["agent", "show", "mobile developer"]);
    assert_eq!(code, 0, "{err}");
    let bundle: serde_json::Value = serde_json::from_str(&out).expect("show prints the bundle as JSON");
    assert_eq!(bundle["persona"]["slug"], "mobile-developer", "{out}");
    assert!(bundle["warnings"].as_array().unwrap().iter().any(|w| w == "skill plugin:gone/gone/gone not found"), "{out}");
    let (code, _, err) = run(&["agent", "show", "nobody"]);
    assert_ne!(code, 0);
    assert!(err.contains("not found"), "{err}");

    let (code, _, err) = run(&["project", "connect", repo_path]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["project", "agents", repo_path]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("mobile-developer"), "an empty roster lists nothing: {out}");

    let (code, out, err) = run(&["project", "agents", repo_path, "--add", "mobile-developer", "--add", "Security Reviewer", "--default", "security-reviewer"]);
    assert_eq!(code, 0, "{err}");
    let mobile = out.lines().find(|l| l.contains("mobile-developer")).unwrap_or_else(|| panic!("{out}"));
    let security = out.lines().find(|l| l.contains("security-reviewer")).unwrap_or_else(|| panic!("{out}"));
    assert!(security.contains("yes") && !mobile.contains("yes"), "the default column marks one row: {out}");

    let (code, out, err) = run(&["project", "personas", repo_path, "--remove", "mobile-developer"]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("mobile-developer") && out.contains("security-reviewer"), "{out}");
    let (code, _, err) = run(&["project", "personas", repo_path, "--default", "mobile-developer"]);
    assert_ne!(code, 0, "a default must be on the roster");
    assert!(err.contains("not on the roster"), "{err}");
}

/// `atlas workflow` has no `save`: a workflow is a graph, seeded here straight against
/// the HTTP API the way a GUI would. `atlas workflow list` must show it, and `atlas
/// workflow run NAME` must start it and print the run number `POST .../run` answered
/// with, even though the run itself will fail (extraction is not configured).
#[test]
fn workflow_list_and_run_via_the_cli() {
    let daemon = TestDaemon::new();
    assert!(daemon.cmd().args(["daemon", "start"]).status().unwrap().success());

    let base = format!("http://127.0.0.1:{}/api/v1", daemon.port);
    let graph = serde_json::json!({
        "nodes": [
            {"id": "t", "kind": "trigger", "position": {"x": 0.0, "y": 0.0}, "data": {"kind": "manual"}},
            {"id": "a", "kind": "action", "position": {"x": 240.0, "y": 0.0}, "data": {"name": "step", "instructions": "do it", "agent": "desktop", "practices": [], "memories": null}},
            {"id": "o", "kind": "output", "position": {"x": 480.0, "y": 0.0}, "data": {"propose_memories": false, "file_tasks": false}},
        ],
        "edges": [
            {"id": "t-a", "source": "t", "target": "a"},
            {"id": "a-o", "source": "a", "target": "o"},
        ],
    });
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let c = daemon.client();
        let r = c
            .post(format!("{base}/workflows"))
            .json(&serde_json::json!({"name": "cli-release", "trigger": {"kind": "manual"}, "graph": graph, "enabled": true}))
            .send()
            .await
            .unwrap();
        let status = r.status();
        assert_eq!(status, 201, "{}", r.text().await.unwrap());
    });

    let out = daemon.cmd().args(["workflow", "list"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let listed = String::from_utf8_lossy(&out.stdout);
    assert!(listed.contains("cli-release"), "{listed}");

    let out = daemon.cmd().args(["workflow", "run", "cli-release"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let ran = String::from_utf8_lossy(&out.stdout);
    assert!(ran.contains("run #1"), "{ran}");

    assert!(daemon.cmd().args(["daemon", "stop"]).status().unwrap().success());
}

/// `atlas workflow run --wait` must not block forever: pointed at a model endpoint
/// that accepts the connection and never answers, `--timeout 1` gives up after about a
/// second, exits non-zero, and names the run it was waiting on.
#[test]
fn workflow_run_wait_times_out_rather_than_hanging_forever() {
    let daemon = TestDaemon::new();
    assert!(daemon.cmd().args(["daemon", "start"]).status().unwrap().success());

    // Accepts the connection and then just holds it open, writing nothing back: the
    // run stays `running` for far longer than the CLI's short --timeout below.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stub_addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            std::thread::sleep(std::time::Duration::from_secs(60));
            drop(stream);
        }
    });

    let base = format!("http://127.0.0.1:{}/api/v1", daemon.port);
    let graph = serde_json::json!({
        "nodes": [
            {"id": "t", "kind": "trigger", "position": {"x": 0.0, "y": 0.0}, "data": {"kind": "manual"}},
            {"id": "a", "kind": "action", "position": {"x": 240.0, "y": 0.0}, "data": {"name": "step", "instructions": "do it", "agent": "desktop", "practices": [], "memories": null}},
            {"id": "o", "kind": "output", "position": {"x": 480.0, "y": 0.0}, "data": {"propose_memories": false, "file_tasks": false}},
        ],
        "edges": [
            {"id": "t-a", "source": "t", "target": "a"},
            {"id": "a-o", "source": "a", "target": "o"},
        ],
    });
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let c = daemon.client();
        let put = c
            .put(format!("{base}/settings"))
            .json(&serde_json::json!({"extraction.enabled": true, "extraction.base_url": format!("http://{stub_addr}"), "extraction.model": "stub"}))
            .send()
            .await
            .unwrap();
        assert_eq!(put.status(), 200);
        let r = c
            .post(format!("{base}/workflows"))
            .json(&serde_json::json!({"name": "hangs", "trigger": {"kind": "manual"}, "graph": graph, "enabled": true}))
            .send()
            .await
            .unwrap();
        let status = r.status();
        assert_eq!(status, 201, "{}", r.text().await.unwrap());
    });

    let out = daemon.cmd().args(["workflow", "run", "hangs", "--wait", "--timeout", "1"]).output().unwrap();
    assert!(!out.status.success(), "a timed-out wait must exit non-zero");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("timed out waiting for run"), "{err}");

    assert!(daemon.cmd().args(["daemon", "stop"]).status().unwrap().success());
}

/// A pre-Phase-9 export directory's `workflows/*.md` file (the old Markdown-document
/// shape `atlas export` no longer writes, since `DocKind::Workflow` docs were retired
/// by the Phase 9 migration) is not silently dropped or refused on import: `atlas
/// import` turns it into a manual single-action workflow, the same graph shape the
/// daemon's own startup doc-to-workflow migration builds.
#[test]
fn import_converts_a_pre_migration_workflow_document_into_a_real_workflow() {
    let daemon = TestDaemon::new();
    assert!(daemon.cmd().args(["daemon", "start"]).status().unwrap().success());

    let dump = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dump.path().join("workflows")).unwrap();
    std::fs::write(dump.path().join("workflows/release.md"), "---\nname: release\ntags: ops\n---\n\nTag, build, publish.\n").unwrap();

    let out = daemon.cmd().args(["import", dump.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let out = daemon.cmd().args(["workflow", "show", "release"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let shown: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(shown["name"], "release", "{shown}");
    assert_eq!(shown["trigger"]["kind"], "manual", "{shown}");
    let nodes = shown["graph"]["nodes"].as_array().unwrap();
    let action = nodes.iter().find(|n| n["kind"] == "action").expect("a single action node");
    assert_eq!(action["data"]["instructions"], "Tag, build, publish.", "{shown}");
    assert_eq!(action["data"]["agent"], "desktop", "{shown}");

    assert!(daemon.cmd().args(["daemon", "stop"]).status().unwrap().success());
}
