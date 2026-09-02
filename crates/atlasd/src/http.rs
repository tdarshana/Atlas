use axum::{body::Bytes, extract::{FromRequest, FromRequestParts, Path, Query, Request, State}, http::{header, request::Parts, HeaderMap, Method, StatusCode}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}, Json, Router};
use atlas_core::{backend::Backend, jobs::Job, models::*, AtlasError};
use serde::Deserialize;
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;
use crate::state::AppState;

pub struct ApiError(AtlasError);
impl From<AtlasError> for ApiError { fn from(e: AtlasError) -> Self { Self(e) } }
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = match self.0 {
            AtlasError::NotFound(_) => StatusCode::NOT_FOUND,
            AtlasError::Invalid(_) => StatusCode::BAD_REQUEST,
            AtlasError::Conflict(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (code, Json(serde_json::json!({"error": self.0.to_string()}))).into_response()
    }
}

/// `Json<T>` extractor whose rejection is `ApiError`, so malformed bodies
/// come back as `{"error": string}` (400) instead of axum's plain-text rejection.
pub struct ApiJson<T>(pub T);
impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Self(v)),
            Err(rejection) => Err(ApiError(AtlasError::Invalid(rejection.body_text()))),
        }
    }
}

/// `Query<T>` extractor whose rejection is `ApiError`, for the same reason: an
/// unparseable or missing query parameter must come back as `{"error": string}` (400)
/// rather than axum's plain-text rejection.
pub struct ApiQuery<T>(pub T);
impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(v)) => Ok(Self(v)),
            Err(rejection) => Err(ApiError(AtlasError::Invalid(rejection.body_text()))),
        }
    }
}

/// `Path<T>` extractor whose rejection is `ApiError`, for the same reason.
pub struct ApiPath<T>(pub T);
impl<T, S> FromRequestParts<S> for ApiPath<T>
where
    T: serde::de::DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(v)) => Ok(Self(v)),
            Err(rejection) => Err(ApiError(AtlasError::Invalid(rejection.body_text()))),
        }
    }
}

/// `host` with any `:port` suffix removed, so `127.0.0.1:7433` and `127.0.0.1` compare alike.
fn strip_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => host,
    }
}

fn is_loopback_host(host: &str) -> bool { matches!(strip_port(host), "127.0.0.1" | "localhost" | "[::1]" | "tauri.localhost") }

/// `http://` loopback origins count, plus the two fixed origins the Tauri desktop
/// app's webview sends: `tauri://localhost` (WKWebView/wry, macOS and Linux) and
/// `http://tauri.localhost` (WebView2, Windows; covered by `is_loopback_host` above).
/// Anything else is a page on the open web.
fn is_loopback_origin(origin: &str) -> bool {
    if origin == "tauri://localhost" { return true; }
    match origin.strip_prefix("http://") { Some(rest) => !rest.contains('/') && is_loopback_host(rest), None => false }
}

/// The daemon has no authentication, so it must not be reachable from a web page that
/// happens to be open in the user's browser: reject any cross-origin request, and any
/// request whose `Host` is a name pointed at 127.0.0.1 from outside (DNS rebinding).
async fn guard(req: Request, next: Next) -> Response {
    let allowed = {
        let h = req.headers();
        let origin_ok = h.get(header::ORIGIN).is_none_or(|v| v.to_str().is_ok_and(is_loopback_origin));
        // An absent Host is HTTP/2 or HTTP/1.0, where the authority never came from a browser.
        let host_ok = h.get(header::HOST).is_none_or(|v| v.to_str().is_ok_and(is_loopback_host));
        origin_ok && host_ok
    };
    if allowed { next.run(req).await } else { (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "forbidden origin"}))).into_response() }
}

/// Wrap a whole app, `/mcp` included, in the loopback guard.
pub fn guard_loopback(app: Router) -> Router { app.layer(middleware::from_fn(guard)) }

/// The only origins allowed to *read* a response: the two the Tauri webview sends
/// (`tauri://localhost` on WKWebView/wry, `http://tauri.localhost` on WebView2) and the
/// desktop app's Vite dev server. Deliberately narrower than `is_loopback_origin`, which
/// admits any loopback host on any port.
const CORS_ORIGINS: &[&str] = &["tauri://localhost", "http://tauri.localhost", "http://localhost:1420"];

fn is_cors_origin(origin: &str) -> bool { CORS_ORIGINS.contains(&origin) }

/// Sends `Access-Control-Allow-Origin` only for `CORS_ORIGINS`, so the desktop app can read
/// responses. This is a strict subset of what `guard` lets through: a local page served from
/// another loopback port passes the guard (its request runs) but gets no allow-origin header,
/// so the browser blocks it from reading the body. `guard` still runs outside this layer and
/// returns 403 with no CORS headers for a non-loopback origin.
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            origin.to_str().is_ok_and(is_cors_origin)
        }))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT])
        .allow_credentials(false)
        .max_age(std::time::Duration::from_secs(600))
}

