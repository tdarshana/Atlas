use axum::{body::Bytes, extract::{FromRequest, FromRequestParts, Path, Query, Request, State}, http::{header, request::Parts, HeaderMap, Method, StatusCode}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{delete, get, post, put}, Json, Router};
use atlas_core::{backend::{StatusBackend, MemoryBackend, ProjectBackend, LibraryBackend, JobBackend, BoardBackend, WorkflowBackend, SearchBackend, SkillBackend, McpBackend, PersonaBackend}, jobs::Job, models::*, search::global::{SearchKind, SearchQuery, SearchResult, DEFAULT_LIMIT}, AtlasError};
use atlas_mcp::{ToolScope, TOOL_TABLE};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;
use atlas_core::projects::PersonaRef;
use crate::mcp_clients::{McpClient, Transport};
use crate::state::AppState;

pub struct ApiError(AtlasError);
impl From<AtlasError> for ApiError { fn from(e: AtlasError) -> Self { Self(e) } }
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = match self.0 {
            AtlasError::NotFound(_) => StatusCode::NOT_FOUND,
            AtlasError::Invalid(_) => StatusCode::BAD_REQUEST,
            AtlasError::Conflict(_) => StatusCode::CONFLICT,
            AtlasError::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
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

/// Parses `X-Atlas-Actor`, trimmed. `None` when the header is absent; present but out
/// of range (empty after trimming, or over 64 characters) is `Invalid`, not a silent
/// clamp: a caller who sent a bad header should be told, not have it quietly replaced.
fn parse_actor_header(headers: &HeaderMap) -> std::result::Result<Option<String>, ApiError> {
    match headers.get("x-atlas-actor") {
        None => Ok(None),
        Some(v) => {
            let s = v.to_str().map_err(|e| ApiError(AtlasError::Invalid(format!("X-Atlas-Actor: {e}"))))?.trim();
            if s.is_empty() || s.chars().count() > 64 {
                return Err(ApiError(AtlasError::Invalid("X-Atlas-Actor must be 1..64 characters".into())));
            }
            Ok(Some(s.to_string()))
        }
    }
}

/// Resolves `X-Atlas-Persona`, when present, to the persona it names. The header takes
/// a slug and nothing else (not a name, not an id), and one no persona holds is
/// `Invalid`: the MCP router is the only sender, and it only ever sends a slug it was
/// given back by the daemon, so anything else is a bug worth surfacing as 400.
async fn parse_persona_header(headers: &HeaderMap, state: &AppState) -> std::result::Result<Option<PersonaRef>, ApiError> {
    let Some(v) = headers.get("x-atlas-persona") else { return Ok(None) };
    let slug = v.to_str().map_err(|e| ApiError(AtlasError::Invalid(format!("X-Atlas-Persona: {e}"))))?.trim();
    if slug.is_empty() {
        return Ok(None);
    }
    let unknown = || ApiError(AtlasError::Invalid(format!("no persona {slug}")));
    match state.backend.get_persona(slug).await {
        Ok(p) if p.slug == slug => Ok(Some(PersonaRef { id: p.id, slug: p.slug })),
        Ok(_) => Err(unknown()),
        Err(AtlasError::NotFound(_)) => Err(unknown()),
        Err(e) => Err(ApiError(e)),
    }
}

/// The board's caller identity: `X-Atlas-Actor`, defaulting to `api` when the header
/// is absent, plus the persona `X-Atlas-Persona` names, which the gated writes hand
/// to the backend beside the label.
pub struct Actor(pub String, pub Option<PersonaRef>);
impl FromRequestParts<AppState> for Actor {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let label = parse_actor_header(&parts.headers)?.unwrap_or_else(|| "api".into());
        Ok(Self(label, parse_persona_header(&parts.headers, state).await?))
    }
}

/// `X-Atlas-Persona` alone, for the memory route, whose actor still arrives as the
/// deprecated `?actor=` query parameter rather than in `X-Atlas-Actor`.
pub struct PersonaHeader(pub Option<PersonaRef>);
impl FromRequestParts<AppState> for PersonaHeader {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(Self(parse_persona_header(&parts.headers, state).await?))
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
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT, header::HeaderName::from_static("x-atlas-actor")])
        .allow_credentials(false)
        .max_age(std::time::Duration::from_secs(600))
}

#[derive(Deserialize)] pub struct ActorQ { pub actor: Option<String> }
#[derive(Deserialize)] pub struct ForgetBody { pub reason: Option<String> }
#[derive(Deserialize)] pub struct RootBody { pub root: std::path::PathBuf }
#[derive(Deserialize)] pub struct StatusBody { pub status: String }
#[derive(Deserialize)] pub struct ProjectQ { pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct ListMemoriesQ { pub status: Option<String>, pub project_id: Option<Uuid>, pub scope: Option<String>, pub limit: Option<usize>, pub offset: Option<usize> }
#[derive(Deserialize)] pub struct MemoryFacetsQ { pub project_id: Option<Uuid>, pub scope: Option<String> }
#[derive(Deserialize)] pub struct LogQ {
    #[serde(default)] pub source: Option<String>,
    #[serde(default)] pub kind: Option<String>,
    #[serde(default)] pub q: Option<String>,
    #[serde(default)] pub after: Option<DateTime<Utc>>,
    #[serde(default)] pub limit: Option<usize>,
}
#[derive(Deserialize)] pub struct IngestBody {
    pub text: String,
    /// Deprecated: send the actor as `X-Atlas-Actor` instead. Kept for one release so
    /// an older caller still works; the header wins when both are sent.
    #[serde(default)] pub source_tool: Option<String>,
    #[serde(default)] pub project_root: Option<std::path::PathBuf>,
}

// ---- MCP (Phase 10) ----

#[derive(Deserialize)] pub struct RegisterMcpClientBody { pub id: String, pub transport: String, pub client_name: String, #[serde(default)] pub client_version: Option<String> }
#[derive(Deserialize)] pub struct McpHeartbeatBody { pub tool_calls: u64 }
/// One row of `GET /api/v1/mcp/status`'s tools table. The strings are owned rather than
/// `&'static str` because a plugin's tools are registered at run time; `source` says
/// which they are: `"builtin"` for `TOOL_TABLE`, `"plugin:<id>"` for a plugin's.
#[derive(Serialize)] pub struct McpToolRow { pub name: String, pub description: String, pub args: String, pub scope: ToolScope, pub enabled: bool, pub source: String }
#[derive(Serialize)] pub struct McpStdioTransport { pub command: &'static str }
#[derive(Serialize)] pub struct McpHttpTransport { pub url: String, pub protocol_version: String }
#[derive(Serialize)] pub struct McpTransports { pub stdio: McpStdioTransport, pub http: McpHttpTransport }
#[derive(Serialize)] pub struct McpCounts { pub tools: usize, pub resources: usize, pub prompts: usize, pub clients: usize }
#[derive(Serialize)] pub struct McpStatusReport {
    pub transports: McpTransports,
    pub counts: McpCounts,
    pub tools: Vec<McpToolRow>,
    pub resources: Vec<rmcp::model::Resource>,
    pub prompts: Vec<rmcp::model::Prompt>,
    pub clients: Vec<McpClient>,
}

