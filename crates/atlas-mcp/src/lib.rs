//! MCP tool surface for Atlas, generic over a Backend so the same tools serve
//! from the daemon (LocalBackend) and from the stdio shim (RemoteBackend).
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use atlas_core::backend::Backend;
use atlas_core::board::render::render_board_markdown;
use atlas_core::export::claude_agent_md;
use atlas_core::models::*;
use chrono::{DateTime, Utc};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use uuid::Uuid;

pub fn source_tool_label() -> String { std::env::var("ATLAS_SOURCE_TOOL").unwrap_or_else(|_| "mcp".into()) }

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryRememberArgs {
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
pub struct MemorySearchArgs {
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
pub struct MemoryForgetArgs { pub id: Uuid, pub reason: Option<String> }

/// Filters for `memory_list`. Unlike `memory_search`, there is no query: this is a
/// plain listing, narrowed by whichever of these the caller gives.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryListArgs {
    /// Keep only memories of these kinds (fact, decision, preference, insight, todo).
    pub kinds: Option<Vec<String>>,
    /// Keep only memories carrying at least one of these tags.
    pub tags: Option<Vec<String>>,
    pub project_id: Option<Uuid>,
    /// Absolute path to the project to list. Ignored when project_id is given. With
    /// neither, every memory is in scope (global and every project's own).
    pub project_root: Option<PathBuf>,
    /// Keep only memories created at or after this time.
    pub since: Option<DateTime<Utc>>,
    /// Default 50.
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryReviewArgs {
    /// The pending memory's id.
    pub id: Uuid,
    /// "accept" (becomes active) or "reject" (stays out of recall, like forget).
    pub decision: String,
}

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
    /// The agent inside this tool, appended to this server's own label to form the
    /// actor recorded on every memory extracted from the transcript.
    pub agent: Option<String>,
    /// Absolute path to the project this transcript belongs to. Defaults to the
    /// root this server was started in.
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowRunArgs {
    /// The workflow's name (or id).
    pub name: String,
    /// Text for the first action's "Input:" line, when the workflow expects one.
    pub input: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowStatusArgs {
    /// A run id returned by workflow_run.
    pub run_id: Uuid,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectConnectArgs {
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

/// `agent_list`'s arguments. Agents are global today, not scoped to a project, so
/// `project_root` has no effect yet; it is accepted now for the same shape as
/// `practice_list` and to leave room for project-scoped agents later.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AgentListArgs {
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PracticeListArgs {
    pub project_root: Option<PathBuf>,
    /// Keep only practices carrying at least one of these tags.
    pub tags: Option<Vec<String>>,
}

/// Board (Phase 6) tool arguments. Every write tool takes an optional `agent`,
/// appended to the actor as `<source_tool>/<agent>` so a board history reads as a
/// plain sentence naming the specific agent, not just the tool that hosted it.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskListArgs {
    /// Absolute path to the project whose board to list, or the literal string
    /// "global" for tasks with no project. Defaults to the root this server was
    /// started in.
    pub project_root: Option<PathBuf>,
    pub stage: Option<String>,
    pub assignee: Option<String>,
    /// Keep only tasks that are ready: not done, every blocker done, no open subtask.
    pub ready: Option<bool>,
    /// Case-insensitive substring match over key, title and description.
    pub query: Option<String>,
    /// Include tasks in a done stage. Default false.
    pub include_done: Option<bool>,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskKeyArgs {
    /// A task's key (e.g. ATL-12) or id.
    pub key: String,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskCreateArgs {
    pub title: String,
    pub description: Option<String>,
    /// Default task.
    pub kind: Option<TaskKind>,
    /// Default medium.
    pub priority: Option<TaskPriority>,
    pub labels: Option<Vec<String>>,
    /// Parent task, by id or key.
    pub parent: Option<String>,
    /// Tasks this one waits on, by id or key.
    pub blocked_by: Option<Vec<String>>,
    /// Absolute path to the project this task belongs to, or the literal string
    /// "global" for a task with no project. Defaults to the root this server was
    /// started in.
    pub project_root: Option<PathBuf>,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskUpdateArgs {
    pub key: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub kind: Option<TaskKind>,
    pub priority: Option<TaskPriority>,
    /// New assignee. An empty string clears the assignee; leave this field out
    /// entirely to leave the assignee unchanged.
    pub assignee: Option<String>,
    pub labels: Option<Vec<String>>,
    /// The `updated_at` you last read for this task. A mismatch fails the call
    /// with a conflict instead of overwriting a concurrent change.
    pub expected_updated_at: Option<DateTime<Utc>>,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskMoveArgs {
    pub key: String,
    /// Must be one of the board's stage names; call board_stages to see them.
    pub stage: String,
    pub expected_updated_at: Option<DateTime<Utc>>,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskCommentArgs {
    pub key: String,
    pub body: String,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskClaimArgs {
    pub key: String,
    /// Take the task even if it is already assigned to someone else. Default false.
    pub force: Option<bool>,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskBlockArgs {
    pub key: String,
    /// The tasks this one waits on, by id or key. This replaces the whole list, so
    /// pass every blocker that still applies; an empty list clears them all.
    pub blocked_by: Vec<String>,
    pub agent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FrameworkDocsArgs {
    /// Absolute path to the project. Defaults to the root this server was started in.
    pub project_root: Option<PathBuf>,
    /// Restrict to one framework: superpowers, openspec, speckit or gsd. Required
    /// when path is given.
    pub kind: Option<FrameworkKind>,
    /// A document path from a prior call's listing (a `FrameworkDoc.path`). Requires
    /// kind. Omit both kind and path to list every framework detected with its
    /// documents.
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SkillListArgs {
    /// Absolute path to the project whose skills to list. Defaults to the root this
    /// server was started in; without any project, only the global skills are listed.
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SkillGetArgs {
    /// A skill id from a prior `skill_list` call.
    pub id: String,
    /// Absolute path to the project the id was listed under. Defaults to the root this
    /// server was started in.
    pub project_root: Option<PathBuf>,
}

/// Whether a tool in [`TOOL_TABLE`] only reads state or can change it. Shown in the
/// `/api/v1/mcp/status` tools table (Task 2) as a badge: read is informational, write
/// is a warning, since a write tool run by an agent this project has not admitted is
/// refused by the backend's own `agent_access` gate, not by anything in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolScope { Read, Write }

/// The prefix every plugin-contributed MCP tool name carries. No name in [`TOOL_TABLE`]
/// starts with it (asserted by `plugin_tool_names_never_collide_with_a_builtin`), so a
/// plugin can never shadow a built-in tool.
pub const PLUGIN_TOOL_PREFIX: &str = "plugin__";

/// The MCP name a plugin's tool is listed and called under:
/// `plugin__<plugin id, '-' as '_'>__<tool name>`. The id's dashes become underscores
/// because MCP clients treat a tool name as an identifier, and the doubled underscore
/// separates the two halves.
///
/// The invariant this rests on is enforced at registration, not here:
/// `atlas_core::settings::validate_plugin_tool_decls` refuses a plugin id containing
/// `--` and a tool name containing `__`, so after the dash rewrite neither half can hold
/// a `__` of its own and the separator is the only one in the name. Without those two
/// rules the mapping would not be injective: plugin `a--b` with tool `count` and plugin
/// `a` with tool `b__count` would both produce `plugin__a__b__count`.
pub fn plugin_tool_name(plugin_id: &str, name: &str) -> String {
    format!("{PLUGIN_TOOL_PREFIX}{}__{name}", plugin_id.replace('-', "_"))
}

/// The inverse of [`plugin_tool_name`]: the underscored plugin id and the tool name, or
/// `None` for any name that is not a plugin tool's. The id comes back underscored
/// because the mapping loses which underscores were dashes; the caller resolves the real
/// id against the registered decls.
///
/// Splitting on the *first* `__` after the prefix is exact rather than a guess: a
/// registered id holds no `--` and so no `__` after the rewrite, so the first one is the
/// separator. A name built any other way is not one this function has to recover.
pub fn parse_plugin_tool_name(mcp_name: &str) -> Option<(String, String)> {
    let rest = mcp_name.strip_prefix(PLUGIN_TOOL_PREFIX)?;
    let (plugin_id, name) = rest.split_once("__")?;
    if plugin_id.is_empty() || name.is_empty() { return None; }
    Some((plugin_id.to_string(), name.to_string()))
}

/// The rmcp tool a plugin decl is listed as. `args` is handed through as the tool's
/// `inputSchema` unchanged; anything that is not a JSON object (which
/// `validate_plugin_tool_decls` refuses at registration) degrades to an empty object
/// rather than dropping the tool.
fn plugin_tool(decl: &PluginToolDecl) -> Tool {
    let schema = decl.args.as_object().cloned().unwrap_or_default();
    Tool::new(plugin_tool_name(&decl.plugin_id, &decl.name), decl.description.clone(), Arc::new(schema))
}

/// One row of the MCP tool status table: enough to render `docs/usage.md`'s table and
/// the desktop Settings card without either hand-typing the other's copy or this
/// crate depending on either. `atlas-mcp-tool-table-matches-the-router` (below) checks
/// this against the tool router itself so the two cannot drift silently; `atlas-core`'s
/// `settings::MCP_TOOL_NAMES` is checked against it the same way.
pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    /// A short, comma-joined summary of arguments, `*` marking a required one.
    pub args: &'static str,
    pub scope: ToolScope,
}

pub const TOOL_TABLE: &[ToolMeta] = &[
    ToolMeta { name: "memory_remember", description: "Store a memory shared with every agent. Use for facts about the project, decisions and their reasons, user preferences, and insights worth keeping.", args: "text*, kind, tags, scope, project_id, project_root, source_agent", scope: ToolScope::Write },
    ToolMeta { name: "memory_search", description: "Search shared memory with a natural-language query. Returns ranked memories with scores. Call this before starting work on a task to pick up prior decisions and preferences.", args: "query*, limit, scope, kinds, tags, project_id, project_root", scope: ToolScope::Read },
    ToolMeta { name: "memory_list", description: "List memories without a search query, narrowed by kind, tag, project and time. Call to browse what is stored rather than search for something specific.", args: "kinds, tags, project_id, project_root, since, limit", scope: ToolScope::Read },
    ToolMeta { name: "memory_forget", description: "Mark a memory as superseded so it stops appearing in recall. Nothing is deleted.", args: "id*, reason", scope: ToolScope::Write },
    ToolMeta { name: "memory_review", description: "Accept a pending memory into active use, or reject it. Pending memories come from an actor whose project requires review.", args: "id*, decision*", scope: ToolScope::Write },
    ToolMeta { name: "project_list", description: "List every project Atlas knows about.", args: "none", scope: ToolScope::Read },
    ToolMeta { name: "project_get", description: "Fetch one project by name.", args: "name*", scope: ToolScope::Read },
    ToolMeta { name: "project_connect", description: "Register a repository with Atlas and build its profile (languages, frameworks, tree, recent commits). Call this once when starting work in a repository Atlas has not seen.", args: "root_path*", scope: ToolScope::Write },
    ToolMeta { name: "project_context", description: "Call at the start of a session to load the current project's profile, practices, workflows and top memories. Pass project_root to name a repository other than the one this server was started in.", args: "project_root", scope: ToolScope::Write },
    ToolMeta { name: "task_create", description: "Create a task on the board. Pass project_root to name a project other than the one this server was started in, or \"global\" for a task with no project.", args: "title*, description, kind, priority, labels, parent, blocked_by, project_root, agent", scope: ToolScope::Write },
    ToolMeta { name: "task_list", description: "List board tasks for the current project. Pass ready=true to get work whose blockers are done and is safe to start; pass stage or assignee to narrow further, or project_root: \"global\" for tasks with no project.", args: "project_root, stage, assignee, ready, query, include_done, agent", scope: ToolScope::Read },
    ToolMeta { name: "task_get", description: "Fetch one task by key, including its subtasks and full event history. Call before updating or moving a task you have not read recently.", args: "key*, agent", scope: ToolScope::Read },
    ToolMeta { name: "task_claim", description: "Claim a task: assign it to you and, if it is still in the board's first stage, move it to the second. Fails if someone else already holds it unless force is set.", args: "key*, force, agent", scope: ToolScope::Write },
    ToolMeta { name: "task_move", description: "Move a task to another stage on its board. Fails naming the valid stages if the stage does not exist, or with a conflict if expected_updated_at no longer matches.", args: "key*, stage*, expected_updated_at, agent", scope: ToolScope::Write },
    ToolMeta { name: "task_comment", description: "Add a comment to a task's history. Use it to record progress, verification notes, or why a task was moved.", args: "key*, body*, agent", scope: ToolScope::Write },
    ToolMeta { name: "task_block", description: "Record the tasks a task waits on, by key. This replaces the whole blocker list, so pass every blocker that still applies; an empty list clears them. A task with an open blocker drops out of task_list(ready=true) until that blocker is done.", args: "key*, blocked_by*, agent", scope: ToolScope::Write },
    ToolMeta { name: "task_update", description: "Edit a task's title, description, kind, priority, assignee or labels. Pass expected_updated_at, from a prior task_get or task_list, to fail with a conflict instead of overwriting a concurrent change.", args: "key*, title, description, kind, priority, assignee, labels, expected_updated_at, agent", scope: ToolScope::Write },
    ToolMeta { name: "board_stages", description: "List the stages this project's board moves tasks through, in order. Call before task_move so you never invent a stage name.", args: "project_root", scope: ToolScope::Read },
    ToolMeta { name: "practice_list", description: "List the coding practices that apply here: the global ones plus any scoped to this project, optionally narrowed by tag. Call before writing code so the work follows the house style.", args: "project_root, tags", scope: ToolScope::Read },
    ToolMeta { name: "practice_get", description: "Fetch the full text of one practice by name. Call after practice_list when a practice looks relevant to the task.", args: "name*", scope: ToolScope::Read },
    ToolMeta { name: "agent_list", description: "List the agent roles stored in Atlas, with their descriptions. Call this to see which specialist role fits the task before doing the work yourself.", args: "project_root", scope: ToolScope::Read },
    ToolMeta { name: "agent_get", description: "Fetch one agent role by name, including its full instructions. Call after agent_list to adopt the role.", args: "name*", scope: ToolScope::Read },
    ToolMeta { name: "agent_save", description: "Create or update an agent role so every coding agent on this machine can use it. Call when the user describes a repeatable specialist role worth keeping.", args: "name*, description*, instructions*, model_hint, tools, tags", scope: ToolScope::Write },
    ToolMeta { name: "workflow_list", description: "List the workflows that apply here: the global ones plus any scoped to this project. Call when the user asks for a multi-step process such as a release or a review.", args: "project_root", scope: ToolScope::Read },
    ToolMeta { name: "workflow_get", description: "Fetch one workflow by name, including its full graph. Call after workflow_list to see its trigger and actions.", args: "name*", scope: ToolScope::Read },
    ToolMeta { name: "workflow_run", description: "Start a workflow run by name and answer with its run id and number. Call workflow_status to follow it. Fails with a conflict if the workflow already has a run queued or running.", args: "name*, input", scope: ToolScope::Write },
    ToolMeta { name: "workflow_status", description: "Report a workflow run's status: the run's own state plus a summary of each step (name, status, duration, last log line). Call after workflow_run to follow progress.", args: "run_id*", scope: ToolScope::Read },
    ToolMeta { name: "ingest_transcript", description: "Queue a conversation transcript for opt-in LLM extraction of durable memories. Returns a job id to poll; fails if extraction is not enabled and configured on the daemon.", args: "text*, agent, project_root", scope: ToolScope::Write },
    ToolMeta { name: "status", description: "Report Atlas daemon status: version, database path, active memory count, embedding availability.", args: "none", scope: ToolScope::Read },
    ToolMeta { name: "framework_docs", description: "List the planning frameworks detected in a project (Superpowers, OpenSpec, SpecKit, GSD) with the documents each holds, or fetch one document's text. Pass kind and path together, from a prior listing, to read a document.", args: "project_root, kind, path", scope: ToolScope::Read },
    ToolMeta { name: "skill_list", description: "List the skills that apply here: the SKILL.md folders Claude Code and Codex read, the ones installed plugins carry, and Atlas's own. A project's switched-off skills are left out. Call before starting work to see which skills are in play.", args: "project_root", scope: ToolScope::Read },
    ToolMeta { name: "skill_get", description: "Fetch one skill's full text by id, from a prior skill_list. Call when a listed skill looks relevant to the task.", args: "id*, project_root", scope: ToolScope::Read },
];

/// Called on every accepted `call_tool`, so the daemon and the stdio shim can each
/// track connected MCP clients (`atlasd::mcp_clients`) without this crate knowing
/// anything about that registry. `session_id` is the `Mcp-Session-Id` header read out
/// of the streamable HTTP transport's injected request parts (there is no clean
/// `initialize` hook in rmcp's streamable HTTP service), `None` for the stdio shim,
/// which has no such header and registers itself explicitly over the API instead. The
/// last argument is the project this call resolved (the same one project gating just
/// checked), `None` when the call named none; best effort, since the registry only
/// learns of it on a call the router happened to resolve a project for.
pub type OnToolCall = Arc<dyn Fn(Option<String>, Option<Implementation>, Option<ProtocolVersion>, Option<Uuid>) + Send + Sync>;

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
    on_tool_call: Option<OnToolCall>,
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
/// Like [`err`], but for the board tools: a `Conflict` (someone else holds the
/// task, or `expected_updated_at` no longer matches) is the caller's to retry, not a
/// server fault, so it is reported as `invalid_params` too, prefixed `conflict:` so
/// an agent can tell it apart from a plain mistake and decide whether to re-read and
/// retry. `NotFound` is spelled out the same way, since rmcp has no error kind for
/// either.
fn board_err(e: atlas_core::AtlasError) -> McpError {
    match e {
        atlas_core::AtlasError::Conflict(m) => McpError::invalid_params(format!("conflict: {m}"), None),
        atlas_core::AtlasError::NotFound(m) => McpError::invalid_params(format!("not found: {m}"), None),
        other => err(other),
    }
}

fn json_result<T: serde::Serialize>(v: &T) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(v).map_err(|e| McpError::internal_error(e.to_string(), None))?)]))
}

#[tool_router]
impl<B: Backend> AtlasMcp<B> {
    pub fn new(backend: Arc<B>) -> Self {
        Self { backend, source_tool: source_tool_label(), project_root: None, env_project_root: true, projects: Arc::default(), on_tool_call: None, tool_router: Self::tool_router() }
    }

    /// Registers a hook run on every accepted `call_tool`. See [`OnToolCall`].
    pub fn with_on_tool_call(mut self, hook: OnToolCall) -> Self { self.on_tool_call = Some(hook); self }

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
    /// Reads go through here so they stay reads: `project_connect` upserts the row and
    /// writes an audit entry, which a recall or a doc listing has no business doing on
    /// every call. `project_connect` and `project_context` are explicit connects and
    /// refresh the entry instead.
    ///
    /// When `project_connect` is in `mcp.disabled_tools`, this never connects: it only
    /// matches `root` against a project Atlas already knows (the same read-only match
    /// `resolve_project_segment` gives a resource read) and fails with
    /// [`PROJECT_CONNECT_DISABLED`] otherwise. Disabling the one tool that registers a
    /// repository must actually stop every other tool from doing it implicitly through
    /// a `project_root` argument.
    async fn resolve_project(&self, project_root: Option<PathBuf>) -> Result<Option<Project>, McpError> {
        let Some(root) = self.root_for(project_root) else { return Ok(None) };
        if let Some(p) = self.cached(&root) { return Ok(Some(p)); }
        if self.disabled_tools().await?.contains("project_connect") {
            return match self.resolve_project_segment(&root.to_string_lossy()).await {
                Ok(Some(id)) => {
                    let project = self.backend.get_project(id).await.map_err(err)?;
                    self.cache(root, &project);
                    Ok(Some(project))
                }
                Ok(None) | Err(atlas_core::AtlasError::NotFound(_)) => Err(McpError::invalid_params(PROJECT_CONNECT_DISABLED, None)),
                Err(e) => Err(err(e)),
            };
        }
        let project = self.backend.connect_project(root.clone(), "mcp").await.map_err(err)?;
        self.cache(root, &project);
        Ok(Some(project))
    }

    /// The project for the resolved root, for project-level MCP gating only (Task
    /// MCP-A). Unlike `resolve_project`, this never connects: it always takes the
    /// read-only "match an already-known project" step (the same one
    /// `resolve_project_segment` gives a resource read, and `resolve_project` itself
    /// falls back to when `project_connect` is disabled), regardless of whether
    /// `project_connect` is disabled. Gating only needs to know which already-known
    /// project's override applies to this call; it must never be the thing that
    /// upserts a row, writes an audit entry, or triggers a profile build for a project
    /// nobody has connected yet, which `resolve_project`'s connecting branch would do
    /// on every uncached root while `project_connect` is enabled.
    ///
    /// A hit is not cached here: caching it would make a later, real `resolve_project`
    /// call from the tool's own logic (if it needs one) see a stale hit and skip the
    /// connect (and the profile refresh) it should still perform. Reading the cache
    /// first is still safe and saves a rescan when an earlier real connect already
    /// populated it.
    async fn resolve_project_for_gating(&self, project_root: Option<PathBuf>) -> Result<Option<Project>, McpError> {
        let Some(root) = self.root_for(project_root) else { return Ok(None) };
        if let Some(p) = self.cached(&root) { return Ok(Some(p)); }
        match self.resolve_project_segment(&root.to_string_lossy()).await {
            Ok(Some(id)) => Ok(Some(self.backend.get_project(id).await.map_err(err)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(err(e)),
        }
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
    /// global ones, or, when no project resolves, only the global ones. The store's
    /// unfiltered listing spans every project, which is right for a resource listing
    /// but not for a tool that says it lists what applies here.
    async fn docs_here(&self, kind: DocKind, project_root: Option<PathBuf>) -> Result<Vec<Doc>, McpError> {
        let id = self.resolve_project(project_root).await?.map(|p| p.id);
        let docs = self.backend.list_docs(kind, id).await.map_err(err)?;
        Ok(match id { Some(_) => docs, None => docs.into_iter().filter(|d| d.project_id.is_none()).collect() })
    }

    /// The workflows that apply where this call is coming from, as summaries. Mirrors
    /// `docs_here`: the project's plus the global ones, or, when no project resolves,
    /// only the global ones.
    async fn workflows_here(&self, project_root: Option<PathBuf>) -> Result<Vec<WorkflowSummary>, McpError> {
        let id = self.resolve_project(project_root).await?.map(|p| p.id);
        let workflows = self.backend.list_workflows(id).await.map_err(err)?;
        let workflows = match id {
            Some(_) => workflows,
            None => workflows.into_iter().filter(|w| w.project_id.is_none()).collect(),
        };
        Ok(workflows.iter().map(WorkflowSummary::from).collect())
    }

    /// The project id to scope by, or `None` for global. `project_id` wins over any
    /// root, so an explicit id never triggers a project lookup.
    async fn scope_id(&self, project_id: Option<Uuid>, project_root: Option<PathBuf>) -> Result<Option<Uuid>, McpError> {
        match project_id {
            Some(id) => Ok(Some(id)),
            None => Ok(self.resolve_project(project_root).await?.map(|p| p.id)),
        }
    }

    /// The actor a board write is recorded under: this server's `source_tool` label,
    /// plus `/<agent>` when the caller named one, so the event history reads as a
    /// sentence naming the specific agent ("claude-code/reviewer moved ATL-12").
    fn actor(&self, agent: &Option<String>) -> String {
        match agent.as_deref().map(str::trim) {
            Some(a) if !a.is_empty() => format!("{}/{a}", self.source_tool),
            _ => self.source_tool.clone(),
        }
    }

    /// Resolves `project_root` to the project scope a board tool should use. The
    /// literal value "global" names the project-less board explicitly; otherwise the
    /// usual precedence applies (the argument, then `ATLAS_PROJECT_ROOT`, then the
    /// root this server was started in). Unlike `memory_remember`, which is content to
    /// stay unscoped, a board tool errors when none of those resolves: a task's key
    /// is a project prefix, so there is nowhere to file it without one.
    ///
    /// The second element of the returned pair is `true` only when the caller passed
    /// the literal "global": callers that need to distinguish "no project scope was
    /// asked for" (list everything) from "the project-less board was asked for
    /// explicitly" (list only project-less tasks) read it; callers that only need the
    /// scoping id ignore it.
    async fn board_project_id(&self, project_root: Option<PathBuf>) -> Result<(Option<Uuid>, bool), McpError> {
        if project_root.as_deref().map(|p| p.as_os_str() == "global").unwrap_or(false) {
            return Ok((None, true));
        }
        match self.resolve_project(project_root).await? {
            Some(p) => Ok((Some(p.id), false)),
            None => Err(McpError::invalid_params(
                "no project is connected: pass project_root, set ATLAS_PROJECT_ROOT, or pass project_root: \"global\" for the global board",
                None,
            )),
        }
    }

    /// Every known MCP tool name currently in `mcp.disabled_tools`, or the built-in
    /// default (`project_connect`, `memory_review`) when the setting has never been
    /// written. Read fresh on every `list_tools`/`call_tool`, the same
    /// read-live-not-cached pattern the backend's own settings reads use, so a change
    /// takes effect on the next call rather than after a restart.
    async fn disabled_tools(&self) -> Result<std::collections::HashSet<String>, McpError> {
        disabled_tool_names(&*self.backend).await.map_err(err)
    }

    #[tool(description = "List board tasks for the current project. Pass ready=true to get work whose blockers are done and is safe to start; pass stage or assignee to narrow further, or project_root: \"global\" for tasks with no project.")]
    async fn task_list(&self, Parameters(a): Parameters<TaskListArgs>) -> Result<CallToolResult, McpError> {
        let (project_id, global_only) = self.board_project_id(a.project_root).await?;
        let filter = TaskFilter {
            project_id,
            stage: a.stage,
            assignee: a.assignee,
            ready: a.ready.unwrap_or(false),
            query: a.query,
            include_done: a.include_done.unwrap_or(false),
            global_only,
            top_level: None,
        };
        json_result(&self.backend.list_tasks(filter).await.map_err(board_err)?)
    }

    #[tool(description = "Fetch one task by key, including its subtasks and full event history. Call before updating or moving a task you have not read recently.")]
    async fn task_get(&self, Parameters(a): Parameters<TaskKeyArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_task(&a.key).await.map_err(board_err)?)
    }

    #[tool(description = "Create a task on the board. Pass project_root to name a project other than the one this server was started in, or \"global\" for a task with no project.")]
    async fn task_create(&self, Parameters(a): Parameters<TaskCreateArgs>) -> Result<CallToolResult, McpError> {
        let (project_id, _) = self.board_project_id(a.project_root).await?;
        let new = NewTask {
            project_id,
            title: a.title,
            description: a.description,
            kind: a.kind,
            priority: a.priority,
            assignee: None,
            labels: a.labels,
            parent: a.parent,
            blocked_by: a.blocked_by,
            stage: None,
            source_ref: None,
        };
        let actor = self.actor(&a.agent);
        json_result(&self.backend.create_task(new, &actor).await.map_err(board_err)?)
    }

    #[tool(description = "Edit a task's title, description, kind, priority, assignee or labels. Pass expected_updated_at, from a prior task_get or task_list, to fail with a conflict instead of overwriting a concurrent change.")]
    async fn task_update(&self, Parameters(a): Parameters<TaskUpdateArgs>) -> Result<CallToolResult, McpError> {
        let upd = TaskUpdate {
            title: a.title,
            description: a.description,
            kind: a.kind,
            priority: a.priority,
            // An empty string clears the assignee (`Some(None)`); absent leaves it
            // alone (`None`); anything else sets it (`Some(Some(v))`).
            assignee: a.assignee.map(|v| if v.is_empty() { None } else { Some(v) }),
            labels: a.labels,
            parent: None,
            expected_updated_at: a.expected_updated_at,
        };
        let actor = self.actor(&a.agent);
        json_result(&self.backend.update_task(&a.key, upd, &actor).await.map_err(board_err)?)
    }

    #[tool(description = "Move a task to another stage on its board. Fails naming the valid stages if the stage does not exist, or with a conflict if expected_updated_at no longer matches.")]
    async fn task_move(&self, Parameters(a): Parameters<TaskMoveArgs>) -> Result<CallToolResult, McpError> {
        let actor = self.actor(&a.agent);
        json_result(&self.backend.move_task(&a.key, &a.stage, a.expected_updated_at, &actor).await.map_err(board_err)?)
    }

    #[tool(description = "Add a comment to a task's history. Use it to record progress, verification notes, or why a task was moved.")]
    async fn task_comment(&self, Parameters(a): Parameters<TaskCommentArgs>) -> Result<CallToolResult, McpError> {
        let actor = self.actor(&a.agent);
        json_result(&self.backend.comment_task(&a.key, &a.body, &actor).await.map_err(board_err)?)
    }

    #[tool(description = "Claim a task: assign it to you and, if it is still in the board's first stage, move it to the second. Fails if someone else already holds it unless force is set.")]
    async fn task_claim(&self, Parameters(a): Parameters<TaskClaimArgs>) -> Result<CallToolResult, McpError> {
        let actor = self.actor(&a.agent);
        json_result(&self.backend.claim_task(&a.key, a.force.unwrap_or(false), &actor).await.map_err(board_err)?)
    }

    #[tool(description = "Record the tasks a task waits on, by key. This replaces the whole blocker list, so pass every blocker that still applies; an empty list clears them. A task with an open blocker drops out of task_list(ready=true) until that blocker is done.")]
    async fn task_block(&self, Parameters(a): Parameters<TaskBlockArgs>) -> Result<CallToolResult, McpError> {
        let actor = self.actor(&a.agent);
        json_result(&self.backend.set_task_blockers(&a.key, a.blocked_by, &actor).await.map_err(board_err)?)
    }

    #[tool(description = "List the stages this project's board moves tasks through, in order. Call before task_move so you never invent a stage name.")]
    async fn board_stages(&self, Parameters(a): Parameters<ProjectRootArgs>) -> Result<CallToolResult, McpError> {
        let (project_id, _) = self.board_project_id(a.project_root).await?;
        json_result(&self.backend.board_stages(project_id).await.map_err(board_err)?)
    }

    #[tool(description = "List the planning frameworks detected in a project (Superpowers, OpenSpec, SpecKit, GSD) with the documents each holds, or fetch one document's text. Pass kind and path together, from a prior listing, to read a document.")]
    async fn framework_docs(&self, Parameters(a): Parameters<FrameworkDocsArgs>) -> Result<CallToolResult, McpError> {
        let project = self
            .resolve_project(a.project_root)
            .await?
            .ok_or_else(|| McpError::invalid_params("no project is connected: pass project_root or set ATLAS_PROJECT_ROOT", None))?;
        if let Some(path) = &a.path {
            let kind = a.kind.ok_or_else(|| McpError::invalid_params("path requires kind", None))?;
            let content = self.backend.get_framework_doc(project.id, kind, path).await.map_err(err)?;
            return json_result(&serde_json::json!({"kind": kind, "path": path, "content": content}));
        }
        let mut listings = self.backend.list_frameworks(project.id).await.map_err(err)?;
        if let Some(kind) = a.kind {
            listings.retain(|l| l.inventory.kind == kind);
        }
        json_result(&listings)
    }

    #[tool(description = "List the skills that apply here: the SKILL.md folders Claude Code and Codex read, the ones installed plugins carry, and Atlas's own. A project's switched-off skills are left out. Call before starting work to see which skills are in play.")]
    async fn skill_list(&self, Parameters(a): Parameters<SkillListArgs>) -> Result<CallToolResult, McpError> {
        let project_id = self.resolve_project(a.project_root).await?.map(|p| p.id);
        let list = self.backend.list_skills(project_id).await.map_err(err)?;
        json_result(&SkillList {
            // A skill this project switched off is not one an agent should reach for,
            // so the tool answers with the enabled set rather than the whole list.
            skills: list.skills.into_iter().filter(|s| s.enabled_here != Some(false)).collect(),
            warnings: list.warnings,
        })
    }

    #[tool(description = "Fetch one skill's full text by id, from a prior skill_list. Call when a listed skill looks relevant to the task.")]
    async fn skill_get(&self, Parameters(a): Parameters<SkillGetArgs>) -> Result<CallToolResult, McpError> {
        let project_id = self.resolve_project(a.project_root).await?.map(|p| p.id);
        json_result(&self.backend.get_skill(project_id, &a.id).await.map_err(err)?)
    }

    /// Resolves the `{name}` segment of an `atlas://projects/{name}/...` resource URI
    /// to a project scope: "global" names the project-less scope, otherwise `raw` is
    /// tried, in order, as a project id, a project name (case-insensitive), a board key
    /// (`ATL`), or a percent-encoded project root path, against whatever `list_projects`
    /// already has on file. Read-only: unlike a tool argument, a resource read never
    /// connects a project that is not already known.
    async fn resolve_project_segment(&self, raw: &str) -> Result<Option<Uuid>, atlas_core::AtlasError> {
        let decoded = percent_decode_str(raw)
            .decode_utf8()
            .map_err(|e| atlas_core::AtlasError::Invalid(format!("project URI segment is not valid UTF-8: {e}")))?
            .into_owned();
        if decoded == "global" {
            return Ok(None);
        }
        let projects = self.backend.list_projects().await?;
        if let Ok(id) = decoded.parse::<Uuid>() {
            if let Some(p) = projects.iter().find(|p| p.id == id) {
                return Ok(Some(p.id));
            }
        }
        if let Some(p) = projects.iter().find(|p| p.name.eq_ignore_ascii_case(&decoded)) {
            return Ok(Some(p.id));
        }
        if let Some(p) = projects.iter().find(|p| p.board_key.as_deref().map(|k| k.eq_ignore_ascii_case(&decoded)).unwrap_or(false)) {
            return Ok(Some(p.id));
        }
        let wanted = std::path::Path::new(&decoded);
        let canon = wanted.canonicalize();
        if let Some(p) = projects.iter().find(|p| {
            let root = std::path::Path::new(&p.root_path);
            root == wanted || canon.as_deref().ok() == Some(root)
        }) {
            return Ok(Some(p.id));
        }
        Err(atlas_core::AtlasError::NotFound(format!("project {decoded}")))
    }

    /// Assembles a project's `ProjectContext` (profile, practices, workflows, top
    /// memories) without connecting it or refreshing its profile: the read-only
    /// counterpart to the `project_context` tool, for the `atlas://projects/{name}/context`
    /// resource. Mirrors `LocalBackend::project_context`'s memory query (a search plus a
    /// top-up of the newest project-scoped memories) using only read calls.
    async fn project_context_readonly(&self, project_id: Uuid) -> Result<ProjectContext, atlas_core::AtlasError> {
        let project = self.backend.get_project(project_id).await?;
        let query = match &project.profile {
            Some(p) => format!("{} {}", p.name, p.frameworks.join(" ")),
            None => project.name.clone(),
        };
        let mut memories = self.backend.recall(RecallQuery {
            query, limit: 20, scope: None, list_scope: MemoryScopeFilter::All, project_id: Some(project.id), kinds: vec![], tags: vec![],
        }).await?;
        let seen: std::collections::HashSet<Uuid> = memories.iter().map(|h| h.memory.id).collect();
        let recent = self.backend.list_memories(MemoryStatus::Active, Some(project.id), MemoryScopeFilter::ProjectOnly).await?;
        memories.extend(recent.into_iter().filter(|m| !seen.contains(&m.id)).take(10).map(|memory| RecallHit { memory, score: 0.0 }));
        // The skills that actually apply here, the same filter `LocalBackend::project_context`
        // uses, so both readings of a project's context agree.
        let skills = self.backend.list_skills(Some(project.id)).await
            .map(|l| l.skills.into_iter().filter(|s| s.enabled_here != Some(false)).collect())
            .unwrap_or_default();
        Ok(ProjectContext {
            practices: self.backend.list_docs(DocKind::Practice, Some(project.id)).await?,
            workflows: self.backend.list_workflows(Some(project.id)).await?.iter().map(WorkflowSummary::from).collect(),
            skills,
            project,
            memories,
        })
    }

    #[tool(description = "Store a memory shared with every agent. Use for facts about the project, decisions and their reasons, user preferences, and insights worth keeping.")]
    async fn memory_remember(&self, Parameters(a): Parameters<MemoryRememberArgs>) -> Result<CallToolResult, McpError> {
        let kind = a.kind.as_deref().unwrap_or("fact").parse::<MemoryKind>().map_err(err)?;
        let asked = match a.scope.as_deref() { Some(s) => Some(s.parse::<MemoryScope>().map_err(err)?), None => None };
        // A memory the caller called global stays unattached even in a project session.
        let project_id = match asked { Some(MemoryScope::Global) => None, _ => self.scope_id(a.project_id, a.project_root).await? };
        let scope = asked.unwrap_or(if project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global });
        // The actor names the tool and, when the caller gave one, the agent, the same
        // shape a board write records. The project's `agent_access` is checked against
        // it on the daemon side, and `require_review` may land the memory pending.
        let actor = self.actor(&a.source_agent);
        let m = NewMemory { scope, project_id, kind, text: a.text, tags: a.tags.unwrap_or_default(), source_agent: a.source_agent, source_tool: Some(self.source_tool.clone()), confidence: 1.0, status: MemoryStatus::Active };
        json_result(&self.backend.remember(m, &actor).await.map_err(board_err)?)
    }

    #[tool(description = "Search shared memory with a natural-language query. Returns ranked memories with scores. Call this before starting work on a task to pick up prior decisions and preferences.")]
    async fn memory_search(&self, Parameters(a): Parameters<MemorySearchArgs>) -> Result<CallToolResult, McpError> {
        let scope = match a.scope.as_deref() { Some(s) => Some(s.parse::<MemoryScope>().map_err(err)?), None => None };
        let mut kinds = vec![]; for k in a.kinds.unwrap_or_default() { kinds.push(k.parse::<MemoryKind>().map_err(err)?); }
        // A project id widens rather than narrows: the store returns that project's
        // memories alongside the global ones.
        let project_id = match scope { Some(MemoryScope::Global) => None, _ => self.scope_id(a.project_id, a.project_root).await? };
        let q = RecallQuery { query: a.query, limit: a.limit.unwrap_or(10), scope, list_scope: MemoryScopeFilter::All, project_id, kinds, tags: a.tags.unwrap_or_default() };
        json_result(&self.backend.recall(q).await.map_err(err)?)
    }

    #[tool(description = "List memories without a search query, narrowed by kind, tag, project and time. Call to browse what is stored rather than search for something specific.")]
    async fn memory_list(&self, Parameters(a): Parameters<MemoryListArgs>) -> Result<CallToolResult, McpError> {
        let mut kinds = vec![]; for k in a.kinds.unwrap_or_default() { kinds.push(k.parse::<MemoryKind>().map_err(err)?); }
        let project_id = self.scope_id(a.project_id, a.project_root).await?;
        let mut memories = self.backend.list_memories(MemoryStatus::Active, project_id, MemoryScopeFilter::All).await.map_err(err)?;
        if !kinds.is_empty() {
            memories.retain(|m| kinds.contains(&m.kind));
        }
        if let Some(tags) = &a.tags {
            if !tags.is_empty() {
                memories.retain(|m| tags.iter().any(|t| m.tags.contains(t)));
            }
        }
        if let Some(since) = a.since {
            memories.retain(|m| m.created_at >= since);
        }
        memories.truncate(a.limit.unwrap_or(50));
        json_result(&memories)
    }

    #[tool(description = "Accept a pending memory into active use, or reject it. Pending memories come from an actor whose project requires review.")]
    async fn memory_review(&self, Parameters(a): Parameters<MemoryReviewArgs>) -> Result<CallToolResult, McpError> {
        let status = match a.decision.trim().to_lowercase().as_str() {
            "accept" => MemoryStatus::Active,
            "reject" => MemoryStatus::Rejected,
            other => return Err(McpError::invalid_params(format!("decision must be \"accept\" or \"reject\", got \"{other}\""), None)),
        };
        json_result(&self.backend.set_memory_status(a.id, status, "mcp").await.map_err(err)?)
    }

    #[tool(description = "Mark a memory as superseded so it stops appearing in recall. Nothing is deleted.")]
    async fn memory_forget(&self, Parameters(a): Parameters<MemoryForgetArgs>) -> Result<CallToolResult, McpError> {
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
    async fn project_connect(&self, Parameters(a): Parameters<ProjectConnectArgs>) -> Result<CallToolResult, McpError> {
        let project = self.backend.connect_project(a.root_path.clone(), "mcp").await.map_err(err)?;
        self.cache(a.root_path, &project);
        json_result(&project)
    }

    #[tool(description = "List every project Atlas knows about.")]
    async fn project_list(&self) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.list_projects().await.map_err(err)?)
    }

    #[tool(description = "Fetch one project by name.")]
    async fn project_get(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        let projects = self.backend.list_projects().await.map_err(err)?;
        let project = projects
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(&a.name))
            .ok_or_else(|| McpError::invalid_params(format!("no project named '{}'", a.name), None))?;
        json_result(&project)
    }

    #[tool(description = "List the agent roles stored in Atlas, with their descriptions. Call this to see which specialist role fits the task before doing the work yourself.")]
    async fn agent_list(&self, Parameters(_a): Parameters<AgentListArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.list_agents().await.map_err(err)?)
    }

    #[tool(description = "Fetch one agent role by name, including its full instructions. Call after agent_list to adopt the role.")]
    async fn agent_get(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_agent(&a.name).await.map_err(err)?)
    }

    #[tool(description = "Create or update an agent role so every coding agent on this machine can use it. Call when the user describes a repeatable specialist role worth keeping.")]
    async fn agent_save(&self, Parameters(a): Parameters<SaveAgentArgs>) -> Result<CallToolResult, McpError> {
        let agent = NewAgent { name: a.name, description: a.description, instructions: a.instructions, model_hint: a.model_hint, tools: a.tools.unwrap_or_default(), tags: a.tags.unwrap_or_default() };
        json_result(&self.backend.save_agent(agent, "mcp").await.map_err(err)?)
    }

    #[tool(description = "List the coding practices that apply here: the global ones plus any scoped to this project, optionally narrowed by tag. Call before writing code so the work follows the house style.")]
    async fn practice_list(&self, Parameters(a): Parameters<PracticeListArgs>) -> Result<CallToolResult, McpError> {
        let mut docs = self.docs_here(DocKind::Practice, a.project_root).await?;
        if let Some(tags) = &a.tags {
            if !tags.is_empty() {
                docs.retain(|d| tags.iter().any(|t| d.tags.contains(t)));
            }
        }
        json_result(&docs)
    }

    #[tool(description = "Fetch the full text of one practice by name. Call after practice_list when a practice looks relevant to the task.")]
    async fn practice_get(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_doc(DocKind::Practice, &a.name).await.map_err(err)?)
    }

    #[tool(description = "List the workflows that apply here: the global ones plus any scoped to this project. Call when the user asks for a multi-step process such as a release or a review.")]
    async fn workflow_list(&self, Parameters(a): Parameters<ProjectRootArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.workflows_here(a.project_root).await?)
    }

    #[tool(description = "Fetch one workflow by name, including its full graph. Call after workflow_list to see its trigger and actions.")]
    async fn workflow_get(&self, Parameters(a): Parameters<NameArgs>) -> Result<CallToolResult, McpError> {
        json_result(&self.backend.get_workflow(&a.name).await.map_err(err)?)
    }

    #[tool(description = "Queue a conversation transcript for opt-in LLM extraction of durable memories. Returns a job id to poll; fails if extraction is not enabled and configured on the daemon.")]
    async fn ingest_transcript(&self, Parameters(a): Parameters<IngestTranscriptArgs>) -> Result<CallToolResult, McpError> {
        // The actor is always this server's own label (plus the agent the caller names),
        // never a string the caller writes: the project's agent-access gate checks it,
        // and an exempt label like `desktop` must not be nameable from a tool call.
        let source_tool = self.actor(&a.agent);
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

    #[tool(description = "Start a workflow run by name and answer with its run id and number. Call workflow_status to follow it. Fails with a conflict if the workflow already has a run queued or running.")]
    async fn workflow_run(&self, Parameters(a): Parameters<WorkflowRunArgs>) -> Result<CallToolResult, McpError> {
        // Reached over MCP, a run always counts as a `prompt` trigger: it was an agent,
        // not the schedule or a human at the CLI, that asked for it.
        let run = self.backend.run_workflow(&a.name, TriggerKind::Prompt, &self.source_tool, a.input).await.map_err(board_err)?;
        json_result(&serde_json::json!({"run_id": run.id, "number": run.number}))
    }

    #[tool(description = "Report a workflow run's status: the run's own state plus a summary of each step (name, status, duration, last log line). Call after workflow_run to follow progress.")]
    async fn workflow_status(&self, Parameters(a): Parameters<WorkflowStatusArgs>) -> Result<CallToolResult, McpError> {
        let (run, steps) = self.backend.get_run(a.run_id).await.map_err(board_err)?;
        let steps: Vec<_> = steps
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "status": s.status,
                    "duration_ms": s.finished_at.map(|f| (f - s.started_at).num_milliseconds()),
                    "last_log": s.log.last().map(|l| l.text.clone()),
                })
            })
            .collect();
        json_result(&serde_json::json!({"run": run, "steps": steps}))
    }
}

