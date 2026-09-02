use axum::{extract::{Path, Query, State}, http::StatusCode, response::{IntoResponse, Response}, routing::{get, post}, Json, Router};
use atlas_core::{backend::Backend, models::*, AtlasError};
use serde::Deserialize;
use uuid::Uuid;
use crate::state::AppState;

pub struct ApiError(AtlasError);
impl From<AtlasError> for ApiError { fn from(e: AtlasError) -> Self { Self(e) } }
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = match self.0 { AtlasError::NotFound(_) => StatusCode::NOT_FOUND, AtlasError::Invalid(_) => StatusCode::BAD_REQUEST, _ => StatusCode::INTERNAL_SERVER_ERROR };
        (code, Json(serde_json::json!({"error": self.0.to_string()}))).into_response()
    }
}

#[derive(Deserialize)] pub struct ActorQ { pub actor: Option<String> }
#[derive(Deserialize)] pub struct ForgetBody { pub reason: Option<String> }

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/memories", post(create_memory))
        .route("/api/v1/memories/search", post(search))
        .route("/api/v1/memories/{id}", get(get_memory))
        .route("/api/v1/memories/{id}/forget", post(forget))
        .with_state(state)
}

async fn status(State(s): State<AppState>) -> Result<Json<StatusReport>, ApiError> { Ok(Json(s.backend.status().await?)) }
async fn create_memory(State(s): State<AppState>, Query(q): Query<ActorQ>, Json(m): Json<NewMemory>) -> Result<(StatusCode, Json<Memory>), ApiError> {
    Ok((StatusCode::CREATED, Json(s.backend.remember(m, q.actor.as_deref().unwrap_or("api")).await?)))
}
async fn get_memory(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Memory>, ApiError> { Ok(Json(s.backend.get_memory(id).await?)) }
async fn search(State(s): State<AppState>, Json(q): Json<RecallQuery>) -> Result<Json<Vec<RecallHit>>, ApiError> { Ok(Json(s.backend.recall(q).await?)) }
async fn forget(State(s): State<AppState>, Path(id): Path<Uuid>, Query(q): Query<ActorQ>, body: Option<Json<ForgetBody>>) -> Result<Json<Memory>, ApiError> {
    Ok(Json(s.backend.forget(id, body.and_then(|b| b.0.reason), q.actor.as_deref().unwrap_or("api")).await?))
}
