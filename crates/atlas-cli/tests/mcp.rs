// `common` also carries `fixture_repo` (and the `run` helper behind it), which this
// file, unlike the other integration test binaries, has no need for.
#[allow(dead_code)]
mod common;

use common::TestDaemon;
use rmcp::{
    ServiceExt,
    transport::{ConfigureCommandExt, TokioChildProcess},
};

/// Polls `GET /api/v1/mcp/status` until `clients` is non-empty (or empty, with
/// `want_empty`), for up to five seconds. Registering and unregistering both cross a
/// process boundary (an HTTP call the shim makes on its own async task), so the test
/// cannot assume either has landed the instant the client-side call that triggers it
/// returns.
async fn poll_clients(http: &reqwest::Client, port: u16, want_empty: bool) -> Vec<serde_json::Value> {
    for _ in 0..50 {
        if let Ok(r) = http.get(format!("http://127.0.0.1:{port}/api/v1/mcp/status")).send().await {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                if let Some(arr) = body["clients"].as_array() {
                    if arr.is_empty() == want_empty {
                        return arr.clone();
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("mcp status clients never became {}", if want_empty { "empty" } else { "non-empty" });
}

/// `atlas mcp` registers itself with the daemon once the `initialize` handshake
/// completes, and unregisters (best effort) once its client disconnects. This drives
/// the real stdio shim as a child process, speaking the real MCP handshake to it
/// through `TokioChildProcess`, and checks both ends through `GET /api/v1/mcp/status`.
///
/// It is also what guards the bare form of the command: `atlas mcp` grew subcommands in
/// Phase 16, and with no subcommand it still has to be the stdio shim every configured
/// Claude Code and Codex launches.
#[tokio::test]
async fn stdio_shim_registers_with_the_daemon_and_appears_in_mcp_status() {
    let daemon = TestDaemon::new();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_atlas"));
    let path = format!("{}:{}", exe.parent().unwrap().display(), std::env::var("PATH").unwrap_or_default());
    let transport = TokioChildProcess::new(tokio::process::Command::new(exe).configure(|cmd| {
        cmd.arg("mcp")
            .env("ATLAS_HOME", daemon.home.path())
            .env("ATLAS_PORT", daemon.port.to_string())
            .env("ATLAS_NO_EMBED", "1")
            .env("PATH", path);
    }))
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let http = reqwest::Client::new();
    let clients = poll_clients(&http, daemon.port, false).await;
    assert_eq!(clients.len(), 1, "{clients:?}");
    assert_eq!(clients[0]["transport"], "stdio", "{clients:?}");
    assert!(clients[0]["client_name"].as_str().is_some_and(|n| !n.is_empty()), "{clients:?}");
    assert_eq!(clients[0]["tool_calls"], 0, "{clients:?}");

    client.cancel().await.unwrap();
    poll_clients(&http, daemon.port, true).await;

    daemon.stop();
}

/// `atlas mcp servers` and `atlas mcp check` (Phase 16), against a Cursor config the
/// test writes into a home of its own: no test may read the user's real `~/.cursor` or
/// `~/.claude.json`, so the daemon is started with `ATLAS_SYNC_HOME` pointed at a temp
/// directory.
///
/// The checked server is the real `atlas mcp` shim, pointed at this same daemon, so the
/// check drives the whole path: spawn the process, speak MCP over its stdio, and read
/// back the tool list.
#[tokio::test]
async fn mcp_servers_lists_and_check_starts_a_real_server() {
    let daemon = TestDaemon::new();
    let sync_home = tempfile::tempdir().unwrap();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_atlas"));
    std::fs::create_dir_all(sync_home.path().join(".cursor")).unwrap();
    std::fs::write(
        sync_home.path().join(".cursor/mcp.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {
                "atlas-shim": {
                    "command": exe.to_str().unwrap(),
                    "args": ["mcp"],
                    "env": {
                        "ATLAS_HOME": daemon.home.path().to_str().unwrap(),
                        "ATLAS_PORT": daemon.port.to_string(),
                        "ATLAS_NO_EMBED": "1",
                    }
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listed = daemon.cmd().env("ATLAS_SYNC_HOME", sync_home.path()).args(["mcp", "servers"]).output().unwrap();
    assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
    let table = String::from_utf8_lossy(&listed.stdout).into_owned();
    assert!(table.contains("cursor:user:atlas-shim"), "{table}");
    assert!(table.contains("atlas"), "Atlas is one server among them: {table}");
    // The environment keys are shown; their values never are.
    assert!(!table.contains(daemon.home.path().to_str().unwrap()), "an env value reached the listing: {table}");

    let checked = daemon.cmd().env("ATLAS_SYNC_HOME", sync_home.path()).args(["mcp", "check", "cursor:user:atlas-shim"]).output().unwrap();
    assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
    let result: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    let tools: Vec<&str> = result["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tools.contains(&"memory_remember") && tools.contains(&"memory_search"), "{tools:?}");
    assert!(result["server_name"].as_str().is_some_and(|n| !n.is_empty()), "{result}");
    assert!(result["protocol_version"].as_str().is_some(), "{result}");
}
