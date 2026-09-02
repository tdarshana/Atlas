//! MCP tool surface for Atlas, generic over a Backend so the same tools serve
//! from the daemon (LocalBackend) and from the stdio shim (RemoteBackend).
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use atlas_core::backend::Backend;
use atlas_core::export::claude_agent_md;
use atlas_core::models::*;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use uuid::Uuid;

pub fn source_tool_label() -> String { std::env::var("ATLAS_SOURCE_TOOL").unwrap_or_else(|_| "mcp".into()) }

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RememberArgs {
    /// The fact, decision, preference, insight or todo to store, in one or two sentences.
    pub text: String,
    /// One of fact, decision, preference, insight, todo. Default fact.
    pub kind: Option<String>,
    pub tags: Option<Vec<String>>,
    /// global or project. Default project when project_id is given, else global.
    pub scope: Option<String>,
    pub project_id: Option<Uuid>,
    /// Absolute path to the project this memory belongs to. Ignored when project_id is given.
    pub project_root: Option<PathBuf>,
    /// Name of the agent storing this memory.
    pub source_agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RecallArgs {
    pub query: String,
    pub limit: Option<usize>,
    pub scope: Option<String>,
    pub kinds: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub project_id: Option<Uuid>,
    /// Absolute path to the project to search. Ignored when project_id is given.
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs { pub id: Uuid, pub reason: Option<String> }

/// Arguments for the tools that only need to know which project is in play.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectRootArgs {
    /// Absolute path to the project root. Defaults to the root this server was started in.
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IngestTranscriptArgs {
    /// The conversation transcript to extract durable memories from.
    pub text: String,
    /// The tool the transcript came from, recorded on every memory extracted from
    /// it. Defaults to this server's source_tool label.
    pub source_tool: Option<String>,
    /// Absolute path to the project this transcript belongs to. Defaults to the
    /// root this server was started in.
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ConnectProjectArgs {
    /// Absolute path to the project root. Any directory inside the repository works.
    pub root_path: PathBuf,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NameArgs { pub name: String }

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SaveAgentArgs {
    /// Short kebab-case identifier, unique across agents.
    pub name: String,
    /// One line saying when this agent should be used.
    pub description: String,
    /// The agent's system prompt, in Markdown.
    pub instructions: String,
    /// Preferred model, e.g. sonnet or opus.
    pub model_hint: Option<String>,
    /// Tools the agent is allowed to use.
    pub tools: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct AtlasMcp<B: Backend> {
    backend: Arc<B>,
    source_tool: String,
    /// Project root to fall back on when neither the tool argument nor
    /// `ATLAS_PROJECT_ROOT` names one. Seeded by the stdio shim from its cwd.
    project_root: Option<PathBuf>,
    /// Whether `ATLAS_PROJECT_ROOT` may name the project. True for the stdio shim,
    /// which runs per project; false for the daemon, whose environment says nothing
    /// about the repository any given client is working in.
    env_project_root: bool,
    /// Roots already connected by this server, so a read never writes. Shared across
    /// clones because rmcp builds one handler per session from a shared factory.
    projects: Arc<Mutex<HashMap<PathBuf, Project>>>,
    pub tool_router: ToolRouter<Self>,
}

fn err(e: atlas_core::AtlasError) -> McpError {
    match e {
        atlas_core::AtlasError::NotFound(m) => McpError::invalid_params(m, None),
        atlas_core::AtlasError::Invalid(m) => McpError::invalid_params(m, None),
        // An argument past a size cap is the caller's to fix, and rmcp has no error
        // kind for "too large", so it reads as invalid params like any other argument
        // the server will not take.
        atlas_core::AtlasError::TooLarge(m) => McpError::invalid_params(m, None),
        other => McpError::internal_error(other.to_string(), None),
    }
}

/// Like [`err`], but for lookups addressed by a resource URI: a name or id that is not
/// in the store means the resource does not exist, not that the request was malformed.
fn resource_err(uri: &str, e: atlas_core::AtlasError) -> McpError {
    match e {
        atlas_core::AtlasError::NotFound(m) => McpError::resource_not_found(format!("{uri}: {m}"), None),
        other => err(other),
    }
}
fn json_result<T: serde::Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(v).map_err(|e| McpError::internal_error(e.to_string(), None))?)]))
}