/// Task MCP-A: `GET /api/v1/projects/{id}/mcp`'s tool row. Unlike `McpToolRow`'s single
/// `enabled`, this project view carries the two flags separately, since a tool can be
/// enabled globally and disabled here, or (with `mcp.disabled_tools` naming it) the
/// other way, and the desktop's badges need to tell those apart.
#[derive(Serialize)] pub struct ProjectMcpToolRow {
    pub name: String,
    pub description: String,
    pub args: String,
    pub scope: ToolScope,
    pub enabled_globally: bool,
    /// Actually callable here: enabled globally and not in this project's own override.
    pub enabled_here: bool,
    /// `builtin`, or `plugin:<id>` for a tool a plugin contributed. Read the same way as
    /// [`McpToolRow::source`].
    pub source: String,
}
#[derive(Serialize)] pub struct ProjectMcpConnect {
    pub stdio: McpStdioTransport,
    pub http: McpHttpTransport,
    pub project_root: String,
}
#[derive(Serialize)] pub struct ProjectMcpReport {
    pub tools: Vec<ProjectMcpToolRow>,
    pub resources: Vec<rmcp::model::Resource>,
    pub prompts: Vec<rmcp::model::Prompt>,
    pub clients: Vec<McpClient>,
    pub connect: ProjectMcpConnect,
}
#[derive(Deserialize)] pub struct McpToolsBody { pub disabled: Vec<String> }
/// `PUT /api/v1/mcp/plugin-tools/{plugin_id}`'s body. Each decl's `plugin_id` is
/// optional in the JSON and overwritten from the path.
#[derive(Deserialize)] pub struct PluginToolsBody { pub tools: Vec<PluginToolDecl> }
#[derive(Deserialize)] pub struct PluginToolCallBody { #[serde(default)] pub args: serde_json::Value }

fn actor(q: &ActorQ) -> &str { q.actor.as_deref().unwrap_or("api") }

// ---- board ----

/// A query flag: `true` and `1` are true, and anything else, including an absent or
/// empty value, is false. Serde's own `bool` refuses `ready=1` with a 400, which is a
/// worse answer to a list request than the list it asked for; a filter nobody spelled
/// the way this route expects is better left off than turned into an error.
fn query_flag<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    let raw = Option::<String>::deserialize(d)?;
    Ok(matches!(raw.as_deref(), Some("true") | Some("1")))
}

/// Like `query_flag`, but tri-state: an absent query value is `None` (no filter),
/// and any present value is `Some` of whether it reads as true (`true`/`1`) or not.
fn query_flag_opt<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<bool>, D::Error> {
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw.map(|v| matches!(v.as_str(), "true" | "1")))
}

#[derive(Deserialize)] pub struct TaskListQ {
    #[serde(default)] pub project_id: Option<Uuid>,
    #[serde(default)] pub stage: Option<String>,
    #[serde(default)] pub assignee: Option<String>,
    #[serde(default, deserialize_with = "query_flag")] pub ready: bool,
    #[serde(default)] pub q: Option<String>,
    #[serde(default, deserialize_with = "query_flag")] pub include_done: bool,
    /// `global`, for the literal global board (tasks with no project); anything else,
    /// including absent or empty, is the existing widen-or-narrow-by-`project_id` read.
    #[serde(default)] pub scope: Option<String>,
    /// `Some(true)` keeps only parent-less tasks, `Some(false)` only subtasks, `None`
    /// (an absent query value) applies no filter.
    #[serde(default, deserialize_with = "query_flag_opt")] pub top_level: Option<bool>,
    /// Keep only tasks done as this persona, by id or slug. Empty is no filter.
    #[serde(default)] pub persona: Option<String>,
}
#[derive(Deserialize)] pub struct MoveBody { pub stage: String, #[serde(default)] pub expected_updated_at: Option<DateTime<Utc>> }
#[derive(Deserialize)] pub struct CommentBody { pub body: String }
#[derive(Deserialize, Default)] pub struct ClaimBody { #[serde(default)] pub force: bool }
#[derive(Deserialize)] pub struct BlockersBody { pub blocked_by: Vec<String> }
#[derive(Deserialize)] pub struct BoardStagesQ { pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct TaskCountsQ {
    #[serde(default)] pub project_id: Option<Uuid>,
    /// Read exactly like `TaskListQ::scope`, so `/tasks/counts` never disagrees with
    /// `/tasks` about what a bare request or `scope=global` means.
    #[serde(default)] pub scope: Option<String>,
    /// Read exactly like `TaskListQ::top_level`, so the side panel's counts can match
    /// a board list narrowed to parent tasks.
    #[serde(default, deserialize_with = "query_flag_opt")] pub top_level: Option<bool>,
}
#[derive(Deserialize)] pub struct SetStagesBody { pub stages: Vec<Stage>, #[serde(default)] pub renames: HashMap<String, String> }
#[derive(Deserialize)] pub struct SetProjectStagesBody { #[serde(default)] pub stages: Option<Vec<Stage>>, #[serde(default)] pub renames: HashMap<String, String> }
#[derive(Serialize)] pub struct StageCount { pub stage: String, pub count: i64 }

// ---- frameworks (Phase 12) ----

#[derive(Deserialize)] pub struct FrameworkImportBody { pub what: ImportWhat }

// ---- skills (Phase 15) ----

#[derive(Deserialize)] pub struct SkillsQ { #[serde(default)] pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct SkillBodyBody { pub body: String }
#[derive(Deserialize)] pub struct SkillsDisabledBody { pub disabled: Vec<String> }

// ---- personas (Phase 17) ----

#[derive(Deserialize)] pub struct PersonaBundleQ { #[serde(default)] pub project_id: Option<Uuid> }

// ---- the agents' MCP servers (Phase 16) ----

#[derive(Deserialize)] pub struct McpServersQ { #[serde(default)] pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct McpEnabledBody { pub enabled: bool }

// ---- workflows (Phase 9) ----

#[derive(Deserialize)] pub struct WorkflowListQ { #[serde(default)] pub project_id: Option<Uuid> }
#[derive(Deserialize)] pub struct RunsQ { #[serde(default)] pub limit: Option<usize> }
#[derive(Deserialize)] pub struct AllRunsQ { #[serde(default)] pub since: Option<DateTime<Utc>>, #[serde(default)] pub limit: Option<usize> }
#[derive(Deserialize)] pub struct RunWorkflowBody { #[serde(default)] pub trigger: Option<TriggerKind>, #[serde(default)] pub input: Option<String> }
#[derive(Serialize)] pub struct RunDetail { pub run: WorkflowRun, pub steps: Vec<WorkflowStep> }
/// `GET /workflows/{id}/runs?limit=` default, for a caller who leaves it off.
const DEFAULT_RUNS_LIMIT: usize = 20;
/// `GET /runs?since=&limit=` default, for a caller who leaves it off.
const DEFAULT_ALL_RUNS_LIMIT: usize = 50;

// ---- search ----

