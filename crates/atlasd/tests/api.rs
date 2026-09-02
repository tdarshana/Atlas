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
    let daemon_json = std::fs::read_to_string(d._home.path().join("daemon.json")).unwrap();
    assert!(daemon_json.contains(&format!("\"port\":{}", d.port)) || daemon_json.contains(&format!("\"port\": {}", d.port)));
}

#[tokio::test]
async fn mcp_over_http_lists_and_calls_tools() {
    let d = start().await;
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let c = reqwest::Client::new();
    let init = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    assert!(init.status().is_success(), "initialize failed: {}", init.status());
    let session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
    let mut req = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json");
    if let Some(s) = &session { req = req.header("mcp-session-id", s); }
    let _ = req.json(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})).send().await.unwrap();
    let mut req = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json");
    if let Some(s) = &session { req = req.header("mcp-session-id", s); }
    let body = req.json(&serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).send().await.unwrap().text().await.unwrap();
    for t in ["remember", "recall", "forget", "status"] { assert!(body.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {body}"); }
    let mut req = c.post(&url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json");
    if let Some(s) = &session { req = req.header("mcp-session-id", s); }
    let body = req.json(&serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remember","arguments":{"text":"mcp round trip works","kind":"insight"}}})).send().await.unwrap().text().await.unwrap();
    assert!(body.contains("mcp round trip works"), "{body}");
}
