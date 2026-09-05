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

    /// Builds a URL under the daemon's base with `reqwest::Url`'s query-pair encoder,
    /// for the same reason `search_url` uses it: a caller's text can carry `#`, `&`,
    /// spaces or non-ASCII, and raw interpolation would truncate or split it.
    fn url_with(base: &str, path: &str, pairs: &[(&str, String)]) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{base}{path}")).map_err(|e| AtlasError::Other(e.to_string()))?;
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in pairs {
                q.append_pair(k, v);
            }
        }
        Ok(url)
    }

    /// Builds the `GET /search` URL with `reqwest::Url`'s query-pair encoder rather
    /// than string interpolation: `q` can carry `#`, `&`, spaces or non-ASCII text a
    /// caller typed, and raw interpolation would either truncate it (`#` starts a URL
    /// fragment reqwest never sends) or split it into bogus extra parameters (`&`).
    fn search_url(base: &str, q: &SearchQuery) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{base}/search")).map_err(|e| AtlasError::Other(e.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", &q.q);
            pairs.append_pair("limit", &q.limit.to_string());
            if let Some(p) = q.project_id {
                pairs.append_pair("project_id", &p.to_string());
            }
            if let Some(kinds) = &q.kinds {
                pairs.append_pair("kinds", &kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(","));
            }
        }
        Ok(url)
    }

    /// A `/workflows/{id_or_name}` URL, with `more` path segments appended (e.g.
    /// `["run"]`), percent-encoding `id_or_name` through `Url::path_segments_mut`.
    /// Unlike an agent or doc name, a workflow name is only bounded by length, not by
    /// a safe character set, so it can carry a slash, a space, or anything else that
    /// would otherwise split or mangle the path if it were interpolated raw.
    fn workflow_url(base: &str, id_or_name: &str, more: &[&str]) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{base}/workflows")).map_err(|e| AtlasError::Other(e.to_string()))?;
        {
            let mut segs = url.path_segments_mut().map_err(|_| AtlasError::Other("the daemon base URL cannot be a base".into()))?;
            segs.push(id_or_name);
            for part in more {
                segs.push(part);
            }
        }
        Ok(url)
    }

    /// A `/projects/{id}/frameworks/{kind}/docs/{path}` URL, pushing each `/`-separated
    /// component of `path` as its own path segment so `reqwest` percent-encodes the
    /// characters within a component while the slashes between components stay
    /// structural, matching how the daemon's wildcard route (`{*path}`) reads them
    /// back.
    fn framework_doc_url(base: &str, project_id: Uuid, kind: FrameworkKind, path: &str) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{base}/projects/{project_id}/frameworks/{}/docs", kind.as_str()))
            .map_err(|e| AtlasError::Other(e.to_string()))?;
        {
            let mut segs = url.path_segments_mut().map_err(|_| AtlasError::Other("the daemon base URL cannot be a base".into()))?;
            for part in path.split('/') {
                segs.push(part);
            }
        }
        Ok(url)
    }

    /// A `/skills/{id}` URL, pushing each `/`-separated part of the id as its own path
    /// segment, the same way `framework_doc_url` does: a plugin skill's id carries
    /// slashes between its marketplace, plugin and skill names, and those have to stay
    /// structural for the daemon's wildcard route (`{*id}`) to read them back, while
    /// everything within a part is percent-encoded.
    fn skill_url(base: &str, id: &str, project_id: Option<Uuid>) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{base}/skills")).map_err(|e| AtlasError::Other(e.to_string()))?;
        {
            let mut segs = url.path_segments_mut().map_err(|_| AtlasError::Other("the daemon base URL cannot be a base".into()))?;
            for part in id.split('/') {
                segs.push(part);
            }
        }
        if let Some(p) = project_id {
            url.query_pairs_mut().append_pair("project_id", &p.to_string());
        }
        Ok(url)
    }

    /// An MCP server id, which carries `:` and, for a plugin server, slashes, travels as
    /// one percent-encoded path segment rather than the structural slashes a skill id
    /// uses: the check and enable routes have a segment *after* the id, and a wildcard
    /// route may only end a path. `action` is that trailing segment.
    fn mcp_server_url(base: &str, id: &str, project_id: Option<Uuid>, action: Option<&str>) -> Result<reqwest::Url> {
        // Encoded by hand rather than through `path_segments_mut`, whose encode set leaves
        // `/` alone and would split the id into segments the daemon cannot reassemble.
        const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'.')
            .remove(b'_')
            .remove(b'~');
        let encoded = percent_encoding::utf8_percent_encode(id, SEGMENT).to_string();
        let path = match action {
            Some(action) => format!("{base}/mcp/servers/{encoded}/{action}"),
            None => format!("{base}/mcp/servers/{encoded}"),
        };
        let mut url = reqwest::Url::parse(&path).map_err(|e| AtlasError::Other(e.to_string()))?;
        if let Some(p) = project_id {
            url.query_pairs_mut().append_pair("project_id", &p.to_string());
        }
        Ok(url)
    }

    /// Registers this stdio MCP session with the daemon so `GET /api/v1/mcp/status`
    /// can show it. `id` is generated by the caller (`atlas mcp`), which must know it
    /// up front to heartbeat and unregister with the same id later.
    pub async fn register_mcp_client(&self, id: &str, transport: &str, client_name: &str, client_version: Option<&str>) -> Result<()> {
        let body = serde_json::json!({"id": id, "transport": transport, "client_name": client_name, "client_version": client_version});
        Self::handle_empty(self.client.post(format!("{}/mcp/clients", self.base)).json(&body).send().await.map_err(Self::net)?).await
    }
    /// The stdio shim's periodic heartbeat: `tool_calls` is the running total the
    /// shim tracks itself, not a delta, so a heartbeat that races another one still
    /// leaves the daemon with the right count.
    pub async fn heartbeat_mcp_client(&self, id: &str, tool_calls: u64) -> Result<()> {
        Self::handle_empty(self.client.put(format!("{}/mcp/clients/{id}", self.base)).json(&serde_json::json!({"tool_calls": tool_calls})).send().await.map_err(Self::net)?).await
    }
    /// The shim calls this best effort on exit; there is nothing useful to do about a
    /// failed unregister on the way out.
    pub async fn unregister_mcp_client(&self, id: &str) -> Result<()> {
        Self::handle_empty(self.client.delete(format!("{}/mcp/clients/{id}", self.base)).send().await.map_err(Self::net)?).await
    }

    /// Task MCP-A: `GET /projects/{id}/mcp`, what MCP looks like from one project's
    /// point of view (the tools table with `enabled_globally`/`enabled_here`, this
    /// project's own resources and prompts, its connected clients, and the connect
    /// info). Raw JSON, like `get_settings`: the report's shape lives in `atlasd::http`
    /// and this crate does not depend on it.
    pub async fn project_mcp(&self, id: Uuid) -> Result<serde_json::Value> {
        Self::handle(self.client.get(format!("{}/projects/{id}/mcp", self.base)).send().await.map_err(Self::net)?).await
    }
    /// `PUT /projects/{id}/mcp/tools`: replaces this project's MCP tool override
    /// wholesale. `disabled: []` clears it.
    pub async fn set_project_mcp_tools(&self, id: Uuid, disabled: Vec<String>, actor: &str) -> Result<Project> {
        Self::handle(
            self.client
                .put(format!("{}/projects/{id}/mcp/tools", self.base))
                .header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"disabled": disabled}))
                .send()
                .await
                .map_err(Self::net)?,
        )
        .await
    }
}