const AGENTS: &str = "atlas://agents/";
const PRACTICES: &str = "atlas://practices/";
const WORKFLOWS: &str = "atlas://workflows/";
const PROJECTS: &str = "atlas://projects/";
const SKILLS: &str = "atlas://skills/";
const MEMORIES_RECENT: &str = "atlas://memories/recent";

/// How many memories `atlas://memories/recent` renders.
const MEMORIES_RECENT_LIMIT: usize = 50;

/// Everything but alphanumerics and `/ - _ . ~`: enough to escape a space or other
/// reserved character in a project name or root path so a `{name}` segment of an
/// `atlas://projects/{name}/...` URI stays a single legal path component, while
/// leaving the segment itself readable.
const PROJECT_URI_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~');

const MARKDOWN: &str = "text/markdown";
const JSON: &str = "application/json";

/// Returned when `project_connect` is disabled and a call names a root Atlas does not
/// already know: with the tool off, nothing else may register a project either.
const PROJECT_CONNECT_DISABLED: &str = "project is not connected; enable the project_connect tool or connect it from the desktop or CLI";

const BOARD_WORKFLOW_PROMPT: &str = "atlas.board_workflow";
const BOARD_WORKFLOW_TEXT: &str = "Work the task board like this: pick a ready task with task_list \
    (ready=true), claim it with task_claim, and comment progress with task_comment as you go. Move it \
    forward with task_move: into the testing stage with verification notes once the work is done, and \
    into a done stage only once that work is verified. Use task_list with ready=true, then task_claim, \
    then task_move as you progress; comment with task_comment; record a dependency you find mid-work \
    with task_block; never invent a stage, only move to one of the stages listed below.";