#[tool_router]
impl<B: Backend> AtlasMcp<B> {
    pub fn new(backend: Arc<B>) -> Self {
        Self { backend, source_tool: source_tool_label(), project_root: None, env_project_root: true, projects: Arc::default(), tool_router: Self::tool_router() }
    }

    /// Override the label stamped on `source_tool`. Set this at construction time: the
    /// process may already be multi-threaded, so a transport cannot announce itself by
    /// writing to the environment.
    pub fn with_source_tool(mut self, label: impl Into<String>) -> Self { self.source_tool = label.into(); self }

    /// Seed the project root used when a tool names none and `ATLAS_PROJECT_ROOT` is
    /// unset. Passed in rather than exported to the environment, which is unsound to
    /// write once the process is multi-threaded.
    pub fn with_project_root(mut self, root: PathBuf) -> Self { self.project_root = Some(root); self }

    /// Whether `ATLAS_PROJECT_ROOT` may name the project. Defaults to true, which is
    /// right for the stdio shim: it is launched per repository and inherits the client's
    /// environment. The daemon serves every project at once and must turn this off, or
    /// whatever repository its own environment happens to name would scope every client.
    pub fn with_env_project_root(mut self, enabled: bool) -> Self { self.env_project_root = enabled; self }

    /// The root a call is about: the argument first, then `ATLAS_PROJECT_ROOT`, then the
    /// root this server was started in. `None` when none of the three names one.
    ///
    /// Canonicalized, so two spellings of one directory (a relative path, a symlink, a
    /// trailing slash) share a cache entry instead of connecting the project twice. A
    /// path that does not resolve is passed on as given, for the backend to report.
    fn root_for(&self, project_root: Option<PathBuf>) -> Option<PathBuf> {
        let root = project_root
            .or_else(|| self.env_project_root.then(|| std::env::var("ATLAS_PROJECT_ROOT").ok().map(PathBuf::from)).flatten())
            .or_else(|| self.project_root.clone())?;
        Some(root.canonicalize().unwrap_or(root))
    }

    /// The project for the resolved root, connecting it the first time and remembering
    /// it after. `None` when no root resolves, which leaves the caller global.
    ///
    /// Reads go through here so they stay reads: `connect_project` upserts the row and
    /// writes an audit entry, which a recall or a doc listing has no business doing on
    /// every call. `connect_project` and `project_context` are explicit connects and
    /// refresh the entry instead.
    async fn resolve_project(&self, project_root: Option<PathBuf>) -> Result<Option<Project>, McpError> {
        let Some(root) = self.root_for(project_root) else { return Ok(None) };
        if let Some(p) = self.cached(&root) { return Ok(Some(p)); }
        let project = self.backend.connect_project(root.clone(), "mcp").await.map_err(err)?;
        self.cache(root, &project);
        Ok(Some(project))
    }

    /// A lock poisoned by a panic in another session must not take this one down: the
    /// cache is a shortcut, so fall back to the backend rather than propagate.
    fn cached(&self, root: &PathBuf) -> Option<Project> {
        self.projects.lock().ok()?.get(root).cloned()
    }
    fn cache(&self, root: PathBuf, project: &Project) {
        if let Ok(mut m) = self.projects.lock() { m.insert(root, project.clone()); }
    }

    /// The docs that apply where this call is coming from: the project's plus the
    /// global ones, or — when no project resolves — only the global ones. The store's
    /// unfiltered listing spans every project, which is right for a resource listing
    /// but not for a tool that says it lists what applies here.
    async fn docs_here(&self, kind: DocKind, project_root: Option<PathBuf>) -> Result<Vec<Doc>, McpError> {
        let id = self.resolve_project(project_root).await?.map(|p| p.id);
        let docs = self.backend.list_docs(kind, id).await.map_err(err)?;
        Ok(match id { Some(_) => docs, None => docs.into_iter().filter(|d| d.project_id.is_none()).collect() })
    }

    /// The project id to scope by, or `None` for global. `project_id` wins over any
    /// root, so an explicit id never triggers a project lookup.
    async fn scope_id(&self, project_id: Option<Uuid>, project_root: Option<PathBuf>) -> Result<Option<Uuid>, McpError> {
        match project_id {
            Some(id) => Ok(Some(id)),
            None => Ok(self.resolve_project(project_root).await?.map(|p| p.id)),
        }
    }