#[derive(Deserialize)] pub struct SearchQ {
    #[serde(default)] pub q: Option<String>,
    #[serde(default)] pub project_id: Option<Uuid>,
    #[serde(default)] pub kinds: Option<String>,
    #[serde(default)] pub limit: Option<usize>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/events", get(events))
        .route("/api/v1/memories", post(create_memory).get(list_memories))
        .route("/api/v1/memories/facets", get(memory_facets))
        .route("/api/v1/memories/search", post(search))
        .route("/api/v1/memories/{id}", get(get_memory))
        .route("/api/v1/memories/{id}/forget", post(forget))
        .route("/api/v1/memories/{id}/status", post(set_memory_status))
        .route("/api/v1/projects", get(list_projects))
        .route("/api/v1/projects/connect", post(connect_project))
        .route("/api/v1/projects/context", post(project_context))
        .route("/api/v1/projects/{id}", get(get_project).patch(patch_project).delete(delete_project))
        .route("/api/v1/projects/{id}/refresh", post(refresh_project))
        .route("/api/v1/projects/{id}/agent-access", put(put_agent_access))
        .route("/api/v1/projects/{id}/access", get(get_project_access))
        .route("/api/v1/projects/{id}/extraction", put(put_project_extraction))
        .route("/api/v1/projects/{id}/log", get(get_project_log))
        .route("/api/v1/projects/{id}/log/export", get(export_project_log))
        .route("/api/v1/agents", get(list_agents).post(save_agent))
        .route("/api/v1/agents/{name}", get(get_agent).delete(delete_agent))
        .route("/api/v1/practices", get(list_practices).post(save_practice))
        .route("/api/v1/practices/{name}", get(get_practice).delete(delete_practice))
        .route("/api/v1/workflows", get(list_workflows).post(create_workflow))
        .route("/api/v1/workflows/{id}", get(get_workflow).patch(patch_workflow).delete(delete_workflow))
        .route("/api/v1/workflows/{id}/run", post(run_workflow))
        .route("/api/v1/workflows/{id}/runs", get(list_runs))
        .route("/api/v1/runs", get(list_all_runs))
        .route("/api/v1/runs/{id}", get(get_run))
        .route("/api/v1/runs/{id}/cancel", post(cancel_run))
        .route("/api/v1/runs/{id}/export", get(export_run))
        .route("/api/v1/sync", post(sync))
        .route("/api/v1/settings", get(get_settings).put(set_settings))
        .route("/api/v1/ingest", post(ingest))
        .route("/api/v1/jobs/{id}", get(get_job))
        .route("/api/v1/extraction/test", post(test_extraction))
        .route("/api/v1/tasks", get(list_tasks).post(create_task))
        .route("/api/v1/tasks/counts", get(task_counts))
        .route("/api/v1/tasks/{id_or_key}", get(get_task).patch(update_task).delete(delete_task))
        .route("/api/v1/tasks/{id_or_key}/move", post(move_task))
        .route("/api/v1/tasks/{id_or_key}/comment", post(comment_task))
        .route("/api/v1/tasks/{id_or_key}/claim", post(claim_task))
        .route("/api/v1/tasks/{id_or_key}/blockers", put(set_task_blockers))
        .route("/api/v1/board/stages", get(get_board_stages).put(put_board_stages))
        .route("/api/v1/projects/{id}/stages", put(put_project_stages))
        .route("/api/v1/projects/{id}/frameworks", get(list_frameworks))
        .route("/api/v1/projects/{id}/frameworks/{kind}/docs/{*path}", get(get_framework_doc))
        .route("/api/v1/projects/{id}/frameworks/{kind}/import", post(import_framework))
        .route("/api/v1/skills", get(list_skills).post(create_skill))
        .route("/api/v1/skills/{*id}", get(get_skill).put(put_skill_body).patch(patch_skill).delete(delete_skill))
        .route("/api/v1/projects/{id}/skills", put(put_project_skills))
        .route("/api/v1/personas", get(list_personas).post(create_persona))
        .route("/api/v1/personas/{id}", get(get_persona).put(put_persona).delete(delete_persona))
        .route("/api/v1/personas/{id}/bundle", get(persona_bundle))
        .route("/api/v1/projects/{id}/personas", get(project_roster).put(put_project_roster))
        .route("/api/v1/search", get(global_search))
        .route("/api/v1/mcp/status", get(mcp_status))
        .route("/api/v1/mcp/clients", post(register_mcp_client))
        .route("/api/v1/mcp/clients/{id}", put(heartbeat_mcp_client).delete(unregister_mcp_client))
        .route("/api/v1/projects/{id}/mcp", get(project_mcp))
        .route("/api/v1/projects/{id}/mcp/tools", put(put_project_mcp_tools))
        .route("/api/v1/mcp/plugin-tools", get(list_plugin_tools))
        .route("/api/v1/mcp/plugin-tools/{plugin_id}", put(put_plugin_tools).delete(delete_plugin_tools))
        .route("/api/v1/mcp/plugin-tools/{plugin_id}/{name}/call", post(call_plugin_tool))
        .route("/api/v1/mcp/plugin-channel", get(plugin_channel))
        // An MCP server id carries `:` and, for a plugin server, slashes. Unlike a skill
        // id it cannot travel as a wildcard, because two of these routes have a segment
        // after the id and a wildcard may only end a route. So the id is one ordinary
        // segment with its slashes percent-encoded (`%2F`), which axum decodes back into
        // the whole id; `RemoteBackend::mcp_server_url` and `encodeURIComponent` both
        // produce exactly that.
        .route("/api/v1/mcp/servers", get(list_mcp_servers).post(add_mcp_server))
        .route("/api/v1/mcp/servers/{id}/check", post(check_mcp_server))
        .route("/api/v1/mcp/servers/{id}/enabled", put(set_mcp_server_enabled))
        .route("/api/v1/mcp/servers/{id}", delete(remove_mcp_server))
        .with_state(state)
}

async fn status(State(s): State<AppState>) -> Result<Json<StatusReport>, ApiError> { Ok(Json(s.backend.status().await?)) }

