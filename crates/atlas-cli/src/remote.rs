use atlas_core::{backend::Backend, jobs::Job, models::*, search::global::{SearchQuery, SearchResult}, AtlasError, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// The collection each doc kind is served under.
fn docs_path(kind: DocKind) -> &'static str {
    match kind { DocKind::Practice => "practices", DocKind::Workflow => "workflows" }
}

#[derive(Clone)]
pub struct RemoteBackend {
    base: String,
    client: reqwest::Client,
    /// The caller's identity for board routes, which read it from `X-Atlas-Actor`
    /// rather than a query parameter. Set by the CLI (`cli`, or `cli/NAME` for
    /// `--as NAME`); a board call whose trait method takes its own `actor` argument
    /// sends that value instead, the same way the query-parameter routes already do.
    pub actor: String,
}

impl RemoteBackend {
    /// A request deadline matters because `atlas ingest --hook-stdin` runs inside
    /// someone else's turn: a daemon that accepts the connection and then never
    /// answers (a lock it cannot take, a wedged worker) must not hold the turn open.
    /// 30 s is well past any healthy call and well short of a stall a user would sit
    /// through. `Client::builder` only fails on a bad TLS or resolver setup, which a
    /// loopback client has none of, so the default is a sound fallback.
    pub fn new(port: u16) -> Self {
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().unwrap_or_default();
        Self { base: format!("http://127.0.0.1:{port}/api/v1"), client, actor: "cli".into() }
    }
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
        // The daemon renders `AtlasError` through `Display` before putting it on the
        // wire, so `{"error": ...}` already carries the prefix the variant rebuilt
        // here would add a second time: without this, the CLI prints
        // "invalid input: invalid input: a task needs a title".
        let strip = |prefix: &str, msg: String| msg.strip_prefix(prefix).map(str::to_string).unwrap_or(msg);
        match status.as_u16() {
            404 => AtlasError::NotFound(strip("not found: ", msg)),
            400 => AtlasError::Invalid(strip("invalid input: ", msg)),
            409 => AtlasError::Conflict(msg),
            413 => AtlasError::TooLarge(msg),
            _ => AtlasError::Other(msg),
        }
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

    /// The daemon runs the connectivity check, so a 409 here is its "extraction is
    /// disabled" and a 400 is the model endpoint's own error, both carried back
    /// through the same `error` field `Self::error` already reads.
    async fn test_extraction(&self) -> Result<String> {
        let r = self.client.post(format!("{}/extraction/test", self.base)).send().await.map_err(Self::net)?;
        let v: serde_json::Value = Self::handle(r).await?;
        v["reply"].as_str().map(str::to_string).ok_or_else(|| AtlasError::Other(format!("extraction test response had no reply: {v}")))
    }

    // ---- board ----