    #[tool(description = "Store a memory shared with every agent. Use for facts about the project, decisions and their reasons, user preferences, and insights worth keeping.")]
    async fn remember(&self, Parameters(a): Parameters<RememberArgs>) -> Result<CallToolResult, McpError> {
        let kind = a.kind.as_deref().unwrap_or("fact").parse::<MemoryKind>().map_err(err)?;
        let asked = match a.scope.as_deref() { Some(s) => Some(s.parse::<MemoryScope>().map_err(err)?), None => None };
        // A memory the caller called global stays unattached even in a project session.
        let project_id = match asked { Some(MemoryScope::Global) => None, _ => self.scope_id(a.project_id, a.project_root).await? };
        let scope = asked.unwrap_or(if project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global });
        let m = NewMemory { scope, project_id, kind, text: a.text, tags: a.tags.unwrap_or_default(), source_agent: a.source_agent, source_tool: Some(self.source_tool.clone()), confidence: 1.0, status: MemoryStatus::Active };
        json_result(&self.backend.remember(m, "mcp").await.map_err(err)?)
    }

    #[tool(description = "Search shared memory with a natural-language query. Returns ranked memories with scores. Call this before starting work on a task to pick up prior decisions and preferences.")]
    async fn recall(&self, Parameters(a): Parameters<RecallArgs>) -> Result<CallToolResult, McpError> {
        let scope = match a.scope.as_deref() { Some(s) => Some(s.parse::<MemoryScope>().map_err(err)?), None => None };
        let mut kinds = vec![]; for k in a.kinds.unwrap_or_default() { kinds.push(k.parse::<MemoryKind>().map_err(err)?); }
        // A project id widens rather than narrows: the store returns that project's
        // memories alongside the global ones.
        let project_id = match scope { Some(MemoryScope::Global) => None, _ => self.scope_id(a.project_id, a.project_root).await? };
        let q = RecallQuery { query: a.query, limit: a.limit.unwrap_or(10), scope, project_id, kinds, tags: a.tags.unwrap_or_default() };
        json_result(&self.backend.recall(q).await.map_err(err)?)
    }

    #[tool(description = "Mark a memory as superseded so it stops appearing in recall. Nothing is deleted.")]
    async fn forget(&self, Parameters(a): Parameters<ForgetArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.forget(a.id, a.reason, "mcp").await.map_err(err)?)
    }

    #[tool(description = "Report Atlas daemon status: version, database path, active memory count, embedding availability.")]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.status().await.map_err(err)?)
    }

    #[tool(description = "Call at the start of a session to load the current project's profile, practices, workflows and top memories. Pass project_root to name a repository other than the one this server was started in.")]
    async fn project_context(&self, Parameters(a): Parameters<ProjectRootArgs>) -> Result<CallToolResult, McpError> {
        let root = self.root_for(a.project_root)
            .ok_or_else(|| McpError::invalid_params("no project root: pass project_root or set ATLAS_PROJECT_ROOT", None))?;
        // An explicit connect: it goes to the backend even on a cache hit, so a profile
        // that has gone stale is rebuilt rather than served from memory.
        let ctx = self.backend.project_context(root.clone(), "mcp").await.map_err(err)?;
        self.cache(root, &ctx.project);
        json_result(&ctx)
    }

    #[tool(description = "Register a repository with Atlas and build its profile (languages, frameworks, tree, recent commits). Call this once when starting work in a repository Atlas has not seen.")]
    async fn connect_project(&self, Parameters(a): Parameters<ConnectProjectArgs>) -> Result<CallToolResult, McpError> {
        let project = self.backend.connect_project(a.root_path.clone(), "mcp").await.map_err(err)?;
        self.cache(a.root_path, &project);
        json_result(&project)
    }

    #[tool(description = "List the agent roles stored in Atlas, with their descriptions. Call this to see which specialist role fits the task before doing the work yourself.")]
    async fn list_agents(&self) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.list_agents().await.map_err(err)?)
    }

    #[tool(description = "Fetch one agent role by name, including its full instructions. Call after list_agents to adopt the role.")]
    async fn get_agent(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_agent(&a.name).await.map_err(err)?)
    }

    #[tool(description = "Create or update an agent role so every coding agent on this machine can use it. Call when the user describes a repeatable specialist role worth keeping.")]
    async fn save_agent(&self, Parameters(a): Parameters<SaveAgentArgs>) -> Result<CallToolResult, McpError> {
        let agent = NewAgent { name: a.name, description: a.description, instructions: a.instructions, model_hint: a.model_hint, tools: a.tools.unwrap_or_default(), tags: a.tags.unwrap_or_default() };
        json_result(&self.backend.save_agent(agent, "mcp").await.map_err(err)?)
    }

    #[tool(description = "List the coding practices that apply here: the global ones plus any scoped to this project. Call before writing code so the work follows the house style.")]
    async fn list_practices(&self, Parameters(a): Parameters<ProjectRootArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.docs_here(DocKind::Practice, a.project_root).await?)
    }

    #[tool(description = "Fetch the full text of one practice by name. Call after list_practices when a practice looks relevant to the task.")]
    async fn get_practice(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_doc(DocKind::Practice, &a.name).await.map_err(err)?)
    }

    #[tool(description = "List the workflows that apply here: the global ones plus any scoped to this project. Call when the user asks for a multi-step process such as a release or a review.")]
    async fn list_workflows(&self, Parameters(a): Parameters<ProjectRootArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.docs_here(DocKind::Workflow, a.project_root).await?)
    }

    #[tool(description = "Fetch the full text of one workflow by name. Call after list_workflows to follow its steps.")]
    async fn get_workflow(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_doc(DocKind::Workflow, &a.name).await.map_err(err)?)
    }

    #[tool(description = "Queue a conversation transcript for opt-in LLM extraction of durable memories. Returns a job id to poll; fails if extraction is not enabled and configured on the daemon.")]
    async fn ingest_transcript(&self, Parameters(a): Parameters<IngestTranscriptArgs>) -> Result<CallToolResult, McpError> {
        let source_tool = a.source_tool.unwrap_or_else(|| self.source_tool.clone());
        let root = self.root_for(a.project_root);
        let job_id = self.backend.ingest_transcript(a.text, source_tool, root).await.map_err(|e| match e {
            // Extraction being off is a request the caller made in good faith against
            // a daemon not set up for it, not a malformed call - but rmcp has no
            // "conflict" error kind, so it is reported as invalid_params with the same
            // text `POST /ingest` answers 409 with.
            atlas_core::AtlasError::Conflict(msg) => McpError::invalid_params(msg, None),
            other => err(other),
        })?;
        json_result(&serde_json::json!({"job_id": job_id}))
    }
}

