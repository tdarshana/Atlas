mod common;

use std::process::Command;
fn atlas() -> Command { Command::new(env!("CARGO_BIN_EXE_atlas")) }

#[test]
fn remember_and_recall_via_cli_starting_daemon() {
    let home = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let atlasd_dir = std::path::Path::new(env!("CARGO_BIN_EXE_atlas")).parent().unwrap().to_path_buf();
    let env = |c: &mut Command| { c.env("ATLAS_HOME", home.path()).env("ATLAS_PORT", port.to_string()).env("ATLAS_NO_EMBED", "1").env("PATH", format!("{}:{}", atlasd_dir.display(), std::env::var("PATH").unwrap_or_default())); };
    let mut c = atlas(); env(&mut c);
    let out = c.args(["remember", "the deploy target is fly.io", "--kind", "decision", "--tag", "infra"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut c = atlas(); env(&mut c);
    let out = c.args(["recall", "where do we deploy"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("fly.io"), "{s}");
    let mut c = atlas(); env(&mut c);
    let out = c.args(["daemon", "status"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("memories_active"));
    let mut c = atlas(); env(&mut c);
    assert!(c.args(["daemon", "stop"]).status().unwrap().success());
}

/// The whole project workflow from the command line: connect a repository, save an
/// agent, write the agent files into the repository, confirm a second sync has
/// nothing to do, and export and re-import the library.
#[test]
fn project_agent_sync_export_and_import_round_trip() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let atlasd_dir = std::path::Path::new(env!("CARGO_BIN_EXE_atlas")).parent().unwrap().to_path_buf();
    let env = |c: &mut Command| { c.env("ATLAS_HOME", home.path()).env("ATLAS_PORT", port.to_string()).env("ATLAS_NO_EMBED", "1").env("PATH", format!("{}:{}", atlasd_dir.display(), std::env::var("PATH").unwrap_or_default())); };
    let run = |args: &[&str]| {
        let mut c = atlas();
        env(&mut c);
        let out = c.args(args).output().unwrap();
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
    let (code, _, err) = run(&["agent", "save", "reviewer", "--description", "Reviews", "--instructions-file", instructions.to_str().unwrap()]);
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

    // A memory gives the export a `memories.jsonl` line, and lets the re-import
    // below prove that an already-present memory is not stored a second time.
    let (code, _, err) = run(&["remember", "the deploy target is fly.io", "--kind", "decision"]);
    assert_eq!(code, 0, "{err}");

    let dump = work.path().join("dump");
    let (code, _, err) = run(&["export", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(dump.join("agents/reviewer.md").exists(), "export should write agents/reviewer.md");
    assert!(std::fs::read_to_string(dump.join("memories.jsonl")).unwrap().contains("fly.io"));

    let (code, _, err) = run(&["agent", "delete", "reviewer"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = run(&["import", dump.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["agent", "show", "reviewer"]);
    assert_eq!(code, 0, "import should have restored the agent: {err}");
    assert!(out.contains("Review the diff"), "{out}");
    let (_, out, _) = run(&["recall", "deploy target"]);
    assert_eq!(out.lines().filter(|l| l.contains("fly.io")).count(), 1, "import should not duplicate an existing memory: {out}");

    let mut c = atlas(); env(&mut c);
    assert!(c.args(["daemon", "stop"]).status().unwrap().success());
}

/// `atlas mcp` is how editors reach Atlas, so the stdio shim must complete an MCP
/// handshake and list the four tools without a client ever touching the HTTP API.
#[test]
fn mcp_stdio_shim_lists_tools() {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let home = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let atlasd_dir = std::path::Path::new(env!("CARGO_BIN_EXE_atlas")).parent().unwrap().to_path_buf();
    let env = |c: &mut Command| { c.env("ATLAS_HOME", home.path()).env("ATLAS_PORT", port.to_string()).env("ATLAS_NO_EMBED", "1").env("PATH", format!("{}:{}", atlasd_dir.display(), std::env::var("PATH").unwrap_or_default())); };

    let mut c = atlas(); env(&mut c);
    let mut child = c.arg("mcp")
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

    let mut c = atlas(); env(&mut c);
    assert!(c.args(["daemon", "stop"]).status().unwrap().success());
}
