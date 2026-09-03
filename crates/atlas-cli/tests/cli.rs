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

/// The whole project workflow from the command line: connect a repository, save an
/// agent, write the agent files into the repository, confirm a second sync has
/// nothing to do, and export and re-import the library.
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
    let (code, out, err) = run(&["project", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("NAME"), "list should print a table: {out}");

    let instructions = work.path().join("reviewer.md");
    std::fs::write(&instructions, "Review the diff and report only real defects.\n").unwrap();
    let (code, _, err) = run(&["agent", "save", "reviewer", "--description", "Reviews", "--instructions-file", instructions.to_str().unwrap(), "--tag", "qa"]);
    assert_eq!(code, 0, "{err}");

    let (code, out, err) = run(&["sync", "--project", repo_path]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("created"), "sync should report its tally: {out}");
    let agent_file = repo.path().join(".claude/agents/reviewer.md");
    assert!(agent_file.exists(), "sync should have written {}", agent_file.display());
    assert!(std::fs::read_to_string(&agent_file).unwrap().contains("Review the diff"));

    let (code, out, err) = run(&["sync", "--project", repo_path, "--check"]);
    assert_eq!(code, 0, "a second sync should have nothing to do: {out}{err}");
    assert!(out.contains("unchanged"), "check should list each op: {out}");

    // An agent saved but not synced is what --check exists to catch, and its
    // non-zero exit is what lets a hook or a CI job fail on the difference.
    let (code, _, err) = run(&["agent", "save", "linter", "--description", "Lints", "--instructions-file", instructions.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["sync", "--project", repo_path, "--check"]);
    assert_eq!(code, 1, "check should fail while a change is pending: {out}{err}");
    assert!(
        out.lines().any(|l| l.starts_with("create") && l.contains(".claude/agents/linter.md")),
        "check should name the file it would create: {out}"
    );

    // Practices and workflows, with the practice body read from standard input.
    let (code, _, err) = run_stdin(&["practice", "save", "commits", "--body-file", "-", "--project", repo_path], "Imperative mood, one change per commit.\n");
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["practice", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("commits"), "practice list should show the saved practice: {out}");
    let (code, out, err) = run(&["practice", "show", "commits"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Imperative mood"), "the body should have been read from stdin: {out}");

    let release = work.path().join("release.md");
    std::fs::write(&release, "Tag, build, publish.\n").unwrap();
    let (code, _, err) = run(&["workflow", "save", "release", "--body-file", release.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let (_, out, _) = run(&["workflow", "list"]);
    assert!(out.contains("release"), "workflow list should show the saved workflow: {out}");
    let (code, _, err) = run(&["workflow", "delete", "release"]);
    assert_eq!(code, 0, "{err}");
    let (_, out, _) = run(&["workflow", "list"]);
    assert!(!out.contains("release"), "delete should have removed the workflow: {out}");

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
    assert!(dump.join("agents/reviewer.md").exists(), "export should write agents/reviewer.md");
    assert!(dump.join("agents/linter.md").exists(), "export should write agents/linter.md");
    assert!(dump.join("practices/commits.md").exists(), "export should write practices/commits.md");
    assert!(std::fs::read_to_string(dump.join("memories.jsonl")).unwrap().contains("fly.io"));

    // Exporting again over the same directory has to drop the file of an agent
    // that has since been deleted, or the next import would bring it back.
    let (code, _, err) = run(&["agent", "delete", "linter"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = run(&["export", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(!dump.join("agents/linter.md").exists(), "a second export should prune the deleted agent's file");
    assert!(dump.join("agents/reviewer.md").exists(), "a second export should keep the agents that remain");

    let (code, _, err) = run(&["agent", "delete", "reviewer"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = run(&["import", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["agent", "show", "reviewer"]);
    assert_eq!(code, 0, "import should have restored the agent: {err}");
    assert!(out.contains("Review the diff"), "{out}");
    assert!(out.contains("\"qa\""), "import should restore the agent's tags: {out}");
    let (_, out, _) = run(&["agent", "list"]);
    assert!(!out.contains("linter"), "import should not resurrect an agent the export pruned: {out}");
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
    for t in ["remember", "recall", "forget", "status"] { assert!(listing.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {listing}"); }
}
