//! MCP tool surface for Atlas, generic over a Backend so the same tools serve
//! from the daemon (LocalBackend) and from the stdio shim (RemoteBackend).
use std::path::PathBuf;
use std::sync::Arc;
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
    pub tool_router: ToolRouter<Self>,
}

fn err(e: atlas_core::AtlasError) -> McpError {
    match e { atlas_core::AtlasError::NotFound(m) => McpError::invalid_params(m, None), atlas_core::AtlasError::Invalid(m) => McpError::invalid_params(m, None), other => McpError::internal_error(other.to_string(), None) }
}
fn json_result<T: serde::Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(v).map_err(|e| McpError::internal_error(e.to_string(), None))?)]))
}

#[tool_router]
impl<B: Backend> AtlasMcp<B> {
    pub fn new(backend: Arc<B>) -> Self { Self { backend, source_tool: source_tool_label(), project_root: None, tool_router: Self::tool_router() } }

    /// Override the label stamped on `source_tool`. Set this at construction time: the
    /// process may already be multi-threaded, so a transport cannot announce itself by
    /// writing to the environment.
    pub fn with_source_tool(mut self, label: impl Into<String>) -> Self { self.source_tool = label.into(); self }

    /// Seed the project root used when a tool names none and `ATLAS_PROJECT_ROOT` is
    /// unset. Passed in rather than exported to the environment, which is unsound to
    /// write once the process is multi-threaded.
    pub fn with_project_root(mut self, root: PathBuf) -> Self { self.project_root = Some(root); self }

    /// The root a call is about: the argument first, then `ATLAS_PROJECT_ROOT`, then the
    /// root this server was started in. `None` when none of the three names one.
    fn root_for(&self, project_root: Option<PathBuf>) -> Option<PathBuf> {
        project_root
            .or_else(|| std::env::var("ATLAS_PROJECT_ROOT").ok().map(PathBuf::from))
            .or_else(|| self.project_root.clone())
    }

    /// Connects the resolved root so the caller has a project id to scope by. `None`
    /// when no root resolves, which leaves the caller global.
    async fn resolve_project(&self, project_root: Option<PathBuf>) -> Result<Option<Project>, McpError> {
        match self.root_for(project_root) {
            Some(r) => Ok(Some(self.backend.connect_project(r, "mcp").await.map_err(err)?)),
            None => Ok(None),
        }
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
        json_result(&self.backend.project_context(root, "mcp").await.map_err(err)?)
    }

    #[tool(description = "Register a repository with Atlas and build its profile (languages, frameworks, tree, recent commits). Call this once when starting work in a repository Atlas has not seen.")]
    async fn connect_project(&self, Parameters(a): Parameters<ConnectProjectArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.connect_project(a.root_path, "mcp").await.map_err(err)?)
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
        let id = self.resolve_project(a.project_root).await?.map(|p| p.id);
        json_result(&self.backend.list_docs(DocKind::Practice, id).await.map_err(err)?)
    }

    #[tool(description = "Fetch the full text of one practice by name. Call after list_practices when a practice looks relevant to the task.")]
    async fn get_practice(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_doc(DocKind::Practice, &a.name).await.map_err(err)?)
    }

    #[tool(description = "List the workflows that apply here: the global ones plus any scoped to this project. Call when the user asks for a multi-step process such as a release or a review.")]
    async fn list_workflows(&self, Parameters(a): Parameters<ProjectRootArgs>) -> Result<CallToolResult, McpError> {
        let id = self.resolve_project(a.project_root).await?.map(|p| p.id);
        json_result(&self.backend.list_docs(DocKind::Workflow, id).await.map_err(err)?)
    }

    #[tool(description = "Fetch the full text of one workflow by name. Call after list_workflows to follow its steps.")]
    async fn get_workflow(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_doc(DocKind::Workflow, &a.name).await.map_err(err)?)
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
            (claude_agent_md(&self.backend.get_agent(name).await.map_err(err)?), MARKDOWN)
        } else if let Some(name) = uri.strip_prefix(PRACTICES) {
            (self.backend.get_doc(DocKind::Practice, name).await.map_err(err)?.body, MARKDOWN)
        } else if let Some(name) = uri.strip_prefix(WORKFLOWS) {
            (self.backend.get_doc(DocKind::Workflow, name).await.map_err(err)?.body, MARKDOWN)
        } else if let Some(id) = uri.strip_prefix(PROJECTS).and_then(|r| r.strip_suffix("/context")) {
            let id = id.parse::<Uuid>().map_err(|e| McpError::resource_not_found(format!("{uri} is not a project context resource: {e}"), None))?;
            let project = self.backend.get_project(id).await.map_err(err)?;
            let ctx = self.backend.project_context(PathBuf::from(project.root_path), "mcp").await.map_err(err)?;
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
    #[test]
    fn tool_list_covers_memories_projects_agents_and_docs() {
        let dir = tempfile::tempdir().unwrap();
        let b = std::sync::Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(b);
        let names: Vec<String> = s.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
        for n in ["remember", "recall", "forget", "status", "project_context", "connect_project", "list_agents",
                  "get_agent", "save_agent", "list_practices", "get_practice", "list_workflows", "get_workflow"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