/// `GET /api/v1/events`: a server-sent event per write, as it lands, so a client keeps
/// what it shows in step without polling. Each event is named by its entity (`task`,
/// `memory`, ...) and carries the `Change` as JSON. A subscriber that falls more than
/// the buffer behind gets a `lagged` event instead of the changes it missed and should
/// refresh wholesale. A comment frame every fifteen seconds keeps the connection open
/// through proxies and lets the client notice a dead daemon.
async fn events(State(s): State<AppState>) -> axum::response::Sse<impl tokio_stream::Stream<Item = std::result::Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
    use tokio_stream::wrappers::{BroadcastStream, WatchStream};
    use tokio_stream::StreamExt;
    let changes = BroadcastStream::new(s.backend.db.subscribe()).map(|item| {
        Some(match item {
            Ok(change) => Event::default().event(change.entity.clone()).json_data(&change).unwrap_or_else(|_| Event::default().event("lagged").data("")),
            Err(BroadcastStreamRecvError::Lagged(n)) => Event::default().event("lagged").data(n.to_string()),
        })
    });
    // The stream ends when the daemon stops: a `None` from the shutdown flag closes
    // it, so the graceful shutdown is not held open by a client that never hangs up.
    let stopping = WatchStream::from_changes(s.shutdown.clone()).filter_map(|stopping| if stopping { Some(None) } else { None });
    let stream = changes.merge(stopping).take_while(Option::is_some).map(|e| Ok(e.expect("filtered above")));
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
async fn create_memory(State(s): State<AppState>, ApiQuery(q): ApiQuery<ActorQ>, PersonaHeader(persona): PersonaHeader, ApiJson(m): ApiJson<NewMemory>) -> Result<(StatusCode, Json<Memory>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.remember_as(m, actor(&q), persona).await?)))
}
async fn list_memories(State(s): State<AppState>, ApiQuery(q): ApiQuery<ListMemoriesQ>) -> Result<Json<Vec<Memory>>, ApiError> {
    // An empty `?status=` is a caller who left the filter blank, not a bad status; the
    // same reading applies to `?scope=`, which defaults to the widening `all`.
    let status = match q.status.as_deref().filter(|v| !v.is_empty()) { Some(v) => v.parse()?, None => MemoryStatus::Active };
    let scope = match q.scope.as_deref().filter(|v| !v.is_empty()) { Some(v) => v.parse()?, None => MemoryScopeFilter::All };
    // `limit` is capped at `MemoryPage::MAX_LIMIT` on the way down; neither given is the
    // whole set, which is what every caller got before the two existed.
    let page = MemoryPage { limit: q.limit, offset: q.offset };
    Ok(Json(s.backend.list_memories(status, q.project_id, scope, page).await?))
}
/// Kind and tag counts, plus the total, over active memories, filtered the same way
/// `GET /memories` filters `project_id`: a bare `project_id` widens to that project plus
/// every global memory, `scope=project_only` narrows to just the project's own.
async fn memory_facets(State(s): State<AppState>, ApiQuery(q): ApiQuery<MemoryFacetsQ>) -> Result<Json<MemoryFacets>, ApiError> {
    let scope = match q.scope.as_deref().filter(|v| !v.is_empty()) { Some(v) => v.parse()?, None => MemoryScopeFilter::All };
    Ok(Json(s.backend.memory_facets(q.project_id, scope).await?))
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
async fn patch_project(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(p): ApiJson<ProjectPatch>) -> Result<Json<Project>, ApiError> {
    Ok(Json(s.backend.update_project(id, p, &actor).await?))
}
async fn put_agent_access(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(a): ApiJson<AgentAccess>) -> Result<Json<Project>, ApiError> {
    Ok(Json(s.backend.set_agent_access(id, a, &actor).await?))
}
/// The project's own `agent_access`, the global `access.*` defaults, and the two
/// resolved together, so a client can show the rule that actually applies.
async fn get_project_access(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<ProjectAccess>, ApiError> {
    Ok(Json(s.backend.project_access(id).await?))
}
/// A body of `null` clears the override and puts the project back on the global
/// extraction settings.
async fn put_project_extraction(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(e): ApiJson<Option<ProjectExtraction>>) -> Result<Json<Project>, ApiError> {
    Ok(Json(s.backend.set_project_extraction(id, e, &actor).await?))
}
async fn get_project_log(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, ApiQuery(q): ApiQuery<LogQ>) -> Result<Json<Vec<LogEntry>>, ApiError> {
    let f = LogFilter { source: q.source, kind: q.kind, q: q.q, after: q.after, limit: q.limit };
    Ok(Json(s.backend.project_log(id, f).await?))
}
/// JSON lines rather than a JSON array: an export is read a line at a time, and the
/// log has no cap here.
async fn export_project_log(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Response, ApiError> {
    let body = s.backend.project_log_export(id).await?;
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response())
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
async fn get_practice(s: State<AppState>, n: ApiPath<String>) -> Result<Json<Doc>, ApiError> { get_doc(DocKind::Practice, s, n).await }
async fn save_practice(s: State<AppState>, q: ApiQuery<ActorQ>, d: ApiJson<NewDoc>) -> Result<(StatusCode, Json<Doc>), ApiError> { save_doc(DocKind::Practice, s, q, d).await }
async fn delete_practice(s: State<AppState>, n: ApiPath<String>, q: ApiQuery<ActorQ>) -> Result<StatusCode, ApiError> { delete_doc(DocKind::Practice, s, n, q).await }

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

/// The blank check and the character cap live in `LocalBackend::ingest_transcript`,
/// not here: the MCP `ingest_transcript` tool reaches the same backend and is just as
/// unauthenticated, so a guard in this handler would only cover half the doors. This
/// route keeps the status mapping, where 413 comes from `AtlasError::TooLarge`.
///
/// The actor comes from `X-Atlas-Actor` when present; the body's `source_tool` is the
/// deprecated fallback for a caller that has not moved to the header yet. Neither one
/// present is a 400 naming both ways to send it.
async fn ingest(State(s): State<AppState>, headers: HeaderMap, ApiJson(b): ApiJson<IngestBody>) -> Response {
    let source_tool = match parse_actor_header(&headers) {
        Ok(header) => match header.or(b.source_tool) {
            Some(v) => v,
            None => return ApiError(AtlasError::Invalid("ingest needs an actor: send X-Atlas-Actor, or the deprecated source_tool body field".into())).into_response(),
        },
        Err(e) => return e.into_response(),
    };
    match s.backend.ingest_transcript(b.text, source_tool, b.project_root).await {
        Ok(job_id) => (StatusCode::ACCEPTED, Json(serde_json::json!({"job_id": job_id}))).into_response(),
        Err(e) => ApiError(e).into_response(),
    }
}

/// The `jobs` row keeps the transcript while the worker needs it, but the route does
/// not hand it back: an ingest payload is up to a million characters of somebody's
/// conversation, and re-serving it would turn every queued or running job id into a
/// second copy for anything on loopback to read. The payload's other fields
/// (`source_tool`, `project_root`) are what a caller actually follows a job by, so
/// they stay, and `text` becomes its own character count. Once the job is `done` or
/// `failed` the row itself has already made that swap (`JobRepo::mark_done` drops
/// `text` for `chars`), and the worker deletes finished rows after seven days.
async fn get_job(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Job>, ApiError> {
    let mut job = s.backend.get_job(id).await?.ok_or_else(|| ApiError(AtlasError::NotFound(format!("job {id}"))))?;
    if let Some(payload) = job.payload.as_object_mut() {
        if let Some(chars) = payload.remove("text").as_ref().and_then(|t| t.as_str()).map(|t| t.chars().count()) {
            payload.insert("chars".into(), serde_json::json!(chars));
        }
    }
    Ok(Json(job))
}

/// A connectivity check against the configured model. Unlike other extraction
/// errors this does not reuse `ApiError`: a model error must come back as 400 with
/// `{"ok": false, "error": ...}`, not the plain `{"error": ...}` every other route
/// answers with, so the caller can render it inline as a failed check rather than
/// a fatal one.
async fn test_extraction(State(s): State<AppState>, ApiQuery(q): ApiQuery<ProjectQ>) -> Response {
    match s.backend.test_extraction_for(q.project_id).await {
        Ok(reply) => (StatusCode::OK, Json(serde_json::json!({"ok": true, "reply": reply}))).into_response(),
        Err(e @ AtlasError::Conflict(_)) => ApiError(e).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}

// ---- board ----

/// Parses a task list/counts `scope` query param the one way both routes read it:
/// `global` names the literal global board (tasks with no project); anything else,
/// including absent or empty, is the existing widen-or-narrow-by-`project_id` read.
/// Refuses `project_id` and `scope=global` together, since a caller cannot mean both
/// "just this project" and "just no project" at once.
fn task_global_only(scope: Option<&str>, project_id: Option<Uuid>) -> Result<bool, ApiError> {
    let global_only = match scope.filter(|v| !v.is_empty()) {
        Some("global") => true,
        Some(other) => return Err(ApiError(AtlasError::Invalid(format!("unknown scope: {other}")))),
        None => false,
    };
    if global_only && project_id.is_some() {
        return Err(ApiError(AtlasError::Invalid("project_id and scope=global cannot both be set".into())));
    }
    Ok(global_only)
}
async fn list_tasks(State(s): State<AppState>, ApiQuery(q): ApiQuery<TaskListQ>) -> Result<Json<Vec<Task>>, ApiError> {
    let global_only = task_global_only(q.scope.as_deref(), q.project_id)?;
    let f = TaskFilter { project_id: q.project_id, stage: q.stage, assignee: q.assignee, ready: q.ready, query: q.q, include_done: q.include_done, global_only, top_level: q.top_level, persona: q.persona.filter(|p| !p.trim().is_empty()) };
    Ok(Json(s.backend.list_tasks(f).await?))
}
async fn create_task(State(s): State<AppState>, Actor(actor, _): Actor, ApiJson(t): ApiJson<NewTask>) -> Result<(StatusCode, Json<Task>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.create_task(t, &actor).await?)))
}
async fn get_task(State(s): State<AppState>, ApiPath(id): ApiPath<String>) -> Result<Json<TaskDetail>, ApiError> {
    Ok(Json(s.backend.get_task(&id).await?))
}
async fn update_task(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor, ApiJson(u): ApiJson<TaskUpdate>) -> Result<Json<Task>, ApiError> {
    Ok(Json(s.backend.update_task(&id, u, &actor).await?))
}
async fn move_task(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, persona): Actor, ApiJson(b): ApiJson<MoveBody>) -> Result<Json<Task>, ApiError> {
    Ok(Json(s.backend.move_task_as(&id, &b.stage, b.expected_updated_at, &actor, persona).await?))
}
async fn comment_task(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor, ApiJson(b): ApiJson<CommentBody>) -> Result<Json<TaskEvent>, ApiError> {
    Ok(Json(s.backend.comment_task(&id, &b.body, &actor).await?))
}
async fn claim_task(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, persona): Actor, ApiJson(b): ApiJson<ClaimBody>) -> Result<Json<Task>, ApiError> {
    Ok(Json(s.backend.claim_task_as(&id, b.force, &actor, persona).await?))
}
async fn set_task_blockers(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor, ApiJson(b): ApiJson<BlockersBody>) -> Result<Json<Task>, ApiError> {
    Ok(Json(s.backend.set_task_blockers(&id, b.blocked_by, &actor).await?))
}
async fn delete_task(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor) -> Result<StatusCode, ApiError> {
    s.backend.delete_task(&id, &actor).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn get_board_stages(State(s): State<AppState>, ApiQuery(q): ApiQuery<BoardStagesQ>) -> Result<Json<StageList>, ApiError> {
    Ok(Json(s.backend.board_stages(q.project_id).await?))
}
async fn put_board_stages(State(s): State<AppState>, Actor(actor, _): Actor, ApiJson(b): ApiJson<SetStagesBody>) -> Result<Json<Vec<Stage>>, ApiError> {
    Ok(Json(s.backend.set_board_stages(b.stages, b.renames, &actor).await?))
}
async fn put_project_stages(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(b): ApiJson<SetProjectStagesBody>) -> Result<Json<StageList>, ApiError> {
    Ok(Json(s.backend.set_project_stages(id, b.stages, b.renames, &actor).await?))
}
async fn task_counts(State(s): State<AppState>, ApiQuery(q): ApiQuery<TaskCountsQ>) -> Result<Json<Vec<StageCount>>, ApiError> {
    let global_only = task_global_only(q.scope.as_deref(), q.project_id)?;
    let counts = s.backend.task_counts(q.project_id, global_only, q.top_level).await?;
    Ok(Json(counts.into_iter().map(|(stage, count)| StageCount { stage, count }).collect()))
}

// ---- frameworks (Phase 12) ----

async fn list_frameworks(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Vec<FrameworkListing>>, ApiError> {
    Ok(Json(s.backend.list_frameworks(id).await?))
}
/// The document's text as `{"content": "..."}`. `path` is axum's wildcard capture
/// (`{*path}`), already percent-decoded, so it carries the relative path exactly as
/// `FrameworkDoc.path` (and `RemoteBackend::get_framework_doc`) built it, slashes
/// included; `kind.parse()` answers a 400 naming the bad value for an unknown one.
async fn get_framework_doc(State(s): State<AppState>, ApiPath((id, kind, path)): ApiPath<(Uuid, String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let kind: FrameworkKind = kind.parse()?;
    let content = s.backend.get_framework_doc(id, kind, &path).await?;
    Ok(Json(serde_json::json!({"content": content})))
}
async fn import_framework(
    State(s): State<AppState>,
    ApiPath((id, kind)): ApiPath<(Uuid, String)>,
    Actor(actor, _): Actor,
    ApiJson(b): ApiJson<FrameworkImportBody>,
) -> Result<Json<ImportReport>, ApiError> {
    let kind: FrameworkKind = kind.parse()?;
    Ok(Json(s.backend.import_framework(id, kind, b.what, &actor).await?))
}

// ---- skills (Phase 15) ----

/// Every skill in scope, plus whatever discovery could not read. `project_id` widens
/// the listing to that project's own roots and fills each summary's `enabled_here`.
async fn list_skills(State(s): State<AppState>, ApiQuery(q): ApiQuery<SkillsQ>) -> Result<Json<SkillList>, ApiError> {
    Ok(Json(s.backend.list_skills(q.project_id).await?))
}
async fn create_skill(State(s): State<AppState>, Actor(actor, _): Actor, ApiJson(b): ApiJson<NewSkill>) -> Result<(StatusCode, Json<Skill>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.create_skill(b, &actor).await?)))
}
/// A skill id carries a `:` and, for a plugin skill, slashes, so it arrives through
/// axum's wildcard capture (`{*id}`) the same way a framework document path does:
/// already percent-decoded, with the slashes between its parts intact.
async fn get_skill(State(s): State<AppState>, ApiPath(id): ApiPath<String>, ApiQuery(q): ApiQuery<SkillsQ>) -> Result<Json<Skill>, ApiError> {
    Ok(Json(s.backend.get_skill(q.project_id, &id).await?))
}
/// Edits a skill in place: the stored body for a native skill, the whole `SKILL.md`
/// for a discovered one.
async fn put_skill_body(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiQuery(q): ApiQuery<SkillsQ>,
    Actor(actor, _): Actor,
    ApiJson(b): ApiJson<SkillBodyBody>,
) -> Result<Json<Skill>, ApiError> {
    Ok(Json(s.backend.write_skill_body(q.project_id, &id, b.body, &actor).await?))
}
/// A native skill's name and description. A discovered skill has neither of its own,
/// so this answers 400 for one rather than pretending to store them.
async fn patch_skill(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor, ApiJson(p): ApiJson<SkillUpdate>) -> Result<Json<Skill>, ApiError> {
    Ok(Json(s.backend.update_skill(&id, p, &actor).await?))
}
async fn delete_skill(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor) -> Result<StatusCode, ApiError> {
    s.backend.delete_skill(&id, &actor).await?;
    Ok(StatusCode::NO_CONTENT)
}
/// Replaces this project's disabled-skill list wholesale; `disabled: []` clears it.
/// Every id has to name a skill that applies here right now, the same shape
/// `PUT /projects/{id}/mcp/tools` takes for tool names.
async fn put_project_skills(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(b): ApiJson<SkillsDisabledBody>) -> Result<Json<Project>, ApiError> {
    Ok(Json(s.backend.set_project_skills_disabled(id, b.disabled, &actor).await?))
}

