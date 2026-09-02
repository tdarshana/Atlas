//! MCP tool surface for Atlas, generic over a Backend so the same tools serve
//! from the daemon (LocalBackend) and from the stdio shim (RemoteBackend).
use std::sync::Arc;
use atlas_core::backend::Backend;
use atlas_core::models::*;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
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
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs { pub id: Uuid, pub reason: Option<String> }

#[derive(Clone)]
pub struct AtlasMcp<B: Backend> { backend: Arc<B>, source_tool: String, pub tool_router: ToolRouter<Self> }

fn err(e: atlas_core::AtlasError) -> McpError {
    match e { atlas_core::AtlasError::NotFound(m) => McpError::invalid_params(m, None), atlas_core::AtlasError::Invalid(m) => McpError::invalid_params(m, None), other => McpError::internal_error(other.to_string(), None) }
}
fn json_result<T: serde::Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(v).map_err(|e| McpError::internal_error(e.to_string(), None))?)]))
}

#[tool_router]
impl<B: Backend> AtlasMcp<B> {
    pub fn new(backend: Arc<B>) -> Self { Self { backend, source_tool: source_tool_label(), tool_router: Self::tool_router() } }

    /// Override the label stamped on `source_tool`. Set this at construction time: the
    /// process may already be multi-threaded, so a transport cannot announce itself by
    /// writing to the environment.
    pub fn with_source_tool(mut self, label: impl Into<String>) -> Self { self.source_tool = label.into(); self }

    #[tool(description = "Store a memory shared with every agent. Use for facts about the project, decisions and their reasons, user preferences, and insights worth keeping.")]
    async fn remember(&self, Parameters(a): Parameters<RememberArgs>) -> Result<CallToolResult, McpError> {
        let kind = a.kind.as_deref().unwrap_or("fact").parse::<MemoryKind>().map_err(err)?;
        let scope = match a.scope.as_deref() { Some(s) => s.parse::<MemoryScope>().map_err(err)?, None => if a.project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global } };
        let m = NewMemory { scope, project_id: a.project_id, kind, text: a.text, tags: a.tags.unwrap_or_default(), source_agent: a.source_agent, source_tool: Some(self.source_tool.clone()), confidence: 1.0, status: MemoryStatus::Active };
        json_result(&self.backend.remember(m, "mcp").await.map_err(err)?)
    }

    #[tool(description = "Search shared memory with a natural-language query. Returns ranked memories with scores. Call this before starting work on a task to pick up prior decisions and preferences.")]
    async fn recall(&self, Parameters(a): Parameters<RecallArgs>) -> Result<CallToolResult, McpError> {
        let scope = match a.scope.as_deref() { Some(s) => Some(s.parse::<MemoryScope>().map_err(err)?), None => None };
        let mut kinds = vec![]; for k in a.kinds.unwrap_or_default() { kinds.push(k.parse::<MemoryKind>().map_err(err)?); }
        let q = RecallQuery { query: a.query, limit: a.limit.unwrap_or(10), scope, project_id: a.project_id, kinds, tags: a.tags.unwrap_or_default() };
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
}

#[tool_handler]
impl<B: Backend> ServerHandler for AtlasMcp<B> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("atlas", env!("CARGO_PKG_VERSION")))
            .with_instructions("Atlas is the shared memory for all coding agents on this machine. Call recall at the start of a task and remember when you learn a durable fact, make a decision, or notice a user preference.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::backend::LocalBackend;
    use atlas_core::paths::AtlasPaths;
    #[test]
    fn tool_list_has_four_tools() {
        let dir = tempfile::tempdir().unwrap();
        let b = std::sync::Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(b);
        let names: Vec<String> = s.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
        for n in ["remember", "recall", "forget", "status"] { assert!(names.contains(&n.to_string()), "missing {n}"); }
    }
}
