//! Handler-level tests that drive the router in this process with `oneshot`: no
//! `atlasd` child, no port, no readiness poll. A real `LocalBackend` on a temporary
//! home stands behind the handlers, so what is under test is the HTTP layer itself:
//! the token guard, the actor header, the error shape, query and body parsing, and
//! the path aliases. Whole-daemon behaviour stays in `tests/api.rs`; the unit tests of
//! the guard and the extractors sit in `http.rs` itself.

use super::{app, token_sha256, TOKEN_HEADER};
use crate::state::AppState;
use atlas_core::{backend::LocalBackend, paths::AtlasPaths};
use axum::{body::{to_bytes, Body}, http::{Request, StatusCode}, Router};
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The router over a fresh backend. The temp dir must outlive the test, or the
/// database vanishes under the handlers.
fn harness() -> (Router, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
    (app(AppState::in_process(backend, TOKEN.into())), home)
}

/// One request with the token, plus any extra headers, and a JSON body when given.
async fn call(app: &Router, method: &str, path: &str, headers: &[(&str, &str)], body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(path).header(TOKEN_HEADER, TOKEN);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{status} answered non-JSON ({e}): {}", String::from_utf8_lossy(&bytes))) };
    (status, json)
}

#[tokio::test]
async fn status_needs_the_token_and_reports_its_hash() {
    let (app, _home) = harness();
    let bare = app.clone().oneshot(Request::builder().uri("/api/v1/status").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(bare.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = serde_json::from_slice(&to_bytes(bare.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["error"], "unauthorized");

    let (status, report) = call(&app, "GET", "/api/v1/status", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["token_sha256"], token_sha256(TOKEN));
    assert_eq!(report["memories_active"], 0, "{report}");
}

#[tokio::test]
async fn the_actor_header_names_the_writer() {
    let (app, _home) = harness();
    let (status, task) = call(&app, "POST", "/api/v1/tasks", &[("X-Atlas-Actor", "cli/reviewer")], Some(serde_json::json!({"title": "Check the header"}))).await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    assert_eq!(task["created_by"], "cli/reviewer");
    let key = task["key"].as_str().unwrap();
    let (status, detail) = call(&app, "GET", &format!("/api/v1/tasks/{key}"), &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["events"][0]["actor"], "cli/reviewer", "{detail}");
}

#[tokio::test]
async fn malformed_input_is_400_in_the_error_shape() {
    let (app, _home) = harness();
    let (status, body) = call(&app, "POST", "/api/v1/tasks", &[], Some(serde_json::json!({"title": 5}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().is_some_and(|e| !e.is_empty()), "{body}");
    assert_eq!(body["kind"], "invalid", "{body}");

    let (status, body) = call(&app, "GET", "/api/v1/tasks/counts?project_id=not-a-uuid", &[], None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["kind"], "invalid", "{body}");

    let (status, body) = call(&app, "GET", "/api/v1/tasks/ATL-999", &[], None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["kind"], "not_found", "{body}");
}

#[tokio::test]
async fn the_persona_paths_alias_the_agent_routes() {
    let (app, _home) = harness();
    let (status, created) = call(&app, "POST", "/api/v1/agents", &[], Some(serde_json::json!({"name": "Reviewer", "role": "Reviews"}))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["slug"], "reviewer");
    let (_, agents) = call(&app, "GET", "/api/v1/agents", &[], None).await;
    let (status, personas) = call(&app, "GET", "/api/v1/personas", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(personas, agents, "the old path answers the same rows");
    let (status, by_slug) = call(&app, "GET", "/api/v1/personas/reviewer", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(by_slug["id"], created["id"]);
}