// ---- personas (Phase 17) ----

async fn list_personas(State(s): State<AppState>) -> Result<Json<Vec<Persona>>, ApiError> {
    Ok(Json(s.backend.list_personas().await?))
}
async fn create_persona(State(s): State<AppState>, Actor(actor, _): Actor, ApiJson(p): ApiJson<NewPersona>) -> Result<(StatusCode, Json<Persona>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.create_persona(p, &actor).await?)))
}
/// `{id}` is an id, a slug or a name here; the two writes below take the id alone.
async fn get_persona(State(s): State<AppState>, ApiPath(id): ApiPath<String>) -> Result<Json<Persona>, ApiError> {
    Ok(Json(s.backend.get_persona(&id).await?))
}
async fn put_persona(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(p): ApiJson<PersonaUpdate>) -> Result<Json<Persona>, ApiError> {
    Ok(Json(s.backend.update_persona(id, p, &actor).await?))
}
async fn delete_persona(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor) -> Result<StatusCode, ApiError> {
    s.backend.delete_persona(id, &actor).await?;
    Ok(StatusCode::NO_CONTENT)
}
/// The persona with its references resolved in the scope of `?project_id=`; a
/// reference that no longer resolves is a warning in the body, not a failure.
async fn persona_bundle(State(s): State<AppState>, ApiPath(id): ApiPath<String>, ApiQuery(q): ApiQuery<PersonaBundleQ>) -> Result<Json<PersonaBundle>, ApiError> {
    Ok(Json(s.backend.resolve_persona(&id, q.project_id).await?))
}
async fn project_roster(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<Vec<RosterRow>>, ApiError> {
    Ok(Json(s.backend.project_roster(id).await?))
}
/// Replaces the roster wholesale: the body is the full list of entries, `[]` clears it.
async fn put_project_roster(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(entries): ApiJson<Vec<RosterEntry>>) -> Result<Json<Vec<RosterRow>>, ApiError> {
    Ok(Json(s.backend.set_project_roster(id, entries, &actor).await?))
}

// ---- the agents' MCP servers (Phase 16) ----

/// Every MCP server the user's agents are wired to, plus whatever discovery could not
/// read. `project_id` swaps the user-level scopes for that project's own; the plugin
/// servers and Atlas are in both, since they apply wherever the user works.
async fn list_mcp_servers(State(s): State<AppState>, ApiQuery(q): ApiQuery<McpServersQ>) -> Result<Json<McpServerList>, ApiError> {
    Ok(Json(s.backend.list_mcp_servers(q.project_id).await?))
}

/// Starts the server and lists its tools. A server that will not start answers 200 with
/// `ok: false` and the reason: the request was fine, the server was not.
///
/// The id arrives as one percent-encoded path segment (see the route table), so
/// `plugin:acme/tools:one` is sent as `plugin%3Aacme%2Ftools%3Aone`.
async fn check_mcp_server(State(s): State<AppState>, ApiPath(id): ApiPath<String>, ApiQuery(q): ApiQuery<McpServersQ>) -> Result<Json<McpCheckResult>, ApiError> {
    Ok(Json(s.backend.check_mcp_server(q.project_id, &id).await?))
}

async fn set_mcp_server_enabled(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiQuery(q): ApiQuery<McpServersQ>,
    Actor(actor, _): Actor,
    ApiJson(b): ApiJson<McpEnabledBody>,
) -> Result<Json<McpServerEntry>, ApiError> {
    Ok(Json(s.backend.set_mcp_server_enabled(q.project_id, &id, b.enabled, &actor).await?))
}

async fn add_mcp_server(State(s): State<AppState>, Actor(actor, _): Actor, ApiJson(b): ApiJson<NewMcpServer>) -> Result<(StatusCode, Json<McpServerEntry>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.add_mcp_server(b, &actor).await?)))
}

