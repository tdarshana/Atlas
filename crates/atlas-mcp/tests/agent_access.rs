//! A project's `agent_access` seen from the other end of a real MCP session.
//!
//! The rules live in the backend rather than in the tool router, because MCP also
//! reaches the daemon over HTTP, where the shim has nothing to enforce with. These
//! tests drive the tools through an in-memory client/server pair against a
//! `LocalBackend`, so what they check is what an agent actually gets back.

use std::sync::Arc;

use atlas_core::backend::{Backend, LocalBackend};
use atlas_core::models::*;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceExt};
use rmcp::RoleClient;
use atlas_mcp::AtlasMcp;

/// A backend on a fresh `ATLAS_HOME`, plus a project rooted at `root`.
async fn backend(home: &std::path::Path, root: &std::path::Path) -> (Arc<LocalBackend>, Project) {
    let paths = atlas_core::paths::AtlasPaths::at(home);
    let backend = Arc::new(LocalBackend::open(&paths, None, false).unwrap());
    let project = backend.connect_project(root.to_path_buf(), "cli").await.unwrap();
    (backend, project)
}

/// An MCP session over an in-memory duplex, labelled as `source_tool` so the actor
/// the tools record is the one under test.
async fn session(backend: Arc<LocalBackend>, root: &std::path::Path, source_tool: &str) -> RunningService<RoleClient, ()> {
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    let server = AtlasMcp::new(backend)
        .with_source_tool(source_tool)
        .with_env_project_root(false)
        .with_project_root(root.to_path_buf());
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_side).await {
            let _ = running.waiting().await;
        }
    });
    ().serve(client_side).await.unwrap()
}

/// One tool call, built the way rmcp's non-exhaustive params type wants.
fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name).with_arguments(arguments.as_object().cloned().unwrap())
}

/// `task_movers` names the tools that may move a task here. A tool that is not on the
/// list gets the conflict text back rather than a moved task, and one that is moves it.
#[tokio::test]
async fn task_move_obeys_the_projects_task_movers() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (backend, project) = backend(home.path(), root.path()).await;
    backend
        .set_agent_access(
            project.id,
            AgentAccess { memory_writers: None, task_movers: Some(vec!["claude-code".into()]), require_review: false },
            "cli",
        )
        .await
        .unwrap();
    let task = backend
        .create_task(NewTask { project_id: Some(project.id), title: "move me".into(), ..Default::default() }, "cli")
        .await
        .unwrap();

    let refused = session(backend.clone(), root.path(), "codex").await;
    let err = refused
        .call_tool(call("task_move", serde_json::json!({"key": task.key, "stage": "In Progress"})))
        .await
        .expect_err("codex is not on task_movers");
    // The exact sentence, not just its shape: clients read this text.
    let text = err.to_string();
    assert!(
        text.contains(&format!("conflict: actor 'codex' may not move tasks in project {}", project.name)),
        "{text}"
    );
    refused.cancel().await.unwrap();

    // The named tool moves it, and a sub-agent of that tool inherits the permission.
    let allowed = session(backend.clone(), root.path(), "claude-code").await;
    let moved = allowed
        .call_tool(call("task_move", serde_json::json!({"key": task.key, "stage": "In Progress", "agent": "reviewer"})))
        .await
        .unwrap();
    assert!(!moved.is_error.unwrap_or(false), "{moved:?}");
    assert_eq!(backend.get_task(&task.key).await.unwrap().task.stage, "In Progress");
    allowed.cancel().await.unwrap();
}

