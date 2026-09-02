use atlas_core::{backend::Backend, jobs::Job, models::*, AtlasError, Result};
use std::path::PathBuf;
use uuid::Uuid;

/// The collection each doc kind is served under.
fn docs_path(kind: DocKind) -> &'static str {
    match kind { DocKind::Practice => "practices", DocKind::Workflow => "workflows" }
}

#[derive(Clone)]
pub struct RemoteBackend { base: String, client: reqwest::Client }

impl RemoteBackend {
    pub fn new(port: u16) -> Self { Self { base: format!("http://127.0.0.1:{port}/api/v1"), client: reqwest::Client::new() } }
    async fn handle<T: serde::de::DeserializeOwned>(r: reqwest::Response) -> Result<T> {
        if r.status().is_success() { return r.json::<T>().await.map_err(|e| AtlasError::Other(e.to_string())); }
        Err(Self::error(r).await)
    }
    /// For endpoints that answer 204 with no body, where `handle` would fail decoding one.
    async fn handle_empty(r: reqwest::Response) -> Result<()> {
        if r.status().is_success() { return Ok(()); }
        Err(Self::error(r).await)
    }
    async fn error(r: reqwest::Response) -> AtlasError {
        let status = r.status();
        let msg = r.json::<serde_json::Value>().await.ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| status.to_string());
        match status.as_u16() { 404 => AtlasError::NotFound(msg), 400 => AtlasError::Invalid(msg), 409 => AtlasError::Conflict(msg), _ => AtlasError::Other(msg) }
    }
    fn net(e: reqwest::Error) -> AtlasError { AtlasError::Other(format!("daemon unreachable: {e}")) }
}