    async fn list_tasks(&self, f: TaskFilter) -> Result<Vec<Task>> {
        // Matches the rest of this file: query values go straight into the URL, same as
        // `list_memories`'s `status`/`project_id` and `list_docs`'s `project_id`.
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = f.project_id { parts.push(format!("project_id={p}")); }
        if let Some(s) = f.stage { parts.push(format!("stage={s}")); }
        if let Some(a) = f.assignee { parts.push(format!("assignee={a}")); }
        if f.ready { parts.push("ready=true".into()); }
        if let Some(text) = f.query { parts.push(format!("q={text}")); }
        if f.include_done { parts.push("include_done=true".into()); }
        let q = if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) };
        Self::handle(self.client.get(format!("{}/tasks{q}", self.base)).header("X-Atlas-Actor", &self.actor).send().await.map_err(Self::net)?).await
    }
    async fn get_task(&self, id_or_key: &str) -> Result<TaskDetail> {
        Self::handle(self.client.get(format!("{}/tasks/{id_or_key}", self.base)).header("X-Atlas-Actor", &self.actor).send().await.map_err(Self::net)?).await
    }
    async fn create_task(&self, t: NewTask, actor: &str) -> Result<Task> {
        Self::handle(self.client.post(format!("{}/tasks", self.base)).header("X-Atlas-Actor", actor).json(&t).send().await.map_err(Self::net)?).await
    }
    async fn update_task(&self, id_or_key: &str, u: TaskUpdate, actor: &str) -> Result<Task> {
        Self::handle(self.client.patch(format!("{}/tasks/{id_or_key}", self.base)).header("X-Atlas-Actor", actor).json(&u).send().await.map_err(Self::net)?).await
    }
    async fn move_task(&self, id_or_key: &str, stage: &str, expected: Option<DateTime<Utc>>, actor: &str) -> Result<Task> {
        Self::handle(
            self.client.post(format!("{}/tasks/{id_or_key}/move", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"stage": stage, "expected_updated_at": expected}))
                .send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn comment_task(&self, id_or_key: &str, body: &str, actor: &str) -> Result<TaskEvent> {
        Self::handle(
            self.client.post(format!("{}/tasks/{id_or_key}/comment", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"body": body})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn claim_task(&self, id_or_key: &str, force: bool, actor: &str) -> Result<Task> {
        Self::handle(
            self.client.post(format!("{}/tasks/{id_or_key}/claim", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"force": force})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn set_task_blockers(&self, id_or_key: &str, blocked_by: Vec<String>, actor: &str) -> Result<Task> {
        Self::handle(
            self.client.put(format!("{}/tasks/{id_or_key}/blockers", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"blocked_by": blocked_by})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn delete_task(&self, id_or_key: &str, actor: &str) -> Result<()> {
        Self::handle_empty(self.client.delete(format!("{}/tasks/{id_or_key}", self.base)).header("X-Atlas-Actor", actor).send().await.map_err(Self::net)?).await
    }
    async fn board_stages(&self, project_id: Option<Uuid>) -> Result<StageList> {
        let project = project_id.map(|p| format!("?project_id={p}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/board/stages{project}", self.base)).header("X-Atlas-Actor", &self.actor).send().await.map_err(Self::net)?).await
    }
    async fn set_board_stages(&self, stages: Vec<Stage>, renames: HashMap<String, String>, actor: &str) -> Result<Vec<Stage>> {
        Self::handle(
            self.client.put(format!("{}/board/stages", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"stages": stages, "renames": renames})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn set_project_stages(&self, project_id: Uuid, stages: Option<Vec<Stage>>, renames: HashMap<String, String>, actor: &str) -> Result<StageList> {
        Self::handle(
            self.client.put(format!("{}/projects/{project_id}/stages", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"stages": stages, "renames": renames})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn task_counts(&self, project_id: Option<Uuid>) -> Result<Vec<(String, i64)>> {
        let project = project_id.map(|p| format!("?project_id={p}")).unwrap_or_default();
        let rows: Vec<StageCount> =
            Self::handle(self.client.get(format!("{}/tasks/counts{project}", self.base)).header("X-Atlas-Actor", &self.actor).send().await.map_err(Self::net)?).await?;
        Ok(rows.into_iter().map(|r| (r.stage, r.count)).collect())
    }

    // ---- search ----

    async fn search(&self, q: SearchQuery) -> Result<SearchResult> {
        // Matches the rest of this file: query values go straight into the URL rather
        // than through a query-builder, same as `list_tasks`.
        let mut parts = vec![format!("q={}", q.q), format!("limit={}", q.limit)];
        if let Some(p) = q.project_id {
            parts.push(format!("project_id={p}"));
        }
        if let Some(kinds) = &q.kinds {
            parts.push(format!("kinds={}", kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(",")));
        }
        Self::handle(self.client.get(format!("{}/search?{}", self.base, parts.join("&"))).send().await.map_err(Self::net)?).await
    }
}

/// The shape `GET /tasks/counts` answers with, one row per board stage.
#[derive(serde::Deserialize)]
struct StageCount { stage: String, count: i64 }
