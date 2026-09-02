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