const AGENTS: &str = "atlas://agents/";
const PRACTICES: &str = "atlas://practices/";
const WORKFLOWS: &str = "atlas://workflows/";
const PROJECTS: &str = "atlas://projects/";

const MARKDOWN: &str = "text/markdown";
const JSON: &str = "application/json";

/// Resources and prompts are written by hand rather than by the static macros: both
/// lists come from the database, so they change while the server is running.
#[tool_handler]
impl<B: Backend> ServerHandler for AtlasMcp<B> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().enable_resources().enable_prompts().build())
            .with_server_info(Implementation::new("atlas", env!("CARGO_PKG_VERSION")))
            .with_instructions("Atlas is the shared memory for all coding agents on this machine. Call recall at the start of a task and remember when you learn a durable fact, make a decision, or notice a user preference.")
    }

    async fn list_resources(&self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>) -> Result<ListResourcesResult, McpError> {
        let mut out = vec![];
        for a in self.backend.list_agents().await.map_err(err)? {
            out.push(Resource::new(format!("{AGENTS}{}", a.name), a.name.clone()).with_description(a.description).with_mime_type(MARKDOWN));
        }
        for (prefix, kind) in [(PRACTICES, DocKind::Practice), (WORKFLOWS, DocKind::Workflow)] {
            for d in self.backend.list_docs(kind, None).await.map_err(err)? {
                out.push(Resource::new(format!("{prefix}{}", d.name), d.name.clone()).with_mime_type(MARKDOWN));
            }
        }
        for p in self.backend.list_projects().await.map_err(err)? {
            out.push(Resource::new(format!("{PROJECTS}{}/context", p.id), p.name.clone())
                .with_description(format!("Profile, practices, workflows and top memories for {}", p.root_path))
                .with_mime_type(JSON));
        }
        Ok(ListResourcesResult::with_all_items(out))
    }

    async fn read_resource(&self, request: ReadResourceRequestParams, _context: RequestContext<RoleServer>) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri;
        let (text, mime) = if let Some(name) = uri.strip_prefix(AGENTS) {
            (claude_agent_md(&self.backend.get_agent(name).await.map_err(|e| resource_err(&uri, e))?), MARKDOWN)
        } else if let Some(name) = uri.strip_prefix(PRACTICES) {
            (self.backend.get_doc(DocKind::Practice, name).await.map_err(|e| resource_err(&uri, e))?.body, MARKDOWN)
        } else if let Some(name) = uri.strip_prefix(WORKFLOWS) {
            (self.backend.get_doc(DocKind::Workflow, name).await.map_err(|e| resource_err(&uri, e))?.body, MARKDOWN)
        } else if let Some(id) = uri.strip_prefix(PROJECTS).and_then(|r| r.strip_suffix("/context")) {
            let id = id.parse::<Uuid>().map_err(|e| McpError::resource_not_found(format!("{uri} is not a project context resource: {e}"), None))?;
            let project = self.backend.get_project(id).await.map_err(|e| resource_err(&uri, e))?;
            let ctx = self.backend.project_context(PathBuf::from(project.root_path), "mcp").await.map_err(|e| resource_err(&uri, e))?;
            (serde_json::to_string_pretty(&ctx).map_err(|e| McpError::internal_error(e.to_string(), None))?, JSON)
        } else {
            return Err(McpError::resource_not_found(format!("no Atlas resource at {uri}"), None));
        };
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri).with_mime_type(mime)]).into())
    }

    async fn list_prompts(&self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>) -> Result<ListPromptsResult, McpError> {
        let prompts = self.backend.list_agents().await.map_err(err)?.into_iter()
            .map(|a| Prompt::new(a.name, Some(a.description), None))
            .collect();
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(&self, request: GetPromptRequestParams, _context: RequestContext<RoleServer>) -> Result<GetPromptResponse, McpError> {
        let agent = self.backend.get_agent(&request.name).await.map_err(err)?;
        let text = format!("Adopt the following agent role:\n\n{}", agent.instructions);
        let mut result = GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]);
        result.description = Some(agent.description);
        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::backend::LocalBackend;
    use atlas_core::paths::AtlasPaths;

    /// The text of a tool result, so a test can read what the tool told the client.
    fn text_of(r: &CallToolResult) -> String {
        r.content.iter().filter_map(|c| c.as_text()).map(|t| t.text.clone()).collect()
    }

    /// How many times a project row has been upserted or re-profiled. Reads must not
    /// move this number.
    fn project_writes(b: &LocalBackend) -> i64 {
        b.db.with_conn(|c| Ok(c.query_row("select count(*) from audit where entity = 'project'", [], |r| r.get::<_, i64>(0))?)).unwrap()
    }

    #[test]
    fn tool_list_covers_memories_projects_agents_and_docs() {
        let dir = tempfile::tempdir().unwrap();
        let b = std::sync::Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(b);
        let names: Vec<String> = s.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
        for n in ["remember", "recall", "forget", "status", "project_context", "connect_project", "list_agents",
                  "get_agent", "save_agent", "list_practices", "get_practice", "list_workflows", "get_workflow",
                  "ingest_transcript"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }

    /// A daemon with extraction off (the default) answers `ingest_transcript` with
    /// `invalid_params` carrying the same "extraction is disabled" text `POST
    /// /ingest` answers 409 with, not an internal error.
    #[tokio::test]
    async fn ingest_transcript_reports_disabled_extraction_as_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend);

        let err = s
            .ingest_transcript(Parameters(IngestTranscriptArgs { text: "user: we use bun".into(), source_tool: None, project_root: None }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("extraction is disabled"), "{err:?}");
    }

    /// The blank check and the size cap live in the backend, not in the daemon's HTTP
    /// handler, so this tool inherits them: MCP is as unauthenticated as `POST /ingest`
    /// and reaches the same queue. Neither refusal spends a model call, and neither is
    /// an internal error: they are arguments the server will not take.
    #[tokio::test]
    async fn ingest_transcript_refuses_a_blank_or_oversized_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        // Configured, so the enable gate is open and the transcript itself is what is
        // being judged. The endpoint is never called: both refusals happen before the
        // job is queued.
        backend
            .set_settings(
                serde_json::Map::from_iter([
                    ("extraction.enabled".to_string(), serde_json::Value::Bool(true)),
                    ("extraction.base_url".to_string(), serde_json::Value::String("http://127.0.0.1:1/v1".into())),
                    ("extraction.model".to_string(), serde_json::Value::String("stub".into())),
                ]),
                "t",
            )
            .await
            .unwrap();
        let s = AtlasMcp::new(backend);
        let ingest = |text: String| {
            let s = &s;
            async move { s.ingest_transcript(Parameters(IngestTranscriptArgs { text, source_tool: None, project_root: None })).await }
        };

        for blank in ["", "   \n\t "] {
            let err = ingest(blank.to_string()).await.unwrap_err();
            assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{err:?}");
            assert!(err.message.contains("empty"), "{err:?}");
        }

        let err = ingest("x".repeat(1_000_001)).await.unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{err:?}");
        assert!(err.message.contains("too large"), "{err:?}");

        // A transcript at the cap is fine, so the boundary is not off by one.
        ingest("x".repeat(1_000_000)).await.unwrap();
    }

    /// A server started in a project scopes memories to it without being told, and does
    /// so from a cache: the second recall must not touch the projects table.
    #[tokio::test]
    async fn seeded_root_scopes_memories_and_is_resolved_once() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend.clone())
            .with_env_project_root(false)
            .with_project_root(repo.path().to_path_buf());

        // No scope, no project_id: the seeded root supplies both.
        s.remember(Parameters(RememberArgs {
            text: "the fixture uses duckdb".into(), kind: None, tags: None, scope: None,
            project_id: None, project_root: None, source_agent: None,
        })).await.unwrap();

        let stored = backend.list_memories(MemoryStatus::Active, None).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].scope, MemoryScope::Project);
        let project_id = stored[0].project_id.expect("the seeded root should have scoped the memory");

        let projects = backend.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project_id);

        // Recall with no project_id finds it through the same resolution.
        let hit = s.recall(Parameters(RecallArgs {
            query: "duckdb".into(), limit: None, scope: None, kinds: None, tags: None,
            project_id: None, project_root: None,
        })).await.unwrap();
        assert!(text_of(&hit).contains("the fixture uses duckdb"), "{}", text_of(&hit));

        // Everything from here on is a read, so the audit trail must stand still.
        let before = project_writes(&backend);
        s.recall(Parameters(RecallArgs {
            query: "duckdb".into(), limit: None, scope: None, kinds: None, tags: None,
            project_id: None, project_root: None,
        })).await.unwrap();
        s.list_practices(Parameters(ProjectRootArgs { project_root: None })).await.unwrap();
        assert_eq!(project_writes(&backend), before, "a read reconnected the project instead of using the cache");
    }

    /// Without a seeded root and with the environment ignored, the server stays global:
    /// memories are unscoped and only project-less docs are listed.
    #[tokio::test]
    async fn unscoped_server_stays_global() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        s.remember(Parameters(RememberArgs {
            text: "bun is the runtime".into(), kind: None, tags: None, scope: None,
            project_id: None, project_root: None, source_agent: None,
        })).await.unwrap();
        let stored = backend.list_memories(MemoryStatus::Active, None).await.unwrap();
        assert_eq!(stored[0].scope, MemoryScope::Global);
        assert!(stored[0].project_id.is_none());
        assert!(backend.list_projects().await.unwrap().is_empty(), "a global remember connected a project");

        // A practice belonging to some other project must not show up in a global listing.
        let project = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "scoped".into(), body: "b".into(), tags: vec![], project_id: Some(project.id) }, "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "everywhere".into(), body: "b".into(), tags: vec![], project_id: None }, "test").await.unwrap();
        let listed = s.list_practices(Parameters(ProjectRootArgs { project_root: None })).await.unwrap();
        let listed = text_of(&listed);
        assert!(listed.contains("everywhere"), "{listed}");
        assert!(!listed.contains("scoped"), "a global listing leaked another project's practice: {listed}");
    }
}
