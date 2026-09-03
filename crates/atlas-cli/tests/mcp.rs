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