async fn remove_mcp_server(State(s): State<AppState>, ApiPath(id): ApiPath<String>, ApiQuery(q): ApiQuery<McpServersQ>, Actor(actor, _): Actor) -> Result<StatusCode, ApiError> {
    s.backend.remove_mcp_server(q.project_id, &id, &actor).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- workflows (Phase 9) ----

async fn list_workflows(State(s): State<AppState>, ApiQuery(q): ApiQuery<WorkflowListQ>) -> Result<Json<Vec<Workflow>>, ApiError> {
    Ok(Json(s.backend.list_workflows(q.project_id).await?))
}
async fn create_workflow(State(s): State<AppState>, Actor(actor, _): Actor, ApiJson(w): ApiJson<NewWorkflow>) -> Result<(StatusCode, Json<Workflow>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.create_workflow(w, &actor).await?)))
}
async fn get_workflow(State(s): State<AppState>, ApiPath(id): ApiPath<String>) -> Result<Json<Workflow>, ApiError> {
    Ok(Json(s.backend.get_workflow(&id).await?))
}
async fn patch_workflow(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor, ApiJson(p): ApiJson<WorkflowPatch>) -> Result<Json<Workflow>, ApiError> {
    Ok(Json(s.backend.update_workflow(&id, p, &actor).await?))
}
async fn delete_workflow(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, _): Actor) -> Result<StatusCode, ApiError> {
    s.backend.delete_workflow(&id, &actor).await?;
    Ok(StatusCode::NO_CONTENT)
}
/// Queues a run and answers 202 with it: the run itself takes as long as the model
/// call, the same asynchronous shape `POST /ingest` uses.
async fn run_workflow(State(s): State<AppState>, ApiPath(id): ApiPath<String>, Actor(actor, persona): Actor, ApiJson(b): ApiJson<RunWorkflowBody>) -> Result<(StatusCode, Json<WorkflowRun>), ApiError> {
    let trigger = b.trigger.unwrap_or(TriggerKind::Manual);
    Ok((StatusCode::ACCEPTED, Json(s.backend.run_workflow_as(&id, trigger, &actor, b.input, persona).await?)))
}
async fn list_runs(State(s): State<AppState>, ApiPath(id): ApiPath<String>, ApiQuery(q): ApiQuery<RunsQ>) -> Result<Json<Vec<WorkflowRun>>, ApiError> {
    Ok(Json(s.backend.list_runs(&id, q.limit.unwrap_or(DEFAULT_RUNS_LIMIT)).await?))
}
/// `GET /api/v1/runs?since=&limit=`: every run across every workflow that finished
/// after `since` (default the epoch, so an absent `since` is every finished run),
/// newest first. The desktop app's notification poller is the only caller today.
async fn list_all_runs(State(s): State<AppState>, ApiQuery(q): ApiQuery<AllRunsQ>) -> Result<Json<Vec<WorkflowRun>>, ApiError> {
    let since = q.since.unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
    Ok(Json(s.backend.runs_since(since, q.limit.unwrap_or(DEFAULT_ALL_RUNS_LIMIT)).await?))
}
async fn get_run(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<RunDetail>, ApiError> {
    let (run, steps) = s.backend.get_run(id).await?;
    Ok(Json(RunDetail { run, steps }))
}
async fn cancel_run(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor) -> Result<Json<WorkflowRun>, ApiError> {
    Ok(Json(s.backend.cancel_run(id, &actor).await?))
}
async fn export_run(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Response, ApiError> {
    let body = s.backend.export_run_log(id).await?;
    Ok(([(header::CONTENT_TYPE, "text/plain")], body).into_response())
}

// ---- search ----

/// `kinds` is a comma-separated list (`task,memory`); a blank or absent value
/// searches every kind, and an unrecognized one is a 400 naming it, the same as
/// every other query-parsed value in this file.
async fn global_search(State(s): State<AppState>, ApiQuery(q): ApiQuery<SearchQ>) -> Result<Json<SearchResult>, ApiError> {
    let kinds = match q.kinds.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(raw) => Some(raw.split(',').map(str::parse::<SearchKind>).collect::<std::result::Result<Vec<_>, AtlasError>>()?),
        None => None,
    };
    let query = SearchQuery { q: q.q.unwrap_or_default(), project_id: q.project_id, kinds, limit: q.limit.unwrap_or(DEFAULT_LIMIT) };
    Ok(Json(s.backend.search(query).await?))
}

// ---- MCP (Phase 10) ----

/// The stdio shim's registration on start. Idempotent on `id`, so a retry from the
/// shim never produces two entries for the one process. Only the stdio shim registers
/// itself explicitly: an HTTP MCP session has no separate handshake to register from
/// and is picked up on its first tool call instead (`ClientRegistry::record_http_call`),
/// so this route refuses `transport: "http"` rather than let any local caller add a row
/// that claims to be one.
async fn register_mcp_client(State(s): State<AppState>, ApiJson(b): ApiJson<RegisterMcpClientBody>) -> Result<(StatusCode, Json<McpClient>), ApiError> {
    let transport = b.transport.parse::<Transport>()
        .map_err(|_| ApiError(AtlasError::Invalid(format!("transport must be \"stdio\" or \"http\", got \"{}\"", b.transport))))?;
    if transport != Transport::Stdio {
        return Err(ApiError(AtlasError::Invalid(format!("this route only registers the stdio transport, got \"{}\"", b.transport))));
    }
    let record = s.mcp_clients.register(b.id, transport, b.client_name, b.client_version);
    Ok((StatusCode::CREATED, Json(record)))
}

/// The stdio shim's 60 s heartbeat. 404 for an id nobody registered: most likely the
/// daemon restarted since, and the shim's own next registration attempt (it does not
/// retry one) is the recovery, not this route.
async fn heartbeat_mcp_client(State(s): State<AppState>, ApiPath(id): ApiPath<String>, ApiJson(b): ApiJson<McpHeartbeatBody>) -> Result<Json<McpClient>, ApiError> {
    s.mcp_clients.heartbeat(&id, b.tool_calls).map(Json).ok_or_else(|| ApiError(AtlasError::NotFound(format!("mcp client {id}"))))
}

/// The stdio shim's best-effort unregister on exit. Always 204: the caller cannot
/// tell an id that was never registered from one that already expired, and neither
/// is an error worth reporting on the way out.
async fn unregister_mcp_client(State(s): State<AppState>, ApiPath(id): ApiPath<String>) -> StatusCode {
    s.mcp_clients.unregister(&id);
    StatusCode::NO_CONTENT
}

/// `GET /api/v1/mcp/status`: everything the desktop Settings card (Task 3) and `atlas
/// mcp status` need to render the MCP server. The tools table, resources and prompts
/// come from the same sources the router itself uses (`atlas_mcp::{TOOL_TABLE,
/// disabled_tool_names, resources_for, prompts_for}`), not a count hand-maintained
/// here, so the two cannot drift.
/// A plugin tool's `args` column: the JSON Schema's own property names, joined, with a
/// `*` on the required ones. A plugin declares a schema rather than the hand-written
/// summary `TOOL_TABLE` carries, so both MCP routes render it the same way from here.
fn plugin_args_summary(args: &serde_json::Value) -> String {
    let required: Vec<&str> = args.get("required").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    args.get("properties").and_then(|v| v.as_object())
        .map(|p| p.keys().map(|k| if required.contains(&k.as_str()) { format!("{k}*") } else { k.clone() }).collect::<Vec<_>>().join(", "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "none".into())
}

async fn mcp_status(State(s): State<AppState>) -> Result<Json<McpStatusReport>, ApiError> {
    let disabled = atlas_mcp::disabled_tool_names(&*s.backend).await?;
    let mut tools: Vec<McpToolRow> = TOOL_TABLE.iter()
        .map(|m| McpToolRow { name: m.name.into(), description: m.description.into(), args: m.args.into(), scope: m.scope, enabled: !disabled.contains(m.name), source: "builtin".into() })
        .collect();
    // Plugin tools are listed under the same MCP names a client sees and gated by the
    // same `mcp.disabled_tools` list. `args` is the schema's own property names, joined,
    // since a plugin declares a JSON Schema rather than the hand-written summary
    // `TOOL_TABLE` carries.
    tools.extend(s.plugin_tools.list().into_iter().map(|d| {
        let name = atlas_mcp::plugin_tool_name(&d.plugin_id, &d.name);
        let args = plugin_args_summary(&d.args);
        McpToolRow {
            enabled: !disabled.contains(&name),
            name,
            description: d.description,
            args,
            scope: match d.scope { PluginToolScope::Read => ToolScope::Read, PluginToolScope::Write => ToolScope::Write },
            source: format!("plugin:{}", d.plugin_id),
        }
    }));
    let resources = atlas_mcp::resources_for(&*s.backend).await?;
    let prompts = atlas_mcp::prompts_for(&*s.backend).await?;
    let clients = s.mcp_clients.live();
    let port = s.backend.port.unwrap_or(0);
    Ok(Json(McpStatusReport {
        transports: McpTransports {
            stdio: McpStdioTransport { command: "atlas mcp" },
            http: McpHttpTransport { url: format!("http://127.0.0.1:{port}/mcp"), protocol_version: rmcp::model::ProtocolVersion::LATEST.to_string() },
        },
        counts: McpCounts { tools: tools.len(), resources: resources.len(), prompts: prompts.len(), clients: clients.len() },
        tools,
        resources,
        prompts,
        clients,
    }))
}

/// `GET /api/v1/projects/{id}/mcp` (Task MCP-A): what MCP looks like from one
/// project's point of view. `enabled_here` reflects both gates a call actually meets
/// (the global list, then this project's own), the same order `atlas_mcp::AtlasMcp`
/// checks them in. `tools/list` itself stays global (see `atlas_mcp::AtlasMcp::call_tool`),
/// so this route, not the live tool list, is where a project's overrides show.
async fn project_mcp(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> Result<Json<ProjectMcpReport>, ApiError> {
    let project = s.backend.get_project(id).await?;
    let disabled_globally = atlas_mcp::disabled_tool_names(&*s.backend).await?;
    let disabled_here: std::collections::HashSet<&str> = project.mcp_disabled_tools.iter().map(String::as_str).collect();
    let mut tools: Vec<ProjectMcpToolRow> = TOOL_TABLE.iter()
        .map(|m| {
            let enabled_globally = !disabled_globally.contains(m.name);
            ProjectMcpToolRow {
                name: m.name.into(), description: m.description.into(), args: m.args.into(), scope: m.scope,
                enabled_globally,
                enabled_here: enabled_globally && !disabled_here.contains(m.name),
                source: "builtin".into(),
            }
        })
        .collect();
    // Plugin tools belong on this tab for the same reason they belong on the global one:
    // both gates apply to them (`plugin__<id>__<name>` is what `mcp.disabled_tools` and a
    // project's override both name), and without them the desktop's `Plugin` badge on the
    // project tab never lights.
    tools.extend(s.plugin_tools.list().into_iter().map(|d| {
        let name = atlas_mcp::plugin_tool_name(&d.plugin_id, &d.name);
        let enabled_globally = !disabled_globally.contains(&name);
        let enabled_here = enabled_globally && !disabled_here.contains(name.as_str());
        ProjectMcpToolRow {
            args: plugin_args_summary(&d.args),
            name,
            description: d.description,
            scope: match d.scope { PluginToolScope::Read => ToolScope::Read, PluginToolScope::Write => ToolScope::Write },
            enabled_globally,
            enabled_here,
            source: format!("plugin:{}", d.plugin_id),
        }
    }));
    let resources = atlas_mcp::resources_for_project(&project);
    let prompts = atlas_mcp::prompts_for(&*s.backend).await?;
    let clients = s.mcp_clients.live().into_iter().filter(|c| c.last_project_id == Some(id)).collect();
    let port = s.backend.port.unwrap_or(0);
    Ok(Json(ProjectMcpReport {
        tools,
        resources,
        prompts,
        clients,
        connect: ProjectMcpConnect {
            stdio: McpStdioTransport { command: "atlas mcp" },
            http: McpHttpTransport { url: format!("http://127.0.0.1:{port}/mcp"), protocol_version: rmcp::model::ProtocolVersion::LATEST.to_string() },
            project_root: project.root_path,
        },
    }))
}

/// `PUT /api/v1/projects/{id}/mcp/tools` (Task MCP-A): replaces this project's MCP
/// tool override wholesale. Goes through `ProjectPatch` like `PATCH
/// /api/v1/projects/{id}`, so the same known-tool-name validation and audit trail
/// apply; `disabled: []` clears the override.
async fn put_project_mcp_tools(State(s): State<AppState>, ApiPath(id): ApiPath<Uuid>, Actor(actor, _): Actor, ApiJson(b): ApiJson<McpToolsBody>) -> Result<Json<Project>, ApiError> {
    Ok(Json(s.backend.update_project(id, ProjectPatch { mcp_disabled_tools: Some(b.disabled), ..Default::default() }, &actor).await?))
}

// ---- plugin MCP tools (Phase 13b) ----

/// `PUT /api/v1/mcp/plugin-tools/{plugin_id}`: replaces that plugin's whole tool set.
/// The body's decls omit `plugin_id` (the path already names it) and it is filled in
/// here, so a body can never register tools under a plugin other than the one it
/// addressed. `register` validates before it stores, since these names reach every MCP
/// client's tool list; an empty set is an unregister, so a plugin whose last tool goes
/// away leaves no entry behind and an unvalidated path segment is never stored as one.
async fn put_plugin_tools(State(s): State<AppState>, ApiPath(plugin_id): ApiPath<String>, ApiJson(b): ApiJson<PluginToolsBody>) -> Result<StatusCode, ApiError> {
    if b.tools.is_empty() {
        atlas_core::settings::validate_plugin_id(&plugin_id)?;
        s.plugin_tools.unregister(&plugin_id);
        return Ok(StatusCode::NO_CONTENT);
    }
    let decls: Vec<PluginToolDecl> = b.tools.into_iter().map(|d| PluginToolDecl { plugin_id: plugin_id.clone(), ..d }).collect();
    s.plugin_tools.register(plugin_id, decls)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/mcp/plugin-tools/{plugin_id}`: the plugin stopped. Always 204, the
/// same reading `unregister_mcp_client` takes: a plugin that was never registered is
/// already in the state the caller asked for.
async fn delete_plugin_tools(State(s): State<AppState>, ApiPath(plugin_id): ApiPath<String>) -> StatusCode {
    s.plugin_tools.unregister(&plugin_id);
    StatusCode::NO_CONTENT
}

/// `GET /api/v1/mcp/plugin-tools`: every registered plugin tool, in the shape
/// `RemoteBackend::plugin_tools` reads back, so the stdio shim lists exactly what the
/// HTTP transport lists.
async fn list_plugin_tools(State(s): State<AppState>) -> Json<Vec<PluginToolDecl>> {
    Json(s.plugin_tools.list())
}

/// `POST /api/v1/mcp/plugin-tools/{plugin_id}/{name}/call`: the stdio shim's way to
/// reach a plugin tool, since only this process holds the app's socket. The reply is the
/// plugin's own JSON result, or the usual `{"error": string}` shape.
async fn call_plugin_tool(State(s): State<AppState>, ApiPath((plugin_id, name)): ApiPath<(String, String)>, Actor(actor, _): Actor, ApiJson(b): ApiJson<PluginToolCallBody>) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(s.backend.call_plugin_tool(&plugin_id, &name, b.args, &actor).await?))
}

/// Whether a plugin channel upgrade may proceed: no `Origin` (a non-browser client such as
/// the Tauri host or a test) or one of `CORS_ORIGINS`. The loopback guard alone is not
/// enough here: it admits any `http://localhost:<port>` page, WebSockets skip CORS, and
/// whoever holds the channel becomes the process every plugin tool call is forwarded to.
fn plugin_channel_origin_ok(headers: &HeaderMap) -> bool {
    headers.get(header::ORIGIN).is_none_or(|v| v.to_str().is_ok_and(is_cors_origin))
}

/// `GET /api/v1/mcp/plugin-channel`: the desktop app's end of the forwarding channel.
/// Behind the loopback guard like every other route, plus an `Origin` check of its own
/// (`plugin_channel_origin_ok`): only the app's webview, the Vite dev server or a client
/// that sends no `Origin` may hold the channel. Anything that passes can already read and
/// write the database over the JSON API.
async fn plugin_channel(State(s): State<AppState>, headers: HeaderMap, upgrade: axum::extract::ws::WebSocketUpgrade) -> Response {
    if !plugin_channel_origin_ok(&headers) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "forbidden origin"}))).into_response();
    }
    let shutdown = s.shutdown.clone();
    upgrade.on_upgrade(move |socket| s.plugin_tools.clone().serve(socket, shutdown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_channel_admits_only_the_app_origins_or_no_origin() {
        let with = |origin: &str| { let mut h = HeaderMap::new(); h.insert(header::ORIGIN, origin.parse().unwrap()); h };
        assert!(plugin_channel_origin_ok(&HeaderMap::new()), "non-browser clients send no Origin");
        for origin in CORS_ORIGINS { assert!(plugin_channel_origin_ok(&with(origin)), "{origin}"); }
        for origin in ["http://localhost:8888", "http://127.0.0.1:7433", "http://localhost", "https://example.com", "null"] {
            assert!(!plugin_channel_origin_ok(&with(origin)), "{origin} must not open the channel");
        }
    }
}
