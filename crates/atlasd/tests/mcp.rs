//! The MCP surface over HTTP: tools, status, clients, per-project gating and the workflow tools.

mod common;
use common::*;
use std::time::Duration;

#[tokio::test]
async fn mcp_over_http_lists_and_calls_tools() {
    let d = start().await;
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let api = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
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
              "agent_get", "agent_use", "practice_list", "practice_get", "workflow_list", "workflow_get"] {
        assert!(body.contains(&format!("\"name\":\"{t}\"")), "tools/list missing {t}: {body}");
    }

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_remember","arguments":{"text":"mcp round trip works","kind":"insight"}}})).await;
    assert!(body.contains("mcp round trip works"), "{body}");

    // Agents reach MCP through the same store the JSON API writes, so create one there
    // and expect `agent_list` to answer with it.
    let saved = c.post(format!("{api}/agents")).json(&serde_json::json!({
        "name": "Reviewer", "role": "Reviews diffs for regressions",
        "instructions": "Read the diff and name the riskiest change.", "tags": ["review"],
    })).send().await.unwrap();
    assert_eq!(saved.status(), 201, "create agent failed");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"agent_list","arguments":{}}})).await;
    assert!(body.contains("reviewer"), "agent_list missing the agent: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":6,"method":"prompts/list"})).await;
    assert!(body.contains("atlas.bootstrap"), "prompts/list missing the bootstrap prompt: {body}");

    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":7,"method":"prompts/get","params":{"name":"atlas.bootstrap"}})).await;
    assert!(body.contains("memory_search"), "prompts/get lost the bootstrap text: {body}");
    let body = rpc(&c, &url, &session, serde_json::json!({"jsonrpc":"2.0","id":8,"method":"prompts/get","params":{"name":"reviewer"}})).await;
    assert!(body.contains("unknown prompt"), "an agent is not a prompt any more: {body}");

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
    let c = d.client();

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
    let c = d.client();

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
    let c = d.client();
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

/// `workflow_run` and `workflow_status` are listed, a run started by name is followed
/// to success, and `workflow_run` on a name nothing was ever saved under is
/// `invalid_params` (JSON-RPC -32602), not an internal error.
#[tokio::test]
async fn mcp_workflow_tools_run_and_report_status() {
    let stub = stub_llm_with_reply("ok").await;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let url = format!("http://127.0.0.1:{}/mcp", d.port);
    let c = d.client();
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
    let c = d.client();
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