#[derive(Deserialize)] pub struct ActorQ { pub actor: Option<String> }
#[derive(Deserialize)] pub struct ForgetBody { pub reason: Option<String> }
#[derive(Deserialize)] pub struct RootBody { pub root: std::path::PathBuf }
#[derive(Deserialize)] pub struct StatusBody { pub status: String }
#[derive(Deserialize)] pub struct ProjectQ { pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct ListMemoriesQ { pub status: Option<String>, pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct IngestBody { pub text: String, pub source_tool: String, #[serde(default)] pub project_root: Option<std::path::PathBuf> }

fn actor(q: &ActorQ) -> &str { q.actor.as_deref().unwrap_or("api") }

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/memories", post(create_memory).get(list_memories))
        .route("/api/v1/memories/search", post(search))
        .route("/api/v1/memories/{id}", get(get_memory))
        .route("/api/v1/memories/{id}/forget", post(forget))
        .route("/api/v1/memories/{id}/status", post(set_memory_status))
        .route("/api/v1/projects", get(list_projects))
        .route("/api/v1/projects/connect", post(connect_project))
        .route("/api/v1/projects/context", post(project_context))
        .route("/api/v1/projects/{id}", get(get_project).delete(delete_project))
        .route("/api/v1/projects/{id}/refresh", post(refresh_project))
        .route("/api/v1/agents", get(list_agents).post(save_agent))
        .route("/api/v1/agents/{name}", get(get_agent).delete(delete_agent))
        .route("/api/v1/practices", get(list_practices).post(save_practice))
        .route("/api/v1/practices/{name}", get(get_practice).delete(delete_practice))
        .route("/api/v1/workflows", get(list_workflows).post(save_workflow))
        .route("/api/v1/workflows/{name}", get(get_workflow).delete(delete_workflow))
        .route("/api/v1/sync", post(sync))
        .route("/api/v1/settings", get(get_settings).put(set_settings))
        .route("/api/v1/ingest", post(ingest))
        .route("/api/v1/jobs/{id}", get(get_job))
        .with_state(state)
}

async fn status(State(s): State<AppState>) -> Result<Json<StatusReport>, ApiError> { Ok(Json(s.backend.status().await?)) }
async fn create_memory(State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(m): ApiJson<NewMemory>) -> Result<(StatusCode, Json<Memory>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.remember(m, actor(&q)).await?)))
}
async fn list_memories(State(s): State<AppState>, ApiQuery(q): ApiQuery<ListMemoriesQ>) -> Result<Json<Vec<Memory>>, ApiError> {
    // An empty `?status=` is a caller who left the filter blank, not a bad status.
    let status = match q.status.as_deref().filter(|v| !v.is_empty()) { Some(v) => v.parse()?, None => MemoryStatus::Active };
    Ok(Json(s.backend.list_memories(status, q.project_id).await?))
}
async fn set_memory_status(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(b): ApiJson<StatusBody>) -> Result<Json<Memory>, ApiError> {
    Ok(Json(s.backend.set_memory_status(id, b.status.parse()?, actor(&q)).await?))
}
async fn get_memory(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Memory>, ApiError> { Ok(Json(s.backend.get_memory(id).await?)) }
async fn search(State(s): State<AppState>, ApiJson(q): ApiJson<RecallQuery>) -> Result<Json<Vec<RecallHit>>, ApiError> { Ok(Json(s.backend.recall(q).await?)) }
async fn forget(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, ApiQuery(q): ApiQuery<ActorQ>, headers: HeaderMap, body: Bytes) -> Result<Json<Memory>, ApiError> {
    let reason = if body.is_empty() {
        None
    } else {
        // A body sent under any other content type is a form post a browser can make
        // without a preflight, so refuse it rather than parse it.
        let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !ct.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json") {
            return Err(ApiError(AtlasError::Invalid("a request body requires Content-Type: application/json".into())));
        }
        serde_json::from_slice::<ForgetBody>(&body).map_err(|e| ApiError(AtlasError::Invalid(e.to_string())))?.reason
    };
    Ok(Json(s.backend.forget(id, reason, actor(&q)).await?))
}

// ---- projects ----