/// `require_review` is not a refusal: the memory is stored, but it waits in the review
/// queue instead of joining recall straight away.
#[tokio::test]
async fn require_review_lands_an_mcp_memory_as_pending() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (backend, project) = backend(home.path(), root.path()).await;
    backend
        .set_agent_access(project.id, AgentAccess { memory_writers: None, task_movers: None, require_review: true }, "cli")
        .await
        .unwrap();

    let client = session(backend.clone(), root.path(), "codex").await;
    let stored = client
        .call_tool(call("remember", serde_json::json!({"text": "the deploy target is fly.io", "kind": "decision"})))
        .await
        .unwrap();
    assert!(!stored.is_error.unwrap_or(false), "{stored:?}");
    client.cancel().await.unwrap();

    let pending = backend.list_memories(MemoryStatus::Pending, Some(project.id), MemoryScopeFilter::All).await.unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].text, "the deploy target is fly.io");
    assert!(backend.list_memories(MemoryStatus::Active, Some(project.id), MemoryScopeFilter::All).await.unwrap().is_empty());
}

/// `memory_writers` is the same allow-list for the memory path, and the CLI is exempt
/// from it: the user's own hands are not an agent.
#[tokio::test]
async fn memory_writers_refuses_an_unnamed_tool_but_never_the_cli() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (backend, project) = backend(home.path(), root.path()).await;
    backend
        .set_agent_access(
            project.id,
            AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: None, require_review: false },
            "cli",
        )
        .await
        .unwrap();

    let client = session(backend.clone(), root.path(), "codex").await;
    let err = client
        .call_tool(call("remember", serde_json::json!({"text": "codex was here"})))
        .await
        .expect_err("codex is not on memory_writers");
    let text = err.to_string();
    assert!(
        text.contains(&format!("conflict: actor 'codex' may not write memories in project {}", project.name)),
        "{text}"
    );
    client.cancel().await.unwrap();

    let mine = NewMemory {
        scope: MemoryScope::Project,
        project_id: Some(project.id),
        kind: MemoryKind::Fact,
        text: "the user wrote this".into(),
        tags: vec![],
        source_agent: None,
        source_tool: Some("cli".into()),
        confidence: 1.0,
        status: MemoryStatus::Active,
    };
    assert_eq!(backend.remember(mine, "cli/ann").await.unwrap().status, MemoryStatus::Active);
}

/// The actor `ingest_transcript` is checked against is this server's own label, never
/// a string the caller sends. The tool no longer offers a `source_tool` argument at
/// all, and one sent anyway is ignored rather than believed: an agent must not be able
/// to name an exempt actor like `desktop` and walk past `memory_writers`.
#[tokio::test]
async fn ingest_transcript_takes_its_actor_from_the_server_not_the_caller() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (backend, project) = backend(home.path(), root.path()).await;
    backend
        .set_agent_access(
            project.id,
            AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: None, require_review: false },
            "cli",
        )
        .await
        .unwrap();

    let client = session(backend.clone(), root.path(), "codex").await;

    // The schema is the first line of the fix: there is nothing to send.
    let tools = client.list_all_tools().await.unwrap();
    let ingest = tools.iter().find(|t| t.name == "ingest_transcript").expect("the tool is served");
    let props = ingest.input_schema.get("properties").and_then(|p| p.as_object()).expect("an object schema");
    assert!(!props.contains_key("source_tool"), "the caller may not name the actor: {props:?}");
    assert!(props.contains_key("agent"), "the agent inside the tool is still nameable: {props:?}");

    // Sent anyway, it is ignored: the gate still sees this server's own label.
    let err = client
        .call_tool(call("ingest_transcript", serde_json::json!({"text": "user: we use bun", "source_tool": "desktop"})))
        .await
        .expect_err("codex is not on memory_writers, whatever it calls itself");
    let text = err.to_string();
    assert!(
        text.contains(&format!("actor 'codex' may not write memories in project {}", project.name)),
        "{text}"
    );

    // `agent` only ever appends to that label, exactly as `remember` does.
    let err = client
        .call_tool(call("ingest_transcript", serde_json::json!({"text": "user: we use bun", "agent": "worker"})))
        .await
        .expect_err("a sub-agent of codex is still codex");
    let text = err.to_string();
    assert!(
        text.contains(&format!("actor 'codex/worker' may not write memories in project {}", project.name)),
        "{text}"
    );
    client.cancel().await.unwrap();
}