const BOOTSTRAP_PROMPT: &str = "atlas.bootstrap";
const BOOTSTRAP_DESCRIPTION: &str = "What Atlas is and how to recall, remember, use the board and find practices at the start of a session.";
const BOOTSTRAP_TEXT: &str = "Atlas is the shared memory and task board for every coding agent on this \
    machine, reached here over MCP. At the start of a session: call project_context (or project_connect \
    if this repository is new to Atlas) to load its profile, practices, workflows and top memories; call \
    memory_search before starting work on anything, to pick up prior decisions and preferences; call \
    practice_list to see the coding practices this project follows before writing code; and work the task \
    board with task_list(ready=true), task_claim, task_move and task_comment, in the order the \
    atlas.board_workflow prompt describes. Store what you learn with memory_remember: facts, decisions and \
    their reasons, user preferences, and insights worth keeping. Nothing is ever deleted from memory; \
    memory_forget only marks a memory superseded, and memory_list browses what is stored without a \
    search query.";

const HANDOFF_PROMPT: &str = "atlas.handoff";
const HANDOFF_DESCRIPTION: &str = "Summarise this session into memories and tasks before it ends.";
const HANDOFF_TEXT: &str = "Summarise this session before it ends, so the next agent, or your own next \
    session, does not have to rediscover it. Call memory_remember for every durable fact, decision, \
    preference or insight this session produced that is not already stored, one memory per idea, scoped \
    to the project it belongs to. For any work left unfinished, file it with task_create or bring an \
    existing task up to date with task_update, and record what happened with task_comment: what is done \
    and what remains. Do not restate what memory_search or task_list already shows as stored; only add \
    what this session made new.";

