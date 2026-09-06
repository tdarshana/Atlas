//! The desktop plugin channel and plugin MCP tools.

mod common;
use common::*;
use std::time::Duration;

/// SEC-1: a browser page on another loopback port must not be able to become the app.
/// The loopback guard passes any `http://localhost:<port>` origin and WebSockets skip
/// CORS, so the channel refuses an upgrade whose `Origin` is not one of the app's own
/// (or the Vite dev server's); a request with no `Origin` is a non-browser client.
#[tokio::test]
async fn plugin_channel_refuses_foreign_loopback_origins() {
    use tokio_tungstenite::tungstenite::{ClientRequestBuilder, Error};
    let d = start().await;
    let uri: tokio_tungstenite::tungstenite::http::Uri = format!("ws://127.0.0.1:{}/api/v1/mcp/plugin-channel?token={}", d.port, d.token).parse().unwrap();

    let foreign = ClientRequestBuilder::new(uri.clone()).with_header("Origin", "http://localhost:8888");
    match tokio_tungstenite::connect_async(foreign).await {
        Err(Error::Http(resp)) => {
            assert_eq!(resp.status(), 403, "{resp:?}");
            let body = String::from_utf8_lossy(resp.body().as_deref().unwrap_or_default()).into_owned();
            assert!(body.contains("forbidden origin"), "{body}");
        }
        Ok(_) => panic!("a page on http://localhost:8888 opened the plugin channel"),
        Err(e) => panic!("expected a 403 handshake response, got {e}"),
    }

    let dev = ClientRequestBuilder::new(uri.clone()).with_header("Origin", "http://localhost:1420");
    let (mut socket, _) = tokio_tungstenite::connect_async(dev).await.expect("the Vite dev origin was refused");
    let _ = futures_util::SinkExt::close(&mut socket).await;

    let (mut socket, _) = tokio_tungstenite::connect_async(uri).await.expect("a request with no Origin was refused");
    let _ = futures_util::SinkExt::close(&mut socket).await;
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
    let c = d.client();

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

    let (app, close_app) = plugin_app(&d, |request| {
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
    let c = d.client();

    let registered = c.put(format!("{base}/mcp/plugin-tools/hello-world")).json(&serde_json::json!({
        "tools": [
            {"name": "ok_tool", "description": "Works.", "args": {"type": "object"}, "scope": "read"},
            {"name": "bad_tool", "description": "Fails.", "args": {"type": "object"}, "scope": "write"},
        ],
    })).send().await.unwrap();
    assert_eq!(registered.status(), 204);

    let (app, close_app) = plugin_app(&d, |request| {
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
    let (second_app, close_second) = plugin_app(&d, |request| serde_json::json!({"id": request["id"], "ok": true, "result": {}})).await;
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