#[async_trait::async_trait]
impl Backend for RemoteBackend {
    async fn status(&self) -> Result<StatusReport> { Self::handle(self.client.get(format!("{}/status", self.base)).send().await.map_err(Self::net)?).await }
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> { Self::handle(self.client.post(format!("{}/memories?actor={actor}", self.base)).json(&m).send().await.map_err(Self::net)?).await }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> { Self::handle(self.client.post(format!("{}/memories/search", self.base)).json(&q).send().await.map_err(Self::net)?).await }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> { Self::handle(self.client.post(format!("{}/memories/{id}/forget?actor={actor}", self.base)).json(&serde_json::json!({"reason": reason})).send().await.map_err(Self::net)?).await }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> { Self::handle(self.client.get(format!("{}/memories/{id}", self.base)).send().await.map_err(Self::net)?).await }
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>) -> Result<Vec<Memory>> {
        let project = project_id.map(|p| format!("&project_id={p}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/memories?status={status}{project}", self.base)).send().await.map_err(Self::net)?).await
    }
    async fn set_memory_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory> {
        Self::handle(self.client.post(format!("{}/memories/{id}/status?actor={actor}", self.base)).json(&serde_json::json!({"status": status})).send().await.map_err(Self::net)?).await
    }

    async fn connect_project(&self, root: PathBuf, actor: &str) -> Result<Project> {
        Self::handle(self.client.post(format!("{}/projects/connect?actor={actor}", self.base)).json(&serde_json::json!({"root": root})).send().await.map_err(Self::net)?).await
    }
    async fn project_context(&self, root: PathBuf, actor: &str) -> Result<ProjectContext> {
        Self::handle(self.client.post(format!("{}/projects/context?actor={actor}", self.base)).json(&serde_json::json!({"root": root})).send().await.map_err(Self::net)?).await
    }
    async fn list_projects(&self) -> Result<Vec<Project>> { Self::handle(self.client.get(format!("{}/projects", self.base)).send().await.map_err(Self::net)?).await }
    async fn get_project(&self, id: Uuid) -> Result<Project> { Self::handle(self.client.get(format!("{}/projects/{id}", self.base)).send().await.map_err(Self::net)?).await }
    async fn refresh_project(&self, id: Uuid) -> Result<Project> { Self::handle(self.client.post(format!("{}/projects/{id}/refresh", self.base)).send().await.map_err(Self::net)?).await }
    async fn delete_project(&self, id: Uuid, actor: &str) -> Result<()> { Self::handle_empty(self.client.delete(format!("{}/projects/{id}?actor={actor}", self.base)).send().await.map_err(Self::net)?).await }

    async fn list_agents(&self) -> Result<Vec<Agent>> { Self::handle(self.client.get(format!("{}/agents", self.base)).send().await.map_err(Self::net)?).await }
    async fn get_agent(&self, name: &str) -> Result<Agent> { Self::handle(self.client.get(format!("{}/agents/{name}", self.base)).send().await.map_err(Self::net)?).await }
    async fn save_agent(&self, a: NewAgent, actor: &str) -> Result<Agent> { Self::handle(self.client.post(format!("{}/agents?actor={actor}", self.base)).json(&a).send().await.map_err(Self::net)?).await }
    async fn delete_agent(&self, name: &str, actor: &str) -> Result<()> { Self::handle_empty(self.client.delete(format!("{}/agents/{name}?actor={actor}", self.base)).send().await.map_err(Self::net)?).await }

    async fn list_docs(&self, kind: DocKind, project_id: Option<Uuid>) -> Result<Vec<Doc>> {
        let project = project_id.map(|p| format!("?project_id={p}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/{}{project}", self.base, docs_path(kind))).send().await.map_err(Self::net)?).await
    }
    async fn get_doc(&self, kind: DocKind, name: &str) -> Result<Doc> { Self::handle(self.client.get(format!("{}/{}/{name}", self.base, docs_path(kind))).send().await.map_err(Self::net)?).await }
    async fn save_doc(&self, kind: DocKind, d: NewDoc, actor: &str) -> Result<Doc> { Self::handle(self.client.post(format!("{}/{}?actor={actor}", self.base, docs_path(kind))).json(&d).send().await.map_err(Self::net)?).await }
    async fn delete_doc(&self, kind: DocKind, name: &str, actor: &str) -> Result<()> { Self::handle_empty(self.client.delete(format!("{}/{}/{name}?actor={actor}", self.base, docs_path(kind))).send().await.map_err(Self::net)?).await }

    /// The daemon runs the sync, so the paths written are the daemon host's.
    async fn sync(&self, req: SyncRequest) -> Result<SyncReport> { Self::handle(self.client.post(format!("{}/sync", self.base)).json(&req).send().await.map_err(Self::net)?).await }

    async fn get_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>> { Self::handle(self.client.get(format!("{}/settings", self.base)).send().await.map_err(Self::net)?).await }
    async fn set_settings(&self, values: serde_json::Map<String, serde_json::Value>, actor: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
        Self::handle(self.client.put(format!("{}/settings?actor={actor}", self.base)).json(&values).send().await.map_err(Self::net)?).await
    }

    /// The daemon runs the extraction, so a 409 here is its "extraction is
    /// disabled", carried back as the same error a `LocalBackend` would raise.
    async fn ingest_transcript(&self, text: String, source_tool: String, project_root: Option<PathBuf>) -> Result<Uuid> {
        let body = serde_json::json!({"text": text, "source_tool": source_tool, "project_root": project_root});
        let r = self.client.post(format!("{}/ingest", self.base)).json(&body).send().await.map_err(Self::net)?;
        let v: serde_json::Value = Self::handle(r).await?;
        v["job_id"].as_str().and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| AtlasError::Other(format!("ingest response had no job_id: {v}")))
    }

    /// A job the daemon has never heard of is `None`, not an error.
    async fn get_job(&self, id: Uuid) -> Result<Option<Job>> {
        let r = self.client.get(format!("{}/jobs/{id}", self.base)).send().await.map_err(Self::net)?;
        if r.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
        Self::handle(r).await.map(Some)
    }
}