/// Markdown for `atlas://projects/{name}/practices`: the global practices plus this
/// project's own, or just the global ones for the literal "global" segment.
fn practices_markdown(project_name: Option<&str>, docs: &[Doc]) -> String {
    let mut out = match project_name {
        Some(name) => format!("# Practices for {name}\n\n"),
        None => "# Global practices\n\n".to_string(),
    };
    if docs.is_empty() {
        out.push_str("_No practices yet._\n");
    }
    for d in docs {
        out.push_str(&format!("## {}\n\n{}\n\n", d.name, d.body));
    }
    out
}

/// Markdown for `atlas://memories/recent`: the newest active memories, newest first.
fn memories_recent_markdown(memories: &[Memory]) -> String {
    let mut out = "# Recent memories\n\n".to_string();
    if memories.is_empty() {
        out.push_str("_No active memories yet._\n");
    }
    for m in memories {
        let tags = if m.tags.is_empty() { String::new() } else { format!(" ({})", m.tags.join(", ")) };
        out.push_str(&format!("- [{}] {}{}\n", m.kind, m.text, tags));
    }
    out
}

/// Markdown for `atlas://workflows/{name}`: the workflow's description plus its
/// action nodes, in place of the old `DocKind::Workflow` document body.
fn workflow_markdown(w: &Workflow) -> String {
    let mut out = format!("# {}\n\n{}\n\n## Actions\n\n", w.name, w.description);
    let mut any = false;
    for n in &w.graph.nodes {
        if let NodeData::Action { name, instructions, .. } = &n.data {
            any = true;
            out.push_str(&format!("- **{name}**: {instructions}\n"));
        }
    }
    if !any {
        out.push_str("_No actions yet._\n");
    }
    out
}

/// Every MCP tool name currently in `mcp.disabled_tools`, or the built-in default
/// (`project_connect`, `memory_review`) when the setting has never been written. Free
/// of `self` so `GET /api/v1/mcp/status` (`atlasd`) can compute the tools table's
/// `enabled` flags without a live session, from the same read [`AtlasMcp::disabled_tools`]
/// wraps for the router itself.
pub async fn disabled_tool_names<B: Backend>(backend: &B) -> atlas_core::Result<std::collections::HashSet<String>> {
    let settings = backend.get_settings().await?;
    let names = match settings.get("mcp.disabled_tools").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        None => atlas_core::settings::DEFAULT_DISABLED_MCP_TOOLS.iter().map(|s| s.to_string()).collect(),
    };
    Ok(names)
}

/// The three `atlas://projects/{name}/...` resources one project owns: its context,
/// its practices, and its board. Shared by `resources_for` (every project) and
/// `resources_for_project` (Task MCP-A's `GET /api/v1/projects/{id}/mcp`, one project).
fn project_resources(p: &Project) -> [Resource; 3] {
    let seg = utf8_percent_encode(&p.name, PROJECT_URI_SEGMENT).to_string();
    [
        Resource::new(format!("{PROJECTS}{seg}/context"), p.name.clone())
            .with_description(format!("Profile, practices, workflows and top memories for {}", p.root_path))
            .with_mime_type(JSON),
        Resource::new(format!("{PROJECTS}{seg}/practices"), format!("{} practices", p.name))
            .with_description(format!("Global and project practices for {}, as Markdown", p.root_path))
            .with_mime_type(MARKDOWN),
        Resource::new(format!("{PROJECTS}{seg}/board"), format!("{} board", p.name))
            .with_description(format!("Task board for {}, as Markdown", p.root_path))
            .with_mime_type(MARKDOWN),
    ]
}

/// Every resource `list_resources` would return, from the same backend calls the
/// router itself makes. Free of `self` so `GET /api/v1/mcp/status` can count them
/// without a live session.
pub async fn resources_for<B: Backend>(backend: &B) -> atlas_core::Result<Vec<Resource>> {
    let mut out = vec![];
    for a in backend.list_agents().await? {
        out.push(Resource::new(format!("{AGENTS}{}", a.name), a.name.clone()).with_description(a.description).with_mime_type(MARKDOWN));
    }
    for d in backend.list_docs(DocKind::Practice, None).await? {
        out.push(Resource::new(format!("{PRACTICES}{}", d.name), d.name.clone()).with_mime_type(MARKDOWN));
    }
    for w in backend.list_workflows(None).await? {
        out.push(Resource::new(format!("{WORKFLOWS}{}", w.name), w.name.clone()).with_description(w.description.clone()).with_mime_type(MARKDOWN));
    }
    for p in backend.list_projects().await? {
        out.extend(project_resources(&p));
    }
    out.push(Resource::new(format!("{PROJECTS}global/practices"), "Global practices".to_string())
        .with_description("Practices with no project, as Markdown")
        .with_mime_type(MARKDOWN));
    out.push(Resource::new(format!("{PROJECTS}global/board"), "Global board".to_string())
        .with_description("Tasks with no project, as Markdown")
        .with_mime_type(MARKDOWN));
    out.push(Resource::new(MEMORIES_RECENT, "Recent memories".to_string())
        .with_description(format!("The last {MEMORIES_RECENT_LIMIT} active memories, as Markdown"))
        .with_mime_type(MARKDOWN));
    // One listing rather than one entry per skill: a machine can carry hundreds of
    // them, and `atlas://skills/{id}` reads any one of them by the id this listing
    // gives.
    out.push(Resource::new(SKILLS, "Skills".to_string())
        .with_description("Every global skill: the SKILL.md folders Claude Code and Codex read, the ones installed plugins carry, and Atlas's own. Read atlas://skills/{id} for one skill's text.")
        .with_mime_type(JSON));
    Ok(out)
}

/// The `atlas://` resources that belong to one project: its own three (`context`,
/// `practices`, `board`), for Task MCP-A's `GET /api/v1/projects/{id}/mcp`. Unlike
/// `resources_for`, this never lists another project's resources or the global ones.
pub fn resources_for_project(p: &Project) -> Vec<Resource> {
    project_resources(p).into_iter().collect()
}

/// Every prompt `list_prompts` would return, from the same backend call the router
/// itself makes. Free of `self` so `GET /api/v1/mcp/status` can count them without a
/// live session.
pub async fn prompts_for<B: Backend>(backend: &B) -> atlas_core::Result<Vec<Prompt>> {
    let mut prompts = vec![
        Prompt::new(BOOTSTRAP_PROMPT, Some(BOOTSTRAP_DESCRIPTION), None),
        Prompt::new(HANDOFF_PROMPT, Some(HANDOFF_DESCRIPTION), None),
        Prompt::new(
            BOARD_WORKFLOW_PROMPT,
            Some("Work the task board: pick a ready task, claim it, move it through the stages, and comment as you go."),
            Some(vec![PromptArgument::new("project_root")
                .with_description("Absolute path to the project whose board to work, or \"global\" for the project-less board. Defaults to the root this server was started in.")
                .with_required(false)]),
        ),
    ];
    prompts.extend(backend.list_agents().await?.into_iter().map(|a| Prompt::new(a.name, Some(a.description), None)));
    Ok(prompts)
}