async fn list_projects(State(s): State<AppState>) -> Result<Json<Vec<Project>>, ApiError> { Ok(Json(s.backend.list_projects().await?)) }
async fn get_project(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Project>, ApiError> { Ok(Json(s.backend.get_project(id).await?)) }
async fn refresh_project(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Project>, ApiError> { Ok(Json(s.backend.refresh_project(id).await?)) }
async fn delete_project(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, ApiQuery(q): ApiQuery<ActorQ>) -> Result<StatusCode, ApiError> {
    s.backend.delete_project(id, actor(&q)).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn connect_project(State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(b): ApiJson<RootBody>) -> Result<Json<Project>, ApiError> {
    Ok(Json(s.backend.connect_project(b.root, actor(&q)).await?))
}
async fn project_context(State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(b): ApiJson<RootBody>) -> Result<Json<ProjectContext>, ApiError> {
    Ok(Json(s.backend.project_context(b.root, actor(&q)).await?))
}

// ---- agents ----

async fn list_agents(State(s): State<AppState>) -> Result<Json<Vec<Agent>>, ApiError> { Ok(Json(s.backend.list_agents().await?)) }
async fn get_agent(State(s): State<AppState>, ApiPath(name): ApiPath<String>) -> Result<Json<Agent>, ApiError> { Ok(Json(s.backend.get_agent(&name).await?)) }
async fn save_agent(State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(a): ApiJson<NewAgent>) -> Result<(StatusCode, Json<Agent>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.save_agent(a, actor(&q)).await?)))
}
async fn delete_agent(State(s): State<AppState>, ApiPath(name): ApiPath<String>, ApiQuery(q): ApiQuery<ActorQ>) -> Result<StatusCode, ApiError> {
    s.backend.delete_agent(&name, actor(&q)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- practices / workflows ----
//
// One pair of handlers per kind, each delegating to the shared body below, because a
// route's handler is where the kind comes from: it isn't in the path or the body.

async fn list_practices(s: State<AppState>, q: ApiQuery<ProjectQ>) -> Result<Json<Vec<Doc>>, ApiError> { list_docs(DocKind::Practice, s, q).await }
async fn list_workflows(s: State<AppState>, q: ApiQuery<ProjectQ>) -> Result<Json<Vec<Doc>>, ApiError> { list_docs(DocKind::Workflow, s, q).await }
async fn get_practice(s: State<AppState>, n: ApiPath<String>) -> Result<Json<Doc>, ApiError> { get_doc(DocKind::Practice, s, n).await }
async fn get_workflow(s: State<AppState>, n: ApiPath<String>) -> Result<Json<Doc>, ApiError> { get_doc(DocKind::Workflow, s, n).await }
async fn save_practice(s: State<AppState>, q: ApiQuery<ActorQ>, d: ApiJson<NewDoc>) -> Result<(StatusCode, Json<Doc>), ApiError> { save_doc(DocKind::Practice, s, q, d).await }
async fn save_workflow(s: State<AppState>, q: ApiQuery<ActorQ>, d: ApiJson<NewDoc>) -> Result<(StatusCode, Json<Doc>), ApiError> { save_doc(DocKind::Workflow, s, q, d).await }
async fn delete_practice(s: State<AppState>, n: ApiPath<String>, q: ApiQuery<ActorQ>) -> Result<StatusCode, ApiError> { delete_doc(DocKind::Practice, s, n, q).await }
async fn delete_workflow(s: State<AppState>, n: ApiPath<String>, q: ApiQuery<ActorQ>) -> Result<StatusCode, ApiError> { delete_doc(DocKind::Workflow, s, n, q).await }

async fn list_docs(kind: DocKind, State(s): State<AppState>, ApiQuery(q): ApiQuery<ProjectQ>) -> Result<Json<Vec<Doc>>, ApiError> {
    Ok(Json(s.backend.list_docs(kind, q.project_id).await?))
}
async fn get_doc(kind: DocKind, State(s): State<AppState>, ApiPath(name): ApiPath<String>) -> Result<Json<Doc>, ApiError> {
    Ok(Json(s.backend.get_doc(kind, &name).await?))
}
async fn save_doc(kind: DocKind, State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(d): ApiJson<NewDoc>) -> Result<(StatusCode, Json<Doc>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.save_doc(kind, d, actor(&q)).await?)))
}
async fn delete_doc(kind: DocKind, State(s): State<AppState>, ApiPath(name): ApiPath<String>, ApiQuery(q): ApiQuery<ActorQ>) -> Result<StatusCode, ApiError> {
    s.backend.delete_doc(kind, &name, actor(&q)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- sync ----

async fn sync(State(s): State<AppState>, ApiJson(req): ApiJson<SyncRequest>) -> Result<Json<SyncReport>, ApiError> { Ok(Json(s.backend.sync(req).await?)) }

// ---- settings ----

async fn get_settings(State(s): State<AppState>) -> Result<Json<serde_json::Map<String, serde_json::Value>>, ApiError> { Ok(Json(s.backend.get_settings().await?)) }
async fn set_settings(State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, ApiJson(values): ApiJson<serde_json::Map<String, serde_json::Value>>) -> Result<Json<serde_json::Map<String, serde_json::Value>>, ApiError> {
    Ok(Json(s.backend.set_settings(values, actor(&q)).await?))
}

// ---- extraction ----
//
// Ingest is asynchronous: the model call can take a minute, so the request only
// queues the work (202) and the caller follows the job.

async fn ingest(State(s): State<AppState>, ApiJson(b): ApiJson<IngestBody>) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let job_id = s.backend.ingest_transcript(b.text, b.source_tool, b.project_root).await?;
    Ok((StatusCode::ACCEPTED, Json(serde_json::json!({"job_id": job_id}))))
}

async fn get_job(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Job>, ApiError> {
    s.backend.get_job(id).await?.map(Json).ok_or_else(|| ApiError(AtlasError::NotFound(format!("job {id}"))))
}