#[async_trait::async_trait]
impl Backend for RemoteBackend {
    async fn status(&self) -> Result<StatusReport> { Self::handle(self.client.get(format!("{}/status", self.base)).send().await.map_err(Self::net)?).await }
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> { Self::handle(self.client.post(format!("{}/memories?actor={actor}", self.base)).json(&m).send().await.map_err(Self::net)?).await }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> {
        atlas_core::backend::check_scope(q.project_id, q.list_scope)?;
        Self::handle(self.client.post(format!("{}/memories/search", self.base)).json(&q).send().await.map_err(Self::net)?).await
    }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> { Self::handle(self.client.post(format!("{}/memories/{id}/forget?actor={actor}", self.base)).json(&serde_json::json!({"reason": reason})).send().await.map_err(Self::net)?).await }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> { Self::handle(self.client.get(format!("{}/memories/{id}", self.base)).send().await.map_err(Self::net)?).await }
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>, scope: MemoryScopeFilter, page: MemoryPage) -> Result<Vec<Memory>> {
        atlas_core::backend::check_scope(project_id, scope)?;
        let project = project_id.map(|p| format!("&project_id={p}")).unwrap_or_default();
        let limit = page.limit.map(|l| format!("&limit={l}")).unwrap_or_default();
        let offset = page.offset.map(|o| format!("&offset={o}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/memories?status={status}&scope={scope}{project}{limit}{offset}", self.base)).send().await.map_err(Self::net)?).await
    }
    async fn set_memory_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory> {
        Self::handle(self.client.post(format!("{}/memories/{id}/status?actor={actor}", self.base)).json(&serde_json::json!({"status": status})).send().await.map_err(Self::net)?).await
    }
    async fn memory_facets(&self, project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<MemoryFacets> {
        atlas_core::backend::check_scope(project_id, scope)?;
        let project = project_id.map(|p| format!("&project_id={p}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/memories/facets?scope={scope}{project}", self.base)).send().await.map_err(Self::net)?).await
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
    async fn update_project(&self, id: Uuid, patch: ProjectPatch, actor: &str) -> Result<Project> {
        Self::handle(self.client.patch(format!("{}/projects/{id}", self.base)).header("X-Atlas-Actor", actor).json(&patch).send().await.map_err(Self::net)?).await
    }
    async fn set_agent_access(&self, id: Uuid, access: AgentAccess, actor: &str) -> Result<Project> {
        Self::handle(self.client.put(format!("{}/projects/{id}/agent-access", self.base)).header("X-Atlas-Actor", actor).json(&access).send().await.map_err(Self::net)?).await
    }
    async fn project_access(&self, id: Uuid) -> Result<ProjectAccess> {
        Self::handle(self.client.get(format!("{}/projects/{id}/access", self.base)).send().await.map_err(Self::net)?).await
    }
    async fn set_project_extraction(&self, id: Uuid, over: Option<ProjectExtraction>, actor: &str) -> Result<Project> {
        Self::handle(self.client.put(format!("{}/projects/{id}/extraction", self.base)).header("X-Atlas-Actor", actor).json(&over).send().await.map_err(Self::net)?).await
    }
    async fn project_log(&self, id: Uuid, f: LogFilter) -> Result<Vec<LogEntry>> {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(v) = f.source { pairs.push(("source", v)); }
        if let Some(v) = f.kind { pairs.push(("kind", v)); }
        if let Some(v) = f.q { pairs.push(("q", v)); }
        if let Some(v) = f.after { pairs.push(("after", v.to_rfc3339())); }
        if let Some(v) = f.limit { pairs.push(("limit", v.to_string())); }
        let url = Self::url_with(&self.base, &format!("/projects/{id}/log"), &pairs)?;
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }
    /// The export is JSON lines, not JSON, so the body comes back as text.
    async fn project_log_export(&self, id: Uuid) -> Result<String> {
        let r = self.client.get(format!("{}/projects/{id}/log/export", self.base)).send().await.map_err(Self::net)?;
        if !r.status().is_success() { return Err(Self::error(r).await); }
        r.text().await.map_err(|e| AtlasError::Other(e.to_string()))
    }

    async fn list_agents(&self) -> Result<Vec<Agent>> { Self::handle(self.client.get(format!("{}/agents", self.base)).send().await.map_err(Self::net)?).await }
    async fn get_agent(&self, name: &str) -> Result<Agent> { Self::handle(self.client.get(format!("{}/agents/{name}", self.base)).send().await.map_err(Self::net)?).await }
    async fn save_agent(&self, a: NewAgent, actor: &str) -> Result<Agent> { Self::handle(self.client.post(format!("{}/agents?actor={actor}", self.base)).json(&a).send().await.map_err(Self::net)?).await }
    async fn delete_agent(&self, name: &str, actor: &str) -> Result<()> { Self::handle_empty(self.client.delete(format!("{}/agents/{name}?actor={actor}", self.base)).send().await.map_err(Self::net)?).await }

    // `DocKind::Workflow` used to be a Markdown document, served at `/api/v1/workflows`.
    // The Phase 9 migration (run once at daemon startup) turns every one of those into a
    // real workflow and deletes the document, and that HTTP path now serves the real
    // workflow API below instead. Nothing is ever stored under this doc kind again, so
    // these four answer it locally rather than reaching a path that no longer means what
    // its name says: a caller (`atlas export`/`import`, and the MCP `workflow_list`/
    // `workflow_get` tools this crate's stdio shim serves) sees an always-empty
    // collection rather than a 404.
    async fn list_docs(&self, kind: DocKind, project_id: Option<Uuid>) -> Result<Vec<Doc>> {
        if kind == DocKind::Workflow {
            return Ok(vec![]);
        }
        let project = project_id.map(|p| format!("?project_id={p}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/{}{project}", self.base, docs_path(kind))).send().await.map_err(Self::net)?).await
    }
    async fn get_doc(&self, kind: DocKind, name: &str) -> Result<Doc> {
        if kind == DocKind::Workflow {
            return Err(AtlasError::NotFound(format!("workflow document {name}")));
        }
        Self::handle(self.client.get(format!("{}/{}/{name}", self.base, docs_path(kind))).send().await.map_err(Self::net)?).await
    }
    async fn save_doc(&self, kind: DocKind, d: NewDoc, actor: &str) -> Result<Doc> {
        if kind == DocKind::Workflow {
            return Err(AtlasError::Invalid("workflow documents are retired; create a workflow through the workflow API instead".into()));
        }
        Self::handle(self.client.post(format!("{}/{}?actor={actor}", self.base, docs_path(kind))).json(&d).send().await.map_err(Self::net)?).await
    }
    async fn delete_doc(&self, kind: DocKind, name: &str, actor: &str) -> Result<()> {
        if kind == DocKind::Workflow {
            return Err(AtlasError::NotFound(format!("workflow document {name}")));
        }
        Self::handle_empty(self.client.delete(format!("{}/{}/{name}?actor={actor}", self.base, docs_path(kind))).send().await.map_err(Self::net)?).await
    }

    /// The daemon runs the sync, so the paths written are the daemon host's.
    async fn sync(&self, req: SyncRequest) -> Result<SyncReport> { Self::handle(self.client.post(format!("{}/sync", self.base)).json(&req).send().await.map_err(Self::net)?).await }

    async fn get_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>> { Self::handle(self.client.get(format!("{}/settings", self.base)).send().await.map_err(Self::net)?).await }
    async fn set_settings(&self, values: serde_json::Map<String, serde_json::Value>, actor: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
        Self::handle(self.client.put(format!("{}/settings?actor={actor}", self.base)).json(&values).send().await.map_err(Self::net)?).await
    }

    /// The daemon runs the extraction, so a 409 here is its "extraction is
    /// disabled", carried back as the same error a `LocalBackend` would raise. The
    /// actor goes in `X-Atlas-Actor`, not the deprecated `source_tool` body field.
    async fn ingest_transcript(&self, text: String, source_tool: String, project_root: Option<PathBuf>) -> Result<Uuid> {
        let body = serde_json::json!({"text": text, "project_root": project_root});
        let r = self.client.post(format!("{}/ingest", self.base)).header("X-Atlas-Actor", &source_tool).json(&body).send().await.map_err(Self::net)?;
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
    async fn test_extraction_for(&self, project_id: Option<Uuid>) -> Result<String> {
        let project = project_id.map(|p| format!("?project_id={p}")).unwrap_or_default();
        let r = self.client.post(format!("{}/extraction/test{project}", self.base)).send().await.map_err(Self::net)?;
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
        if f.global_only { parts.push("scope=global".into()); }
        if let Some(tl) = f.top_level { parts.push(format!("top_level={tl}")); }
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
    async fn task_counts(&self, project_id: Option<Uuid>, global_only: bool, top_level: Option<bool>) -> Result<Vec<(String, i64)>> {
        let mut params = Vec::new();
        if let Some(p) = project_id { params.push(format!("project_id={p}")); }
        if global_only { params.push("scope=global".to_string()); }
        if let Some(tl) = top_level { params.push(format!("top_level={tl}")); }
        let query = if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
        let rows: Vec<StageCount> =
            Self::handle(self.client.get(format!("{}/tasks/counts{query}", self.base)).header("X-Atlas-Actor", &self.actor).send().await.map_err(Self::net)?).await?;
        Ok(rows.into_iter().map(|r| (r.stage, r.count)).collect())
    }

    // ---- workflows (Phase 9) ----

    async fn list_workflows(&self, project_id: Option<Uuid>) -> Result<Vec<Workflow>> {
        let project = project_id.map(|p| format!("?project_id={p}")).unwrap_or_default();
        Self::handle(self.client.get(format!("{}/workflows{project}", self.base)).send().await.map_err(Self::net)?).await
    }
    async fn get_workflow(&self, id_or_name: &str) -> Result<Workflow> {
        Self::handle(self.client.get(Self::workflow_url(&self.base, id_or_name, &[])?).send().await.map_err(Self::net)?).await
    }
    async fn create_workflow(&self, w: NewWorkflow, actor: &str) -> Result<Workflow> {
        Self::handle(self.client.post(format!("{}/workflows", self.base)).header("X-Atlas-Actor", actor).json(&w).send().await.map_err(Self::net)?).await
    }
    async fn update_workflow(&self, id_or_name: &str, patch: WorkflowPatch, actor: &str) -> Result<Workflow> {
        Self::handle(
            self.client.patch(Self::workflow_url(&self.base, id_or_name, &[])?).header("X-Atlas-Actor", actor).json(&patch).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn delete_workflow(&self, id_or_name: &str, actor: &str) -> Result<()> {
        Self::handle_empty(self.client.delete(Self::workflow_url(&self.base, id_or_name, &[])?).header("X-Atlas-Actor", actor).send().await.map_err(Self::net)?).await
    }
    async fn run_workflow(&self, id_or_name: &str, trigger: TriggerKind, actor: &str, input: Option<String>) -> Result<WorkflowRun> {
        Self::handle(
            self.client.post(Self::workflow_url(&self.base, id_or_name, &["run"])?).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"trigger": trigger, "input": input})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn list_runs(&self, id_or_name: &str, limit: usize) -> Result<Vec<WorkflowRun>> {
        let mut url = Self::workflow_url(&self.base, id_or_name, &["runs"])?;
        url.query_pairs_mut().append_pair("limit", &limit.to_string());
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }
    async fn runs_since(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<WorkflowRun>> {
        let mut url = reqwest::Url::parse(&format!("{}/runs", self.base)).map_err(|e| AtlasError::Other(e.to_string()))?;
        url.query_pairs_mut().append_pair("since", &since.to_rfc3339()).append_pair("limit", &limit.to_string());
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }
    async fn get_run(&self, run_id: Uuid) -> Result<(WorkflowRun, Vec<WorkflowStep>)> {
        let detail: RunDetail = Self::handle(self.client.get(format!("{}/runs/{run_id}", self.base)).send().await.map_err(Self::net)?).await?;
        Ok((detail.run, detail.steps))
    }
    async fn cancel_run(&self, run_id: Uuid, actor: &str) -> Result<WorkflowRun> {
        Self::handle(self.client.post(format!("{}/runs/{run_id}/cancel", self.base)).header("X-Atlas-Actor", actor).send().await.map_err(Self::net)?).await
    }
    async fn export_run_log(&self, run_id: Uuid) -> Result<String> {
        let r = self.client.get(format!("{}/runs/{run_id}/export", self.base)).send().await.map_err(Self::net)?;
        if !r.status().is_success() { return Err(Self::error(r).await); }
        r.text().await.map_err(|e| AtlasError::Other(e.to_string()))
    }

    // ---- search ----

    async fn search(&self, q: SearchQuery) -> Result<SearchResult> {
        let url = Self::search_url(&self.base, &q)?;
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }

    // ---- frameworks (Phase 12) ----

    async fn list_frameworks(&self, project_id: Uuid) -> Result<Vec<FrameworkListing>> {
        Self::handle(self.client.get(format!("{}/projects/{project_id}/frameworks", self.base)).send().await.map_err(Self::net)?).await
    }
    async fn get_framework_doc(&self, project_id: Uuid, kind: FrameworkKind, path: &str) -> Result<String> {
        let url = Self::framework_doc_url(&self.base, project_id, kind, path)?;
        let r = self.client.get(url).send().await.map_err(Self::net)?;
        if !r.status().is_success() { return Err(Self::error(r).await); }
        let v: serde_json::Value = r.json().await.map_err(|e| AtlasError::Other(e.to_string()))?;
        v["content"].as_str().map(str::to_string).ok_or_else(|| AtlasError::Other(format!("framework doc response had no content: {v}")))
    }
    async fn import_framework(&self, project_id: Uuid, kind: FrameworkKind, what: ImportWhat, actor: &str) -> Result<ImportReport> {
        Self::handle(
            self.client.post(format!("{}/projects/{project_id}/frameworks/{}/import", self.base, kind.as_str())).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"what": what})).send().await.map_err(Self::net)?,
        )
        .await
    }

    // ---- skills (Phase 15) ----

    async fn list_skills(&self, project_id: Option<Uuid>) -> Result<SkillList> {
        let pairs: Vec<(&str, String)> = project_id.map(|p| ("project_id", p.to_string())).into_iter().collect();
        let url = Self::url_with(&self.base, "/skills", &pairs)?;
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }
    async fn get_skill(&self, project_id: Option<Uuid>, id: &str) -> Result<Skill> {
        let url = Self::skill_url(&self.base, id, project_id)?;
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }
    async fn create_skill(&self, s: NewSkill, actor: &str) -> Result<Skill> {
        Self::handle(self.client.post(format!("{}/skills", self.base)).header("X-Atlas-Actor", actor).json(&s).send().await.map_err(Self::net)?).await
    }
    async fn update_skill(&self, id: &str, patch: SkillUpdate, actor: &str) -> Result<Skill> {
        let url = Self::skill_url(&self.base, id, None)?;
        Self::handle(self.client.patch(url).header("X-Atlas-Actor", actor).json(&patch).send().await.map_err(Self::net)?).await
    }
    async fn write_skill_body(&self, project_id: Option<Uuid>, id: &str, body: String, actor: &str) -> Result<Skill> {
        let url = Self::skill_url(&self.base, id, project_id)?;
        Self::handle(
            self.client.put(url).header("X-Atlas-Actor", actor).json(&serde_json::json!({"body": body})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn delete_skill(&self, id: &str, actor: &str) -> Result<()> {
        let url = Self::skill_url(&self.base, id, None)?;
        Self::handle_empty(self.client.delete(url).header("X-Atlas-Actor", actor).send().await.map_err(Self::net)?).await
    }
    async fn set_project_skills_disabled(&self, project_id: Uuid, ids: Vec<String>, actor: &str) -> Result<Project> {
        Self::handle(
            self.client.put(format!("{}/projects/{project_id}/skills", self.base)).header("X-Atlas-Actor", actor)
                .json(&serde_json::json!({"disabled": ids})).send().await.map_err(Self::net)?,
        )
        .await
    }

    // ---- the agents' MCP servers (Phase 16) ----

    async fn list_mcp_servers(&self, project_id: Option<Uuid>) -> Result<McpServerList> {
        let pairs: Vec<(&str, String)> = project_id.map(|p| ("project_id", p.to_string())).into_iter().collect();
        let url = Self::url_with(&self.base, "/mcp/servers", &pairs)?;
        Self::handle(self.client.get(url).send().await.map_err(Self::net)?).await
    }
    async fn check_mcp_server(&self, project_id: Option<Uuid>, id: &str) -> Result<McpCheckResult> {
        let url = Self::mcp_server_url(&self.base, id, project_id, Some("check"))?;
        Self::handle(self.client.post(url).send().await.map_err(Self::net)?).await
    }
    async fn set_mcp_server_enabled(&self, project_id: Option<Uuid>, id: &str, enabled: bool, actor: &str) -> Result<McpServerEntry> {
        let url = Self::mcp_server_url(&self.base, id, project_id, Some("enabled"))?;
        Self::handle(
            self.client.put(url).header("X-Atlas-Actor", actor).json(&serde_json::json!({"enabled": enabled})).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn add_mcp_server(&self, input: NewMcpServer, actor: &str) -> Result<McpServerEntry> {
        Self::handle(
            self.client.post(format!("{}/mcp/servers", self.base)).header("X-Atlas-Actor", actor).json(&input).send().await.map_err(Self::net)?,
        )
        .await
    }
    async fn remove_mcp_server(&self, project_id: Option<Uuid>, id: &str, actor: &str) -> Result<()> {
        let url = Self::mcp_server_url(&self.base, id, project_id, None)?;
        Self::handle_empty(self.client.delete(url).header("X-Atlas-Actor", actor).send().await.map_err(Self::net)?).await
    }

    // Plugin MCP tools (Phase 13b). Only the daemon holds the desktop app's socket, so
    // the stdio shim reaches a plugin tool the same way it reaches everything else: over
    // HTTP, which is what makes `atlas mcp` list and call exactly the tools the daemon's
    // own streamable HTTP transport does.
    async fn plugin_tools(&self) -> Result<Vec<PluginToolDecl>> {
        Self::handle(self.client.get(format!("{}/mcp/plugin-tools", self.base)).send().await.map_err(Self::net)?).await
    }

    async fn call_plugin_tool(&self, plugin_id: &str, name: &str, args: serde_json::Value, actor: &str) -> Result<serde_json::Value> {
        // The id and name are percent-encoded rather than interpolated: both are bounded
        // to a safe character set at registration, but this URL is built from an MCP
        // client's tool name, which nothing here validated.
        let mut url = reqwest::Url::parse(&format!("{}/mcp/plugin-tools", self.base)).map_err(|e| AtlasError::Other(e.to_string()))?;
        {
            let mut segs = url.path_segments_mut().map_err(|_| AtlasError::Other("the daemon base URL cannot be a base".into()))?;
            segs.push(plugin_id);
            segs.push(name);
            segs.push("call");
        }
        Self::handle(
            self.client.post(url).header("X-Atlas-Actor", actor).json(&serde_json::json!({"args": args})).send().await.map_err(Self::net)?,
        )
        .await
    }
}

/// The shape `GET /runs/{id}` answers with.
#[derive(serde::Deserialize)]
struct RunDetail { run: WorkflowRun, steps: Vec<WorkflowStep> }

/// The shape `GET /tasks/counts` answers with, one row per board stage.
#[derive(serde::Deserialize)]
struct StageCount { stage: String, count: i64 }

#[cfg(test)]
mod tests {
    use super::*;

    /// `#`, `&`, a space and non-ASCII text must all survive a round trip through
    /// `search_url`'s percent-encoding: decoding the built URL's own query pairs must
    /// hand back exactly what was typed. `#` is the case that broke before this fix
    /// (reqwest treats an unencoded `#` as the start of a URL fragment and never sends
    /// what follows it); `&` would otherwise split into a bogus extra parameter.
    #[test]
    fn search_url_round_trips_special_characters_in_q() {
        for raw in ["C#", "a&b", "space here", "héllo wörld", "100%_done"] {
            let q = SearchQuery { q: raw.into(), project_id: None, kinds: None, limit: 20 };
            let url = RemoteBackend::search_url("http://127.0.0.1:1/api/v1", &q).unwrap();
            let decoded: HashMap<String, String> = url.query_pairs().into_owned().collect();
            assert_eq!(decoded.get("q").map(String::as_str), Some(raw), "{url}");
        }
    }

    #[test]
    fn search_url_carries_project_id_and_kinds() {
        let pid = Uuid::new_v4();
        let q = SearchQuery { q: "x".into(), project_id: Some(pid), kinds: Some(vec![atlas_core::search::global::SearchKind::Task, atlas_core::search::global::SearchKind::Memory]), limit: 5 };
        let url = RemoteBackend::search_url("http://127.0.0.1:1/api/v1", &q).unwrap();
        let decoded: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(decoded.get("project_id").map(String::as_str), Some(pid.to_string().as_str()));
        assert_eq!(decoded.get("kinds").map(String::as_str), Some("task,memory"));
        assert_eq!(decoded.get("limit").map(String::as_str), Some("5"));
    }
}