/// Resources and prompts are written by hand rather than by the static macros: both
/// lists come from the database, so they change while the server is running.
/// `list_tools` and `call_tool` are also written by hand (`#[tool_handler]` only
/// generates the ones a sibling method does not already provide) so `mcp.disabled_tools`
/// can gate the router's static list at both list and call time.
#[tool_handler]
impl<B: Backend> ServerHandler for AtlasMcp<B> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().enable_resources().enable_prompts().build())
            .with_server_info(Implementation::new("atlas", env!("CARGO_PKG_VERSION")))
            .with_instructions("Atlas is the shared memory and task board for all coding agents on this machine. Call memory_search at the start of a task and memory_remember when you learn a durable fact, make a decision, or notice a user preference. The atlas.bootstrap prompt walks through the rest.")
    }

    async fn list_tools(&self, _request: Option<PaginatedRequestParams>, context: RequestContext<RoleServer>) -> Result<ListToolsResult, McpError> {
        let disabled = self.disabled_tools().await?;
        let supports_cache_hints = context.protocol_version().is_some_and(|version| version >= ProtocolVersion::V_2026_07_28);
        // Plugin tools join the static list before the `disabled` filter, so
        // `mcp.disabled_tools` hides one exactly the way it hides a built-in. A backend
        // with no plugins behind it answers with an empty list, which is every backend
        // but the daemon's and the shim's.
        let mut tools = self.tool_router.list_all();
        tools.extend(self.backend.plugin_tools().await.map_err(err)?.iter().map(plugin_tool));
        let tools = tools.into_iter().filter(|t| !disabled.contains(t.name.as_ref())).collect();
        Ok(ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools,
            meta: None,
            next_cursor: None,
            ttl_ms: supports_cache_hints.then_some(0),
            cache_scope: supports_cache_hints.then_some(CacheScope::Public),
        })
    }

    async fn call_tool(&self, request: CallToolRequestParams, context: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError> {
        let disabled = self.disabled_tools().await?;
        if disabled.contains(request.name.as_ref()) {
            return Err(McpError::method_not_found::<CallToolRequestMethod>());
        }
        // Project-level gating (Task MCP-A), on top of the global list just checked.
        // `tools/list` cannot do this: it has no call in hand to resolve a project
        // from, so a project's own overrides only ever take effect here, at call time.
        // The project comes from the same precedence every other tool follows: this
        // call's own `project_root` argument, then `ATLAS_PROJECT_ROOT`, then the root
        // this server was started in; a call that resolves none is never gated by a
        // project. `resolve_project_for_gating`, not `resolve_project`, is what looks
        // it up: this check must never connect an unknown project, upsert a row, or
        // write an audit entry as a side effect of gating alone, for a tool that never
        // touched the project store before (`agent_list` names `project_root` in its
        // schema but never reads it). A resolution error (a malformed root, say) is not
        // this check's to report, so it is treated as "no project" and left for the
        // tool's own logic to surface if it needs one.
        let project_root = request.arguments.as_ref().and_then(|a| a.get("project_root")).and_then(|v| v.as_str()).map(PathBuf::from);
        let project = self.resolve_project_for_gating(project_root).await.unwrap_or(None);
        if let Some(p) = &project {
            if p.mcp_disabled_tools.iter().any(|t| t == request.name.as_ref()) {
                return Err(McpError::method_not_found::<CallToolRequestMethod>());
            }
        }
        if let Some(hook) = &self.on_tool_call {
            // The streamable HTTP transport injects the raw `http::request::Parts` into
            // the request's extensions; the stdio shim never does, so `session_id` is
            // `None` there and the hook (if any) knows not to treat the call as an HTTP
            // session.
            let session_id = context
                .extensions
                .get::<http::request::Parts>()
                .and_then(|p| p.headers.get("mcp-session-id"))
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            hook(session_id, context.client_info(), context.protocol_version(), project.as_ref().map(|p| p.id));
        }
        // A plugin tool is not in the static router, so it is dispatched here, after
        // both gates and the hook have run on its MCP name exactly as they would on a
        // built-in's.
        if let Some((underscored_id, name)) = parse_plugin_tool_name(request.name.as_ref()) {
            // The mapping to an MCP name loses which underscores in the plugin id were
            // dashes, so the real id comes back from the registered decls. When nothing
            // matches, the underscored id is forwarded as-is rather than refused here:
            // an app that has just disconnected has no decls left, and the backend is
            // the one that can tell "the plugin is not running" from "that plugin
            // declares no such tool".
            let decls = self.backend.plugin_tools().await.map_err(err)?;
            let plugin_id = decls.iter()
                .find(|d| d.name == name && d.plugin_id.replace('-', "_") == underscored_id)
                .map(|d| d.plugin_id.clone())
                .unwrap_or(underscored_id);
            let args = request.arguments.clone().map(serde_json::Value::Object).unwrap_or_else(|| serde_json::json!({}));
            let out = self.backend.call_plugin_tool(&plugin_id, &name, args, &self.source_tool).await.map_err(err)?;
            return Ok(json_result(&out)?.into());
        }
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    async fn list_resources(&self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(resources_for(&*self.backend).await.map_err(err)?))
    }

    async fn read_resource(&self, request: ReadResourceRequestParams, _context: RequestContext<RoleServer>) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri;
        let (text, mime) = if let Some(name) = uri.strip_prefix(AGENTS) {
            (claude_agent_md(&self.backend.get_agent(name).await.map_err(|e| resource_err(&uri, e))?), MARKDOWN)
        } else if let Some(name) = uri.strip_prefix(PRACTICES) {
            (self.backend.get_doc(DocKind::Practice, name).await.map_err(|e| resource_err(&uri, e))?.body, MARKDOWN)
        } else if let Some(name) = uri.strip_prefix(WORKFLOWS) {
            (workflow_markdown(&self.backend.get_workflow(name).await.map_err(|e| resource_err(&uri, e))?), MARKDOWN)
        } else if uri == SKILLS {
            let list = self.backend.list_skills(None).await.map_err(|e| resource_err(&uri, e))?;
            (serde_json::to_string_pretty(&list).map_err(|e| McpError::internal_error(e.to_string(), None))?, JSON)
        } else if let Some(rest) = uri.strip_prefix(SKILLS) {
            // A skill id carries a `:` and, for a plugin skill, slashes; a client that
            // percent-encoded either still reaches the same skill.
            let id = percent_decode_str(rest)
                .decode_utf8()
                .map_err(|e| McpError::invalid_params(format!("{uri}: skill id is not valid UTF-8: {e}"), None))?
                .into_owned();
            (self.backend.get_skill(None, &id).await.map_err(|e| resource_err(&uri, e))?.body, MARKDOWN)
        } else if uri == MEMORIES_RECENT {
            let mut memories = self.backend.list_memories(MemoryStatus::Active, None, MemoryScopeFilter::All).await.map_err(err)?;
            memories.truncate(MEMORIES_RECENT_LIMIT);
            (memories_recent_markdown(&memories), MARKDOWN)
        } else if let Some(rest) = uri.strip_prefix(PROJECTS) {
            let (seg, suffix) = rest.rsplit_once('/').ok_or_else(|| McpError::resource_not_found(format!("no Atlas resource at {uri}"), None))?;
            match suffix {
                "context" => {
                    let project_id = self.resolve_project_segment(seg).await.map_err(|e| resource_err(&uri, e))?
                        .ok_or_else(|| McpError::resource_not_found(format!("{uri}: no context resource for the global scope"), None))?;
                    let ctx = self.project_context_readonly(project_id).await.map_err(|e| resource_err(&uri, e))?;
                    (serde_json::to_string_pretty(&ctx).map_err(|e| McpError::internal_error(e.to_string(), None))?, JSON)
                }
                "practices" => {
                    let project_id = self.resolve_project_segment(seg).await.map_err(|e| resource_err(&uri, e))?;
                    let docs = self.backend.list_docs(DocKind::Practice, project_id).await.map_err(|e| resource_err(&uri, e))?;
                    let docs = match project_id { Some(_) => docs, None => docs.into_iter().filter(|d| d.project_id.is_none()).collect() };
                    let name = match project_id {
                        Some(id) => Some(self.backend.get_project(id).await.map_err(|e| resource_err(&uri, e))?.name),
                        None => None,
                    };
                    (practices_markdown(name.as_deref(), &docs), MARKDOWN)
                }
                "board" => {
                    let project_id = self.resolve_project_segment(seg).await.map_err(|e| resource_err(&uri, e))?;
                    let stages = self.backend.board_stages(project_id).await.map_err(|e| resource_err(&uri, e))?.stages;
                    let filter = TaskFilter { project_id, include_done: true, ..Default::default() };
                    let tasks = self.backend.list_tasks(filter).await.map_err(|e| resource_err(&uri, e))?;
                    (render_board_markdown(&stages, &tasks), MARKDOWN)
                }
                _ => return Err(McpError::resource_not_found(format!("no Atlas resource at {uri}"), None)),
            }
        } else {
            return Err(McpError::resource_not_found(format!("no Atlas resource at {uri}"), None));
        };
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri).with_mime_type(mime)]).into())
    }

    async fn list_prompts(&self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompts_for(&*self.backend).await.map_err(err)?))
    }

    async fn get_prompt(&self, request: GetPromptRequestParams, _context: RequestContext<RoleServer>) -> Result<GetPromptResponse, McpError> {
        if request.name == BOOTSTRAP_PROMPT {
            let mut result = GetPromptResult::new(vec![PromptMessage::new_text(Role::User, BOOTSTRAP_TEXT.to_string())]);
            result.description = Some(BOOTSTRAP_DESCRIPTION.into());
            return Ok(result.into());
        }
        if request.name == HANDOFF_PROMPT {
            let mut result = GetPromptResult::new(vec![PromptMessage::new_text(Role::User, HANDOFF_TEXT.to_string())]);
            result.description = Some(HANDOFF_DESCRIPTION.into());
            return Ok(result.into());
        }
        if request.name == BOARD_WORKFLOW_PROMPT {
            let project_root = request.arguments.as_ref().and_then(|a| a.get("project_root")).and_then(|v| v.as_str()).map(PathBuf::from);
            let (project_id, _) = self.board_project_id(project_root).await?;
            let stages = self.backend.board_stages(project_id).await.map_err(err)?.stages;
            let names = stages.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
            let text = format!("{BOARD_WORKFLOW_TEXT}\n\nStages for this project: {names}");
            let mut result = GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]);
            result.description = Some("Work the task board: claim ready work, move it through the stages, and comment as you go.".into());
            return Ok(result.into());
        }
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
    use rmcp::ServiceExt;

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
        for n in ["memory_remember", "memory_search", "memory_list", "memory_forget", "memory_review",
                  "status", "project_context", "project_connect", "project_list", "project_get",
                  "agent_list", "agent_get", "agent_save", "practice_list", "practice_get",
                  "workflow_list", "workflow_get", "ingest_transcript", "workflow_run", "workflow_status"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }

    /// [`TOOL_TABLE`] is a hand-maintained const, kept honest here rather than by
    /// construction: its names must be exactly the tool router's names (nothing
    /// listed that does not exist, nothing that exists left out of the status table),
    /// and it must be exactly the list `atlas-core::settings::MCP_TOOL_NAMES` validates
    /// `mcp.disabled_tools` against, so a name that would validate but never gate
    /// anything (or vice versa) fails a test rather than shipping quietly.
    #[test]
    fn tool_table_matches_the_router_and_the_settings_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let b = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(b);
        let mut router_names: Vec<String> = s.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
        router_names.sort();
        let mut table_names: Vec<String> = TOOL_TABLE.iter().map(|m| m.name.to_string()).collect();
        table_names.sort();
        assert_eq!(router_names, table_names, "TOOL_TABLE and the tool router have drifted apart");

        let mut allowlist: Vec<String> = atlas_core::settings::MCP_TOOL_NAMES.iter().map(|s| s.to_string()).collect();
        allowlist.sort();
        assert_eq!(table_names, allowlist, "TOOL_TABLE and atlas_core::settings::MCP_TOOL_NAMES have drifted apart");

        for meta in TOOL_TABLE {
            let tool = router_names_to_tools(&s).into_iter().find(|t| t.name == meta.name).unwrap();
            let actual = tool.description.as_deref().unwrap_or_default();
            assert_eq!(actual, meta.description, "{} description drifted from its #[tool] attribute", meta.name);
        }
    }

    fn router_names_to_tools<B: Backend>(s: &AtlasMcp<B>) -> Vec<Tool> {
        s.tool_router.list_all()
    }

    /// `mcp.disabled_tools` gates both `tools/list` and `tools/call`: with the setting
    /// unset, the two tools disabled by default (`project_connect`, `memory_review`)
    /// are absent from the list and a call to either answers method-not-found; writing
    /// an empty list through the backend re-enables both on the very next request of
    /// the same, still-connected session, proving the "takes effect on the next call
    /// rather than after a restart" claim rather than only that a fresh session picks
    /// up the setting.
    #[tokio::test]
    async fn disabled_tools_are_hidden_from_list_and_refused_on_call() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend.clone());

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        let handle = tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let tools = client.list_tools(None).await.unwrap();
        let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(!names.contains(&"project_connect"), "{names:?}");
        assert!(!names.contains(&"memory_review"), "{names:?}");
        assert!(names.contains(&"memory_remember"), "{names:?}");

        let call = client.call_tool(CallToolRequestParams::new("project_connect")).await.unwrap_err();
        assert!(matches!(&call, rmcp::service::ServiceError::McpError(e) if e.code == rmcp::model::ErrorCode::METHOD_NOT_FOUND), "{call:?}");

        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!([]))]), "t").await.unwrap();
        let tools = client.list_tools(None).await.unwrap();
        let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"project_connect"), "{names:?}");
        assert!(names.contains(&"memory_review"), "{names:?}");

        client.cancel().await.unwrap();
        handle.await.unwrap();
    }

    /// Task MCP-A: a project's own `mcp_disabled_tools` refuses a tool only for calls
    /// that resolve to that project, on top of (never instead of) the global list.
    /// `tools/list` stays global, since it has no call in hand to resolve a project
    /// from; the resolved project's gating is checked on every `call_tool`.
    #[tokio::test]
    async fn project_disabled_tools_are_refused_for_that_project_and_allowed_for_another() {
        let home = tempfile::tempdir().unwrap();
        let repo_a = tempfile::tempdir().unwrap();
        let repo_b = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let project_a = backend.connect_project(repo_a.path().to_path_buf(), "t").await.unwrap();
        let _project_b = backend.connect_project(repo_b.path().to_path_buf(), "t").await.unwrap();
        backend
            .update_project(project_a.id, ProjectPatch { mcp_disabled_tools: Some(vec!["memory_list".into()]), ..Default::default() }, "t")
            .await
            .unwrap();

        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);
        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        let handle = tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        // `tools/list` cannot know which project is asking, so it still lists a tool a
        // project has disabled.
        let tools = client.list_tools(None).await.unwrap();
        assert!(tools.tools.iter().any(|t| t.name == "memory_list"));

        let mut args_a = JsonObject::new();
        args_a.insert("project_root".into(), serde_json::json!(repo_a.path().to_string_lossy()));
        let call = client.call_tool(CallToolRequestParams::new("memory_list").with_arguments(args_a)).await.unwrap_err();
        assert!(matches!(&call, rmcp::service::ServiceError::McpError(e) if e.code == rmcp::model::ErrorCode::METHOD_NOT_FOUND), "{call:?}");

        // The same tool, called against the project that never disabled it, still works.
        let mut args_b = JsonObject::new();
        args_b.insert("project_root".into(), serde_json::json!(repo_b.path().to_string_lossy()));
        client.call_tool(CallToolRequestParams::new("memory_list").with_arguments(args_b)).await.unwrap();

        client.cancel().await.unwrap();
        handle.await.unwrap();
    }

    /// Fix round 1: the gating check itself must never connect an unknown project or
    /// write an audit row, only the tool's own logic may do that. `agent_list` names
    /// `project_root` in its schema (`TOOL_TABLE`) but its handler never reads it
    /// (`Parameters(_a)`), so it never touched the project store before this task; it
    /// must not start now just because gating resolves a project on every call. True
    /// whether `project_connect` is disabled (the default) or enabled: `resolve_project`
    /// would connect (and audit) an uncached root once `project_connect` is enabled, so
    /// `call_tool`'s gating step must go through `resolve_project_for_gating` instead,
    /// which always takes the read-only "match a known project" path.
    #[tokio::test]
    async fn gating_a_read_tool_connects_nothing_and_writes_no_audit_row() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        let handle = tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let mut args = JsonObject::new();
        args.insert("project_root".into(), serde_json::json!(repo.path().to_string_lossy()));

        // project_connect disabled (the default).
        client.call_tool(CallToolRequestParams::new("agent_list").with_arguments(args.clone())).await.unwrap();
        assert!(backend.list_projects().await.unwrap().is_empty(), "an unknown project must not be connected by gating alone");
        assert_eq!(project_writes(&backend), 0, "gating must write no audit row");

        // project_connect enabled: the branch that would make `resolve_project` connect
        // an uncached root is now live; gating must still avoid it.
        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!([]))]), "t").await.unwrap();
        client.call_tool(CallToolRequestParams::new("agent_list").with_arguments(args)).await.unwrap();
        assert!(
            backend.list_projects().await.unwrap().is_empty(),
            "an unknown project must not be connected by gating alone, even with project_connect enabled"
        );
        assert_eq!(project_writes(&backend), 0, "gating must write no audit row, even with project_connect enabled");

        client.cancel().await.unwrap();
        handle.await.unwrap();
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
            .ingest_transcript(Parameters(IngestTranscriptArgs { text: "user: we use bun".into(), agent: None, project_root: None }))
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
            async move { s.ingest_transcript(Parameters(IngestTranscriptArgs { text, agent: None, project_root: None })).await }
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

    /// A server started in an already-connected project scopes memories to it without
    /// being told, and does so from a cache: the second recall must not touch the
    /// projects table. `project_connect` is disabled by default, so the project has to
    /// be connected ahead of time here; the `disabled_project_connect_*` tests below
    /// cover the seeded root of a project Atlas has never seen.
    #[tokio::test]
    async fn seeded_root_scopes_memories_and_is_resolved_once() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let connected = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone())
            .with_env_project_root(false)
            .with_project_root(repo.path().to_path_buf());

        // No scope, no project_id: the seeded root supplies both.
        s.memory_remember(Parameters(MemoryRememberArgs {
            text: "the fixture uses duckdb".into(), kind: None, tags: None, scope: None,
            project_id: None, project_root: None, source_agent: None,
        })).await.unwrap();

        let stored = backend.list_memories(MemoryStatus::Active, None, MemoryScopeFilter::All).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].scope, MemoryScope::Project);
        assert_eq!(stored[0].project_id, Some(connected.id), "the seeded root should have scoped the memory to the connected project");

        let projects = backend.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, connected.id);

        // Recall with no project_id finds it through the same resolution.
        let hit = s.memory_search(Parameters(MemorySearchArgs {
            query: "duckdb".into(), limit: None, scope: None, kinds: None, tags: None,
            project_id: None, project_root: None,
        })).await.unwrap();
        assert!(text_of(&hit).contains("the fixture uses duckdb"), "{}", text_of(&hit));

        // Everything from here on is a read, so the audit trail must stand still.
        let before = project_writes(&backend);
        s.memory_search(Parameters(MemorySearchArgs {
            query: "duckdb".into(), limit: None, scope: None, kinds: None, tags: None,
            project_id: None, project_root: None,
        })).await.unwrap();
        s.practice_list(Parameters(PracticeListArgs { project_root: None, tags: None })).await.unwrap();
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

        s.memory_remember(Parameters(MemoryRememberArgs {
            text: "bun is the runtime".into(), kind: None, tags: None, scope: None,
            project_id: None, project_root: None, source_agent: None,
        })).await.unwrap();
        let stored = backend.list_memories(MemoryStatus::Active, None, MemoryScopeFilter::All).await.unwrap();
        assert_eq!(stored[0].scope, MemoryScope::Global);
        assert!(stored[0].project_id.is_none());
        assert!(backend.list_projects().await.unwrap().is_empty(), "a global remember connected a project");

        // A practice belonging to some other project must not show up in a global listing.
        let project = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "scoped".into(), body: "b".into(), tags: vec![], project_id: Some(project.id) }, "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "everywhere".into(), body: "b".into(), tags: vec![], project_id: None }, "test").await.unwrap();
        let listed = s.practice_list(Parameters(PracticeListArgs { project_root: None, tags: None })).await.unwrap();
        let listed = text_of(&listed);
        assert!(listed.contains("everywhere"), "{listed}");
        assert!(!listed.contains("scoped"), "a global listing leaked another project's practice: {listed}");
    }

    /// `practice_list`'s `tags` filter keeps only practices carrying at least one of
    /// the given tags.
    #[tokio::test]
    async fn practice_list_narrows_by_tag() {
        let home = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        backend.save_doc(DocKind::Practice, NewDoc { name: "rust-style".into(), body: "b".into(), tags: vec!["rust".into()], project_id: None }, "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "svelte-style".into(), body: "b".into(), tags: vec!["svelte".into()], project_id: None }, "test").await.unwrap();
        let s = AtlasMcp::new(backend).with_env_project_root(false);

        let listed = s.practice_list(Parameters(PracticeListArgs { project_root: None, tags: Some(vec!["rust".into()]) })).await.unwrap();
        let listed = text_of(&listed);
        assert!(listed.contains("rust-style"), "{listed}");
        assert!(!listed.contains("svelte-style"), "{listed}");
    }

    /// `memory_list` narrows by kind, tag and since without a search query, newest
    /// first, capped by `limit`.
    #[tokio::test]
    async fn memory_list_filters_by_kind_tag_and_since() {
        let home = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        s.memory_remember(Parameters(MemoryRememberArgs { text: "a fact".into(), kind: Some("fact".into()), tags: Some(vec!["x".into()]), scope: None, project_id: None, project_root: None, source_agent: None })).await.unwrap();
        s.memory_remember(Parameters(MemoryRememberArgs { text: "a decision".into(), kind: Some("decision".into()), tags: None, scope: None, project_id: None, project_root: None, source_agent: None })).await.unwrap();

        let by_kind = s.memory_list(Parameters(MemoryListArgs { kinds: Some(vec!["decision".into()]), tags: None, project_id: None, project_root: None, since: None, limit: None })).await.unwrap();
        let by_kind = text_of(&by_kind);
        assert!(by_kind.contains("a decision"), "{by_kind}");
        assert!(!by_kind.contains("a fact"), "{by_kind}");

        let by_tag = s.memory_list(Parameters(MemoryListArgs { kinds: None, tags: Some(vec!["x".into()]), project_id: None, project_root: None, since: None, limit: None })).await.unwrap();
        let by_tag = text_of(&by_tag);
        assert!(by_tag.contains("a fact"), "{by_tag}");
        assert!(!by_tag.contains("a decision"), "{by_tag}");

        let future = Utc::now() + chrono::Duration::days(1);
        let by_since = s.memory_list(Parameters(MemoryListArgs { kinds: None, tags: None, project_id: None, project_root: None, since: Some(future), limit: None })).await.unwrap();
        let by_since: Vec<Memory> = serde_json::from_str(&text_of(&by_since)).unwrap();
        assert!(by_since.is_empty(), "{by_since:?}");
    }

    /// `memory_review` accepts a pending memory into active use, or rejects it; an
    /// unrecognised decision is invalid_params.
    #[tokio::test]
    async fn memory_review_accepts_or_rejects_a_pending_memory() {
        let home = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let m = backend.remember(
            NewMemory { scope: MemoryScope::Global, project_id: None, kind: MemoryKind::Insight, text: "pending one".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 0.4, status: MemoryStatus::Pending },
            "t",
        ).await.unwrap();
        let s = AtlasMcp::new(backend.clone());

        let bad = s.memory_review(Parameters(MemoryReviewArgs { id: m.id, decision: "maybe".into() })).await.unwrap_err();
        assert_eq!(bad.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{bad:?}");

        let accepted = s.memory_review(Parameters(MemoryReviewArgs { id: m.id, decision: "accept".into() })).await.unwrap();
        let accepted: Memory = serde_json::from_str(&text_of(&accepted)).unwrap();
        assert_eq!(accepted.status, MemoryStatus::Active);

        let rejected = s.memory_review(Parameters(MemoryReviewArgs { id: m.id, decision: "reject".into() })).await.unwrap();
        let rejected: Memory = serde_json::from_str(&text_of(&rejected)).unwrap();
        assert_eq!(rejected.status, MemoryStatus::Rejected);
    }

    /// `project_list` and `project_get` round trip a connected project by name.
    #[tokio::test]
    async fn project_list_and_get_round_trip_by_name() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let connected = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend);

        let listed = s.project_list().await.unwrap();
        let listed: Vec<Project> = serde_json::from_str(&text_of(&listed)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, connected.id);

        let got = s.project_get(Parameters(NameArgs { name: connected.name.clone() })).await.unwrap();
        let got: Project = serde_json::from_str(&text_of(&got)).unwrap();
        assert_eq!(got.id, connected.id);

        let missing = s.project_get(Parameters(NameArgs { name: "nope".into() })).await.unwrap_err();
        assert_eq!(missing.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{missing:?}");
    }

    #[test]
    fn tool_list_covers_the_board_tools_with_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let b = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(b);
        let tools = s.tool_router.list_all();
        let board = ["task_list", "task_get", "task_create", "task_update", "task_move", "task_comment", "task_claim", "task_block", "board_stages"];
        assert_eq!(board.len(), 9);
        for n in board {
            let tool = tools.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("missing {n}"));
            assert!(!tool.description.as_deref().unwrap_or_default().is_empty(), "{n} has no description");
        }
        // Deletion stays out of the agent surface.
        assert!(!tools.iter().any(|t| t.name == "task_delete"));
    }

    /// `task_block` replaces the whole blocker list, so an agent can record a
    /// dependency it finds mid-work and clear it again when the blocker is gone.
    #[tokio::test]
    async fn task_block_sets_and_clears_the_blocker_list() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        // project_connect is disabled by default, so the seeded root has to already be
        // a known project for the board tools to resolve it.
        backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_source_tool("test").with_env_project_root(false).with_project_root(repo.path().to_path_buf());

        let mut keys = Vec::new();
        for title in ["waiter", "blocker"] {
            let out = s
                .task_create(Parameters(TaskCreateArgs {
                    title: title.into(), description: None, kind: None, priority: None,
                    labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
                }))
                .await
                .unwrap();
            let t: atlas_core::models::Task = serde_json::from_str(&text_of(&out)).unwrap();
            keys.push(t.key);
        }
        let (waiter, blocker) = (keys[0].clone(), keys[1].clone());

        let out = s
            .task_block(Parameters(TaskBlockArgs { key: waiter.clone(), blocked_by: vec![blocker.clone()], agent: Some("agent-x".into()) }))
            .await
            .unwrap();
        let t: atlas_core::models::Task = serde_json::from_str(&text_of(&out)).unwrap();
        assert_eq!(t.blocked_by, vec![blocker.clone()]);
        assert_eq!(t.open_blockers, 1);
        assert!(!t.ready);

        let ready = s
            .task_list(Parameters(TaskListArgs {
                project_root: None, stage: None, assignee: None, ready: Some(true),
                query: None, include_done: None, agent: None,
            }))
            .await
            .unwrap();
        let ready: Vec<atlas_core::models::Task> = serde_json::from_str(&text_of(&ready)).unwrap();
        assert_eq!(ready.iter().map(|t| t.key.clone()).collect::<Vec<_>>(), vec![blocker.clone()]);

        let out = s.task_block(Parameters(TaskBlockArgs { key: waiter.clone(), blocked_by: vec![], agent: None })).await.unwrap();
        let t: atlas_core::models::Task = serde_json::from_str(&text_of(&out)).unwrap();
        assert!(t.blocked_by.is_empty());
        assert_eq!(t.open_blockers, 0);
        assert!(t.ready);
    }

    /// Create, claim, move, comment, and confirm a task blocked by an open one is
    /// excluded from `ready=true`; also confirms the claim/move/comment events are
    /// all recorded under `<source_tool>/<agent>` and that moving to an unknown
    /// stage names the valid ones.
    #[tokio::test]
    async fn board_round_trip_through_the_tools() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        // project_connect is disabled by default, so the seeded root has to already be
        // a known project for the board tools to resolve it.
        backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone())
            .with_source_tool("test")
            .with_env_project_root(false)
            .with_project_root(repo.path().to_path_buf());

        let created = s
            .task_create(Parameters(TaskCreateArgs {
                title: "first task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
            }))
            .await
            .unwrap();
        let task: atlas_core::models::Task = serde_json::from_str(&text_of(&created)).unwrap();
        let key = task.key.clone();
        assert!(key.starts_with(task.key.split('-').next().unwrap()));

        let claimed = s.task_claim(Parameters(TaskClaimArgs { key: key.clone(), force: None, agent: Some("agent-x".into()) })).await.unwrap();
        let claimed: atlas_core::models::Task = serde_json::from_str(&text_of(&claimed)).unwrap();
        assert_eq!(claimed.assignee.as_deref(), Some("test/agent-x"));
        assert_eq!(claimed.stage, "In Progress");

        let moved = s
            .task_move(Parameters(TaskMoveArgs { key: key.clone(), stage: "Testing".into(), expected_updated_at: None, agent: Some("agent-x".into()) }))
            .await
            .unwrap();
        let moved: atlas_core::models::Task = serde_json::from_str(&text_of(&moved)).unwrap();
        assert_eq!(moved.stage, "Testing");

        s.task_comment(Parameters(TaskCommentArgs { key: key.clone(), body: "looks good".into(), agent: Some("agent-x".into()) })).await.unwrap();

        // A second task, blocked by the first (which is not in a done stage), must
        // not show up in a ready=true listing.
        let second = s
            .task_create(Parameters(TaskCreateArgs {
                title: "second task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: Some(vec![key.clone()]), project_root: None, agent: None,
            }))
            .await
            .unwrap();
        let second: atlas_core::models::Task = serde_json::from_str(&text_of(&second)).unwrap();

        let ready = s
            .task_list(Parameters(TaskListArgs { project_root: None, stage: None, assignee: None, ready: Some(true), query: None, include_done: None, agent: None }))
            .await
            .unwrap();
        let ready_text = text_of(&ready);
        assert!(!ready_text.contains(&second.key), "{ready_text}");

        let detail = s.task_get(Parameters(TaskKeyArgs { key: key.clone(), agent: None })).await.unwrap();
        let detail: TaskDetail = serde_json::from_str(&text_of(&detail)).unwrap();
        // Claim emits both an `assigned` event and, because the task started in the
        // board's first stage, an implicit `moved` event; the explicit task_move adds
        // a second `moved`; the comment adds `commented`: four, in that order.
        let agent_x_kinds: Vec<&str> = detail.events.iter().filter(|e| e.actor == "test/agent-x").map(|e| e.kind.as_str()).collect();
        assert_eq!(agent_x_kinds, vec!["assigned", "moved", "moved", "commented"], "{:?}", detail.events);

        let bad = s
            .task_move(Parameters(TaskMoveArgs { key: key.clone(), stage: "Nope".into(), expected_updated_at: None, agent: None }))
            .await
            .unwrap_err();
        assert_eq!(bad.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(bad.message.contains("Backlog"), "{bad:?}");
    }

    /// With no `path`, `framework_docs` lists the frameworks a project has (filtered
    /// by `kind` when one is given); with `kind` and `path` together it fetches that
    /// document's text.
    #[tokio::test]
    async fn framework_docs_round_trip() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("docs/superpowers/plans")).unwrap();
        std::fs::write(
            repo.path().join("docs/superpowers/plans/2026-01-01-fixture.md"),
            "# Fixture plan\n\n### Task 1: Do the thing\n\n- [ ] do it\n",
        )
        .unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend).with_env_project_root(false).with_project_root(repo.path().to_path_buf());

        let listing = s.framework_docs(Parameters(FrameworkDocsArgs { project_root: None, kind: None, path: None })).await.unwrap();
        let listings: Vec<FrameworkListing> = serde_json::from_str(&text_of(&listing)).unwrap();
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].inventory.kind, FrameworkKind::Superpowers);
        let doc = listings[0].documents.first().expect("the fixture plan should be listed");
        assert_eq!(doc.path, "docs/superpowers/plans/2026-01-01-fixture.md");

        // A kind filter that matches nothing empties the listing without erroring.
        let filtered_out = s
            .framework_docs(Parameters(FrameworkDocsArgs { project_root: None, kind: Some(FrameworkKind::Gsd), path: None }))
            .await
            .unwrap();
        let filtered_out: Vec<FrameworkListing> = serde_json::from_str(&text_of(&filtered_out)).unwrap();
        assert!(filtered_out.is_empty());

        let read = s
            .framework_docs(Parameters(FrameworkDocsArgs { project_root: None, kind: Some(FrameworkKind::Superpowers), path: Some(doc.path.clone()) }))
            .await
            .unwrap();
        let read: serde_json::Value = serde_json::from_str(&text_of(&read)).unwrap();
        assert_eq!(read["content"].as_str().unwrap(), "# Fixture plan\n\n### Task 1: Do the thing\n\n- [ ] do it\n");

        // `path` without `kind` is a caller mistake, not an ambiguity to guess at.
        let bad = s.framework_docs(Parameters(FrameworkDocsArgs { project_root: None, kind: None, path: Some(doc.path.clone()) })).await.unwrap_err();
        assert_eq!(bad.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    /// A board tool needs a project to file a key under: with no root resolvable it
    /// errors, and the literal `project_root: "global"` opts into the project-less
    /// board explicitly.
    #[tokio::test]
    async fn board_tools_require_a_connected_project_unless_global_is_named() {
        let home = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend).with_env_project_root(false);

        let err = s
            .task_create(Parameters(TaskCreateArgs {
                title: "t".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("no project"), "{err:?}");

        let global = s
            .task_create(Parameters(TaskCreateArgs {
                title: "global task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: Some("global".into()), agent: None,
            }))
            .await
            .unwrap();
        let task: atlas_core::models::Task = serde_json::from_str(&text_of(&global)).unwrap();
        assert!(task.key.starts_with("ATLAS-"), "{}", task.key);
    }

    /// `task_create` with `project_root: "global"` files a project-less task, and
    /// `task_list` with the same `"global"` must be able to see it again (M1): before
    /// the fix, `task_list("global")` widened to every project instead of narrowing to
    /// the project-less board `task_create` had just filed onto. A project's own
    /// `task_list` (no `project_root` argument, run from inside a connected project)
    /// must not include the global task.
    #[tokio::test]
    async fn task_list_global_round_trips_with_task_create_global() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false).with_project_root(repo.path().to_path_buf());

        let global = s
            .task_create(Parameters(TaskCreateArgs {
                title: "global task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: Some("global".into()), agent: None,
            }))
            .await
            .unwrap();
        let global_task: atlas_core::models::Task = serde_json::from_str(&text_of(&global)).unwrap();

        let project_task = s
            .task_create(Parameters(TaskCreateArgs {
                title: "project task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
            }))
            .await
            .unwrap();
        let project_task: atlas_core::models::Task = serde_json::from_str(&text_of(&project_task)).unwrap();

        let listed_global = s
            .task_list(Parameters(TaskListArgs {
                project_root: Some("global".into()), stage: None, assignee: None, ready: None,
                query: None, include_done: None, agent: None,
            }))
            .await
            .unwrap();
        let listed_global: Vec<atlas_core::models::Task> = serde_json::from_str(&text_of(&listed_global)).unwrap();
        let listed_global_keys: Vec<&str> = listed_global.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(listed_global_keys, vec![global_task.key.as_str()], "{listed_global_keys:?}");

        let listed_project = s
            .task_list(Parameters(TaskListArgs {
                project_root: None, stage: None, assignee: None, ready: None,
                query: None, include_done: None, agent: None,
            }))
            .await
            .unwrap();
        let listed_project: Vec<atlas_core::models::Task> = serde_json::from_str(&text_of(&listed_project)).unwrap();
        let listed_project_keys: Vec<&str> = listed_project.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(listed_project_keys, vec![project_task.key.as_str()], "{listed_project_keys:?}");
        assert!(!listed_project_keys.contains(&global_task.key.as_str()), "{listed_project_keys:?}");
    }

    /// A full client/server round trip over an in-memory duplex: the board resource
    /// (now at `atlas://projects/{name}/board`) renders Markdown containing the
    /// task's key, and the atlas.board_workflow prompt is listed and names the
    /// project's stages.
    #[tokio::test]
    async fn board_resource_and_prompt_are_served_over_the_wire() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        // project_connect is disabled by default, so the seeded root has to already be
        // a known project for the board tools to resolve it.
        backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false).with_project_root(repo.path().to_path_buf());

        let created = s
            .task_create(Parameters(TaskCreateArgs {
                title: "wire task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
            }))
            .await
            .unwrap();
        let task: atlas_core::models::Task = serde_json::from_str(&text_of(&created)).unwrap();
        let project = backend.list_projects().await.unwrap().into_iter().next().unwrap();

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let uri = format!("atlas://projects/{}/board", percent_encoding::utf8_percent_encode(&project.name, PROJECT_URI_SEGMENT));
        let resource = client.read_resource(ReadResourceRequestParams::new(uri)).await.unwrap();
        let text: String = resource
            .contents
            .iter()
            .filter_map(|c| match c {
                ResourceContents::TextResourceContents { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(text.contains(&task.key), "{text}");

        let prompts = client.list_prompts(None).await.unwrap();
        assert!(prompts.prompts.iter().any(|p| p.name == BOARD_WORKFLOW_PROMPT), "{:?}", prompts.prompts);
        assert!(prompts.prompts.iter().any(|p| p.name == BOOTSTRAP_PROMPT), "{:?}", prompts.prompts);
        assert!(prompts.prompts.iter().any(|p| p.name == HANDOFF_PROMPT), "{:?}", prompts.prompts);

        let prompt = client.get_prompt(GetPromptRequestParams::new(BOARD_WORKFLOW_PROMPT)).await.unwrap();
        let prompt_text: String = prompt.messages.iter().filter_map(|m| m.content.as_text().map(|t| t.text.clone())).collect();
        assert!(prompt_text.contains("Testing"), "{prompt_text}");

        let bootstrap = client.get_prompt(GetPromptRequestParams::new(BOOTSTRAP_PROMPT)).await.unwrap();
        let bootstrap_text: String = bootstrap.messages.iter().filter_map(|m| m.content.as_text().map(|t| t.text.clone())).collect();
        assert!(bootstrap_text.contains("memory_search") && bootstrap_text.contains("task_list"), "{bootstrap_text}");

        let handoff = client.get_prompt(GetPromptRequestParams::new(HANDOFF_PROMPT)).await.unwrap();
        let handoff_text: String = handoff.messages.iter().filter_map(|m| m.content.as_text().map(|t| t.text.clone())).collect();
        assert!(handoff_text.contains("memory_remember") && handoff_text.contains("task_create"), "{handoff_text}");

        client.cancel().await.unwrap();
    }

    /// `atlas.board_workflow` resolves `project_root: "global"` through the same
    /// `board_project_id()` helper the tools use: it must answer with the global
    /// board's stages and must not connect (or create) a project for the literal
    /// "global".
    #[tokio::test]
    async fn board_workflow_prompt_resolves_global_like_the_tools_do() {
        let home = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let mut params = GetPromptRequestParams::new(BOARD_WORKFLOW_PROMPT);
        let mut args = serde_json::Map::new();
        args.insert("project_root".into(), serde_json::Value::String("global".into()));
        params.arguments = Some(args);
        let prompt = client.get_prompt(params).await.unwrap();
        let text: String = prompt.messages.iter().filter_map(|m| m.content.as_text().map(|t| t.text.clone())).collect();
        assert!(text.contains("Backlog") && text.contains("Done"), "{text}");

        client.cancel().await.unwrap();
        assert!(backend.list_projects().await.unwrap().is_empty(), "the global board prompt should not have connected a project");
    }

    /// With `project_connect` disabled, any other tool given a `project_root` Atlas
    /// has never seen must not register it: `resolve_project` matches only known
    /// projects and fails with the disabled-tool sentence instead of connecting.
    #[tokio::test]
    async fn disabled_project_connect_refuses_an_unknown_root_from_another_tool() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["project_connect"]))]), "t").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        let err = s.memory_search(Parameters(MemorySearchArgs {
            query: "x".into(), limit: None, scope: None, kinds: None, tags: None,
            project_id: None, project_root: Some(repo.path().to_path_buf()),
        })).await.unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{err:?}");
        assert!(err.message.contains(PROJECT_CONNECT_DISABLED), "{err:?}");
        assert!(backend.list_projects().await.unwrap().is_empty(), "a disabled project_connect should not have registered the project");
    }

    /// With `project_connect` disabled, reading `atlas://projects/{name}/context` for a
    /// project Atlas has never connected answers resource-not-found rather than
    /// connecting it. The resource is already read-only regardless of the gate; this
    /// proves it stays that way (and stays refused) with the tool off too.
    #[tokio::test]
    async fn disabled_project_connect_context_resource_does_not_connect_an_unknown_project() {
        let home = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["project_connect"]))]), "t").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let err = client.read_resource(ReadResourceRequestParams::new(format!("{PROJECTS}nope/context"))).await.unwrap_err();
        assert!(matches!(&err, rmcp::service::ServiceError::McpError(e) if e.code == rmcp::model::ErrorCode::RESOURCE_NOT_FOUND), "{err:?}");

        client.cancel().await.unwrap();
        assert!(backend.list_projects().await.unwrap().is_empty(), "a disabled project_connect should not have registered the project via the resource");
    }

    /// With `project_connect` disabled, `atlas.board_workflow` given a `project_root`
    /// Atlas has never connected must not register it: it resolves through the same
    /// `board_project_id()` -> `resolve_project()` path every board tool uses.
    #[tokio::test]
    async fn disabled_project_connect_board_workflow_prompt_does_not_connect_an_unknown_root() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["project_connect"]))]), "t").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let mut params = GetPromptRequestParams::new(BOARD_WORKFLOW_PROMPT);
        let mut args = serde_json::Map::new();
        args.insert("project_root".into(), serde_json::Value::String(repo.path().display().to_string()));
        params.arguments = Some(args);
        let err = client.get_prompt(params).await.unwrap_err();
        assert!(matches!(&err, rmcp::service::ServiceError::McpError(e) if e.code == rmcp::model::ErrorCode::INVALID_PARAMS), "{err:?}");

        client.cancel().await.unwrap();
        assert!(backend.list_projects().await.unwrap().is_empty(), "a disabled project_connect should not have registered the project via the prompt");
    }

    /// With `project_connect` disabled, a project Atlas already knows still resolves:
    /// the gate refuses only an unknown root, it does not turn every tool that takes a
    /// `project_root` into a no-op.
    #[tokio::test]
    async fn disabled_project_connect_still_resolves_a_known_project() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let connected = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["project_connect"]))]), "t").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        s.memory_remember(Parameters(MemoryRememberArgs {
            text: "known project fact".into(), kind: None, tags: None, scope: None,
            project_id: None, project_root: Some(repo.path().to_path_buf()), source_agent: None,
        })).await.unwrap();

        let stored = backend.list_memories(MemoryStatus::Active, None, MemoryScopeFilter::All).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].project_id, Some(connected.id), "a known project should still resolve with project_connect disabled");
    }

    /// `atlas://projects/{name}/context` (the URI `resources_for` now lists, matching
    /// `practices` and `board`) accepts either the project's name or its id, and
    /// neither read connects or re-profiles the project.
    #[tokio::test]
    async fn context_resource_reads_by_name_and_by_id_without_connecting() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let project = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "ctx-practice".into(), body: "b".into(), tags: vec![], project_id: Some(project.id) }, "test").await.unwrap();
        let before = project_writes(&backend);
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let listed = client.list_resources(None).await.unwrap();
        let context_uri = listed.resources.iter().map(|r| r.uri.clone()).find(|u| u.contains("/context")).expect("a context resource");
        assert!(context_uri.contains(&project.name), "{context_uri}");

        let by_name = client.read_resource(ReadResourceRequestParams::new(context_uri)).await.unwrap();
        let by_id = client.read_resource(ReadResourceRequestParams::new(format!("{PROJECTS}{}/context", project.id))).await.unwrap();
        for resource in [by_name, by_id] {
            let text: String = resource.contents.iter().filter_map(|c| match c { ResourceContents::TextResourceContents { text, .. } => Some(text.clone()), _ => None }).collect();
            assert!(text.contains("ctx-practice"), "{text}");
        }

        client.cancel().await.unwrap();
        assert_eq!(project_writes(&backend), before, "reading the context resource must not connect or re-profile the project");
    }

    /// A project root with a space round-trips through the resource listing: the
    /// listed `atlas://projects/{name}/board` URI is percent-encoded, and reading it
    /// back decodes to the same project and finds the board.
    #[tokio::test]
    async fn board_resource_uri_round_trips_a_name_with_a_space() {
        let home = tempfile::tempdir().unwrap();
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join("my project");
        std::fs::create_dir(&repo).unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        // project_connect is disabled by default, so the seeded root has to already be
        // a known project for the board tools to resolve it.
        backend.connect_project(repo.clone(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false).with_project_root(repo.clone());

        let created = s
            .task_create(Parameters(TaskCreateArgs {
                title: "spacey task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
            }))
            .await
            .unwrap();
        let task: atlas_core::models::Task = serde_json::from_str(&text_of(&created)).unwrap();

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let listed = client.list_resources(None).await.unwrap();
        let board_uri = listed.resources.iter().map(|r| r.uri.clone()).find(|u| u.contains("/board") && u.contains("%20")).expect("a board URI with an encoded space");

        let resource = client.read_resource(ReadResourceRequestParams::new(board_uri)).await.unwrap();
        let text: String = resource
            .contents
            .iter()
            .filter_map(|c| match c {
                ResourceContents::TextResourceContents { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(text.contains(&task.key), "{text}");

        client.cancel().await.unwrap();
    }

    /// `atlas://projects/{name}/practices` renders the global practices plus the
    /// project's own as one Markdown document; `atlas://memories/recent` renders the
    /// newest active memories.
    #[tokio::test]
    async fn practices_and_recent_memories_resources_render_markdown() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        let project = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "global-one".into(), body: "body one".into(), tags: vec![], project_id: None }, "test").await.unwrap();
        backend.save_doc(DocKind::Practice, NewDoc { name: "project-one".into(), body: "body two".into(), tags: vec![], project_id: Some(project.id) }, "test").await.unwrap();
        backend.remember(
            NewMemory { scope: MemoryScope::Global, project_id: None, kind: MemoryKind::Fact, text: "a recent fact".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: MemoryStatus::Active },
            "test",
        ).await.unwrap();
        let s = AtlasMcp::new(backend);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let uri = format!("atlas://projects/{}/practices", percent_encoding::utf8_percent_encode(&project.name, PROJECT_URI_SEGMENT));
        let resource = client.read_resource(ReadResourceRequestParams::new(uri)).await.unwrap();
        let text: String = resource.contents.iter().filter_map(|c| match c { ResourceContents::TextResourceContents { text, .. } => Some(text.clone()), _ => None }).collect();
        assert!(text.contains("global-one") && text.contains("project-one"), "{text}");

        let resource = client.read_resource(ReadResourceRequestParams::new(MEMORIES_RECENT)).await.unwrap();
        let text: String = resource.contents.iter().filter_map(|c| match c { ResourceContents::TextResourceContents { text, .. } => Some(text.clone()), _ => None }).collect();
        assert!(text.contains("a recent fact"), "{text}");

        client.cancel().await.unwrap();
    }

    /// Seeds a temp home with one user skill and answers with it. The home is handed
    /// to the backend through `AtlasPaths::with_skills_home`, never through the process
    /// environment: `ATLAS_SYNC_HOME` is read once, by `AtlasPaths::discover`, which no
    /// test calls, so nothing in this binary can reach the user's own `~/.claude`.
    fn seeded_skills_home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/skills/greeter")).unwrap();
        std::fs::write(
            dir.path().join(".claude/skills/greeter/SKILL.md"),
            "---\nname: greeter\ndescription: Greets a person by name.\n---\n\nSay hello.\n",
        )
        .unwrap();
        dir
    }

    /// `skill_list` answers with the discovered skills plus Atlas's own, and leaves out
    /// the ones the project switched off; `skill_get` answers with one skill's text.
    #[tokio::test]
    async fn skill_tools_list_and_read() {
        let skills_home = seeded_skills_home();
        let atlas_home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".claude/skills/deployer")).unwrap();
        std::fs::write(repo.path().join(".claude/skills/deployer/SKILL.md"), "---\nname: deployer\ndescription: Ships it.\n---\n\nRun the deploy.\n").unwrap();
        let paths = AtlasPaths::at(atlas_home.path()).with_skills_home(skills_home.path());
        let backend = Arc::new(LocalBackend::open(&paths, None, false).unwrap());
        let project = backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        backend.create_skill(NewSkill { project_id: None, name: "native-one".into(), description: "Stored in Atlas.".into(), body: "# native\n".into() }, "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false).with_project_root(repo.path().to_path_buf());

        let listed = text_of(&s.skill_list(Parameters(SkillListArgs { project_root: None })).await.unwrap());
        let list: SkillList = serde_json::from_str(&listed).unwrap();
        let ids: Vec<&str> = list.skills.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"claude-project:deployer"), "{ids:?}");
        assert!(ids.contains(&"claude-user:greeter"), "{ids:?}");
        assert_eq!(ids.iter().filter(|id| id.starts_with("claude-")).count(), 2, "only the two seeded folders: {ids:?}");
        assert!(list.skills.iter().any(|s| s.source == SkillSource::Native && s.name == "native-one"), "{ids:?}");

        let got = text_of(&s.skill_get(Parameters(SkillGetArgs { id: "claude-project:deployer".into(), project_root: None })).await.unwrap());
        assert!(got.contains("Run the deploy."), "{got}");

        // A skill this project switched off drops out of the tool's answer.
        backend.set_project_skills_disabled(project.id, vec!["claude-project:deployer".into()], "test").await.unwrap();
        let listed = text_of(&s.skill_list(Parameters(SkillListArgs { project_root: None })).await.unwrap());
        let list: SkillList = serde_json::from_str(&listed).unwrap();
        assert!(!list.skills.iter().any(|s| s.id == "claude-project:deployer"), "{listed}");
        assert!(list.skills.iter().any(|s| s.id == "claude-user:greeter"), "{listed}");
    }

    /// `atlas://skills/` lists the global skills as JSON and `atlas://skills/{id}`
    /// reads one skill's text.
    #[tokio::test]
    async fn skill_resources_list_and_read() {
        let skills_home = seeded_skills_home();
        let atlas_home = tempfile::tempdir().unwrap();
        let paths = AtlasPaths::at(atlas_home.path()).with_skills_home(skills_home.path());
        let backend = Arc::new(LocalBackend::open(&paths, None, false).unwrap());
        let s = AtlasMcp::new(backend).with_env_project_root(false);

        let (server_io, client_io) = tokio::io::duplex(16 * 1024);
        tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let listed = client.list_resources(None).await.unwrap();
        assert!(listed.resources.iter().any(|r| r.uri == SKILLS), "{:?}", listed.resources);

        let resource = client.read_resource(ReadResourceRequestParams::new(SKILLS)).await.unwrap();
        let text: String = resource.contents.iter().filter_map(|c| match c { ResourceContents::TextResourceContents { text, .. } => Some(text.clone()), _ => None }).collect();
        let list: SkillList = serde_json::from_str(&text).unwrap();
        assert!(list.skills.iter().any(|s| s.id == "claude-user:greeter"), "{text}");

        let resource = client.read_resource(ReadResourceRequestParams::new(format!("{SKILLS}claude-user:greeter"))).await.unwrap();
        let text: String = resource.contents.iter().filter_map(|c| match c { ResourceContents::TextResourceContents { text, .. } => Some(text.clone()), _ => None }).collect();
        assert!(text.contains("Say hello."), "{text}");

        let missing = client.read_resource(ReadResourceRequestParams::new(format!("{SKILLS}claude-user:nope"))).await;
        assert!(missing.is_err(), "an unknown skill id is not a resource");

        client.cancel().await.unwrap();
    }

    /// `task_update`'s `assignee`: an empty string clears it, an absent field leaves
    /// it alone, and a non-empty string sets it.
    #[tokio::test]
    async fn task_update_assignee_empty_string_clears_it_absent_leaves_it() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(home.path()), None, false).unwrap());
        // project_connect is disabled by default, so the seeded root has to already be
        // a known project for the board tools to resolve it.
        backend.connect_project(repo.path().to_path_buf(), "test").await.unwrap();
        let s = AtlasMcp::new(backend.clone()).with_env_project_root(false).with_project_root(repo.path().to_path_buf());

        let created = s
            .task_create(Parameters(TaskCreateArgs {
                title: "assignee task".into(), description: None, kind: None, priority: None,
                labels: None, parent: None, blocked_by: None, project_root: None, agent: None,
            }))
            .await
            .unwrap();
        let task: atlas_core::models::Task = serde_json::from_str(&text_of(&created)).unwrap();

        let update = |assignee: Option<String>, title: Option<String>| TaskUpdateArgs {
            key: task.key.clone(), title, description: None, kind: None, priority: None,
            assignee, labels: None, expected_updated_at: None, agent: None,
        };

        let assigned = s.task_update(Parameters(update(Some("ann".into()), None))).await.unwrap();
        let assigned: atlas_core::models::Task = serde_json::from_str(&text_of(&assigned)).unwrap();
        assert_eq!(assigned.assignee.as_deref(), Some("ann"));

        let cleared = s.task_update(Parameters(update(Some(String::new()), None))).await.unwrap();
        let cleared: atlas_core::models::Task = serde_json::from_str(&text_of(&cleared)).unwrap();
        assert_eq!(cleared.assignee, None);

        // Absent leaves it alone: renaming the task must not resurrect the assignee.
        let untouched = s.task_update(Parameters(update(None, Some("renamed".into())))).await.unwrap();
        let untouched: atlas_core::models::Task = serde_json::from_str(&text_of(&untouched)).unwrap();
        assert_eq!(untouched.assignee, None);
        assert_eq!(untouched.title, "renamed");
    }

    /// `workflow_run` on a name nothing was ever saved under answers `invalid_params`,
    /// the same shape every other lookup-by-name failure takes.
    #[tokio::test]
    async fn workflow_run_of_an_unknown_name_is_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        let s = AtlasMcp::new(backend);
        let err = s.workflow_run(Parameters(WorkflowRunArgs { name: "does-not-exist".into(), input: None })).await.unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{err:?}");
    }

    // ---- plugin MCP tools (Phase 13b) ----

    #[test]
    fn plugin_tool_names_round_trip() {
        assert_eq!(plugin_tool_name("hello-world", "count"), "plugin__hello_world__count");
        assert_eq!(parse_plugin_tool_name("plugin__hello_world__count"), Some(("hello_world".into(), "count".into())));
        // A tool name may hold single underscores; only the first doubled one splits.
        assert_eq!(parse_plugin_tool_name("plugin__hello_world__greet_twice"), Some(("hello_world".into(), "greet_twice".into())));
        for not_a_plugin_tool in ["memory_remember", "plugin__", "plugin__nodoubleunderscore", "plugin____count", "plugin__id__"] {
            assert_eq!(parse_plugin_tool_name(not_a_plugin_tool), None, "parsed '{not_a_plugin_tool}'");
        }
    }

    /// The mapping is injective over everything registration accepts, and every accepted
    /// pair survives `plugin_tool_name` then `parse_plugin_tool_name`, so no plugin can
    /// take another plugin's calls and no plugin's tool is listed under a name that
    /// resolves to nothing. `validate_plugin_tool_decls` is what makes this hold: no
    /// `--` in an id, no trailing `-` on an id, no `__` in a tool name.
    ///
    /// The lists carry the boundary shapes the three rules exclude, each named in
    /// `illegal_ids`/`illegal_names`, so loosening a rule turns one of them valid and
    /// fails the round trip rather than shipping quietly. `ab-` is the case that
    /// mattered: it encodes to `plugin__ab___count`, whose first `__` lands one character
    /// early and parses back as `("ab", "_count")`.
    #[test]
    fn distinct_plugin_tools_never_share_an_mcp_name() {
        let ids = ["hello-world", "hello", "hello_world_is_not_an_id", "a1", "x-y-z", "helloworld", "hello-w", "ab-", "hello--world", "a-b-"];
        let names = ["count", "greet_twice", "c", "count_2", "b_count", "world_count", "count_", "b__count"];
        let illegal_ids = ["hello_world_is_not_an_id", "ab-", "hello--world", "a-b-"];
        let illegal_names = ["b__count"];
        let mut seen: HashMap<String, (&str, &str)> = HashMap::new();
        for id in ids {
            for name in names {
                let decl = PluginToolDecl {
                    plugin_id: id.into(), name: name.into(), description: "x".into(),
                    args: serde_json::json!({"type": "object"}),
                    scope: atlas_core::models::PluginToolScope::Read,
                };
                let valid = atlas_core::settings::validate_plugin_tool_decls(&[decl]).is_ok();
                let mcp_name = plugin_tool_name(id, name);
                if !valid {
                    assert!(
                        illegal_ids.contains(&id) || illegal_names.contains(&name),
                        "unexpectedly invalid: {id} / {name}",
                    );
                    continue;
                }
                assert!(!illegal_ids.contains(&id), "{id} should have been refused as an id");
                assert!(!illegal_names.contains(&name), "{name} should have been refused as a tool name");
                assert_eq!(parse_plugin_tool_name(&mcp_name), Some((id.replace('-', "_"), name.to_string())), "{mcp_name}");
                if let Some(other) = seen.insert(mcp_name.clone(), (id, name)) {
                    panic!("{mcp_name} is produced by both {other:?} and ({id}, {name})");
                }
            }
        }
        assert!(!seen.is_empty(), "the loop never reached a valid pair");
    }

    /// A plugin tool can never shadow a built-in, because no built-in name starts with
    /// the prefix every plugin tool carries. Asserted rather than assumed: adding a
    /// `plugin__`-prefixed tool to `TOOL_TABLE` would otherwise be silently shadowable.
    #[test]
    fn plugin_tool_names_never_collide_with_a_builtin() {
        for meta in TOOL_TABLE {
            assert!(!meta.name.starts_with(PLUGIN_TOOL_PREFIX), "{} would collide with a plugin tool", meta.name);
        }
    }

    /// A `PluginToolHost` that answers from a fixed table and records what it was asked,
    /// standing in for the daemon's WebSocket channel.
    struct StubHost {
        decls: Vec<PluginToolDecl>,
        calls: Arc<Mutex<Vec<(String, String, serde_json::Value)>>>,
        /// When false, every call answers the way an app that is not connected does.
        connected: bool,
    }

    impl atlas_core::plugin_tools::PluginToolHost for StubHost {
        fn list(&self) -> Vec<PluginToolDecl> { self.decls.clone() }
        fn call(&self, plugin_id: &str, name: &str, args: serde_json::Value) -> atlas_core::plugin_tools::BoxFuture<'_, atlas_core::Result<serde_json::Value>> {
            let connected = self.connected;
            let plugin_id = plugin_id.to_string();
            let name = name.to_string();
            let calls = self.calls.clone();
            Box::pin(async move {
                if !connected {
                    return Err(atlas_core::AtlasError::Invalid(format!("plugin {plugin_id} is not running")));
                }
                calls.lock().unwrap().push((plugin_id, name, args.clone()));
                Ok(serde_json::json!({ "echoed": args }))
            })
        }
    }

    fn stub_decls() -> Vec<PluginToolDecl> {
        ["count", "greet_twice"].into_iter().map(|name| PluginToolDecl {
            plugin_id: "hello-world".into(),
            name: name.into(),
            description: format!("The {name} tool."),
            args: serde_json::json!({"type": "object", "properties": {"n": {"type": "integer"}}}),
            scope: atlas_core::models::PluginToolScope::Read,
        }).collect()
    }

    /// Two plugin tools are listed under their MCP names alongside the built-ins,
    /// `mcp.disabled_tools` hides one exactly as it hides a built-in, and a call is
    /// forwarded to the host with the dashed plugin id and the caller's arguments.
    #[tokio::test]
    async fn plugin_tools_are_listed_gated_and_forwarded() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let host = Arc::new(StubHost { decls: stub_decls(), calls: calls.clone(), connected: true });
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap().with_plugin_tool_host(host));
        let s = AtlasMcp::new(backend.clone()).with_source_tool("test");

        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let handle = tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let tools = client.list_tools(None).await.unwrap();
        let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"plugin__hello_world__count"), "{names:?}");
        assert!(names.contains(&"plugin__hello_world__greet_twice"), "{names:?}");
        assert!(names.contains(&"memory_remember"), "the built-ins are still there: {names:?}");
        let listed = tools.tools.iter().find(|t| t.name == "plugin__hello_world__count").unwrap();
        assert_eq!(listed.description.as_deref(), Some("The count tool."));
        assert_eq!(listed.input_schema.get("type").and_then(|v| v.as_str()), Some("object"), "the decl's args become the input schema");

        let params = CallToolRequestParams::new("plugin__hello_world__count")
            .with_arguments(serde_json::Map::from_iter([("n".to_string(), serde_json::json!(3))]));
        let result = client.call_tool(params).await.unwrap();
        let text: String = result.content.iter().filter_map(|c| c.as_text()).map(|t| t.text.clone()).collect();
        let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(payload["echoed"]["n"], 3, "{payload}");
        let recorded = calls.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "hello-world", "the dashed id, restored from the registry");
        assert_eq!(recorded[0].1, "count");
        assert_eq!(recorded[0].2["n"], 3);

        // The call wrote an audit row, the same way every built-in write does.
        let audited: i64 = backend.db.with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'plugin_tool_call'", [], |r| r.get::<_, i64>(0))?)).unwrap();
        assert_eq!(audited, 1);

        backend.set_settings(serde_json::Map::from_iter([("mcp.disabled_tools".to_string(), serde_json::json!(["plugin__hello_world__count"]))]), "t").await.unwrap();
        let tools = client.list_tools(None).await.unwrap();
        let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(!names.contains(&"plugin__hello_world__count"), "a disabled plugin tool is hidden: {names:?}");
        assert!(names.contains(&"plugin__hello_world__greet_twice"), "{names:?}");
        let refused = client.call_tool(CallToolRequestParams::new("plugin__hello_world__count")).await.unwrap_err();
        assert!(matches!(&refused, rmcp::service::ServiceError::McpError(e) if e.code == rmcp::model::ErrorCode::METHOD_NOT_FOUND), "{refused:?}");

        client.cancel().await.unwrap();
        handle.await.unwrap();
    }

    /// An app that is not connected surfaces as `invalid_params` carrying the host's own
    /// "is not running" message, not as an internal error.
    #[tokio::test]
    async fn a_plugin_tool_call_with_no_app_connected_is_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(StubHost { decls: stub_decls(), calls: Arc::new(Mutex::new(Vec::new())), connected: false });
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap().with_plugin_tool_host(host));
        let s = AtlasMcp::new(backend).with_source_tool("test");

        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let handle = tokio::spawn(async move {
            let running = s.serve(server_io).await.unwrap();
            running.waiting().await.unwrap();
        });
        let client: rmcp::service::RunningService<rmcp::RoleClient, ()> = ().serve(client_io).await.unwrap();

        let call = client.call_tool(CallToolRequestParams::new("plugin__hello_world__count")).await.unwrap_err();
        match &call {
            rmcp::service::ServiceError::McpError(e) => {
                assert_eq!(e.code, rmcp::model::ErrorCode::INVALID_PARAMS, "{e:?}");
                assert!(e.message.contains("hello-world is not running"), "{e:?}");
            }
            other => panic!("{other:?}"),
        }

        client.cancel().await.unwrap();
        handle.await.unwrap();
    }

    /// A backend with no plugin host behind it lists no plugin tools and refuses a call
    /// to one, which is what the CLI's in-process server and every test backend see.
    #[tokio::test]
    async fn a_backend_without_a_plugin_host_lists_none_and_refuses_a_call() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalBackend::open(&AtlasPaths::at(dir.path()), None, false).unwrap());
        assert!(backend.plugin_tools().await.unwrap().is_empty());
        let e = backend.call_plugin_tool("hello-world", "count", serde_json::json!({}), "test").await.unwrap_err();
        assert!(matches!(e, atlas_core::AtlasError::Invalid(ref m) if m.contains("is not running")), "{e:?}");
    }
}
