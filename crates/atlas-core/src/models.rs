use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

macro_rules! str_enum {
    ($name:ident { $($var:ident => $s:literal),* $(,)? }) => {
        // Each variant is renamed to the same literal `as_str`/`FromStr` use, so the JSON
        // wire form and the string form never drift apart (`rename_all = "lowercase"`
        // would spell `AgentsMd` as `agentsmd` while `as_str` says `agents_md`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
        pub enum $name { $(#[serde(rename = $s)] $var),* }
        impl $name {
            pub fn as_str(&self) -> &'static str { match self { $(Self::$var => $s),* } }
        }
        impl std::str::FromStr for $name {
            type Err = crate::AtlasError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s { $($s => Ok(Self::$var),)* _ => Err(crate::AtlasError::Invalid(format!("unknown {}: {s}", stringify!($name)))) }
            }
        }
        impl std::fmt::Display for $name { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str(self.as_str()) } }
    };
}

str_enum!(MemoryScope { Global => "global", Project => "project" });
str_enum!(MemoryKind { Fact => "fact", Decision => "decision", Preference => "preference", Insight => "insight", Todo => "todo" });
str_enum!(MemoryStatus { Active => "active", Pending => "pending", Rejected => "rejected", Superseded => "superseded" });

// How a memory listing treats the project it was given. `All` is the store's own
// rule, where a project widens rather than narrows: that project's memories *plus*
// every global one, which is what an agent starting work wants. `ProjectOnly` narrows
// to the project's own rows, for a screen that has already said whose memories it is
// showing and would be lying to mix the global ones in. `GlobalOnly` narrows to the
// project-less memories, ignoring any `project_id` given alongside it, for a screen
// whose scope filter is the literal global scope rather than one project.
// `All` is the default wherever one is needed; it is spelled out at each call site
// rather than through `Default`, since `str_enum!` builds the enum and a derived
// default would have to be threaded through the macro for one use.
str_enum!(MemoryScopeFilter { All => "all", ProjectOnly => "project_only", GlobalOnly => "global_only" });

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewMemory {
    pub scope: MemoryScope,
    pub project_id: Option<Uuid>,
    pub kind: MemoryKind,
    pub text: String,
    #[serde(default)] pub tags: Vec<String>,
    pub source_agent: Option<String>,
    pub source_tool: Option<String>,
    #[serde(default = "one")] pub confidence: f64,
    #[serde(default = "active")] pub status: MemoryStatus,
}
fn one() -> f64 { 1.0 }
fn active() -> MemoryStatus { MemoryStatus::Active }

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Memory {
    pub id: Uuid,
    pub scope: MemoryScope,
    pub project_id: Option<Uuid>,
    pub kind: MemoryKind,
    pub text: String,
    pub tags: Vec<String>,
    pub source_agent: Option<String>,
    pub source_tool: Option<String>,
    pub confidence: f64,
    pub status: MemoryStatus,
    pub superseded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecallHit { pub memory: Memory, pub score: f64 }

/// Kind and tag counts, plus the total, over the active memories `GET
/// /memories/facets` was asked about; filtered the same way `GET /memories` filters
/// `project_id`, via `MemoryScopeFilter`. A side panel renders these without loading
/// every matching memory first.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryFacets {
    pub kinds: std::collections::HashMap<String, i64>,
    pub tags: std::collections::HashMap<String, i64>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatusReport {
    pub version: String,
    pub db_path: String,
    pub memories_active: i64,
    pub memories_pending: i64,
    pub embedding: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct ProjectProfile {
    pub name: String,
    #[serde(default)] pub languages: Vec<String>,
    #[serde(default)] pub frameworks: Vec<String>,
    #[serde(default)] pub tree: Vec<String>,
    #[serde(default)] pub readme_head: String,
    #[serde(default)] pub recent_commits: Vec<String>,
    #[serde(default)] pub summary: Option<String>,
    #[serde(default = "chrono::Utc::now")] pub built_at: DateTime<Utc>,
    /// Planning frameworks (Superpowers, OpenSpec, SpecKit, GSD) detected in the
    /// project, distinct from the code frameworks above. `serde(default)` so a
    /// profile stored before this field existed still deserialises.
    #[serde(default)] pub planning_frameworks: Vec<FrameworkInventory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub root_path: String,
    pub git_remote: Option<String>,
    pub profile: Option<ProjectProfile>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    /// Board key prefix for this project's task keys (`ATL` in `ATL-12`). Null on
    /// rows written before migration 3; `TaskRepo::next_key` fills it in on first use.
    #[serde(default)] pub board_key: Option<String>,
    /// Per-project stage list. `None` means "use the global list".
    #[serde(default)] pub board_stages: Option<Vec<Stage>>,
    /// Which agent labels may write here. All-null by default, which lets anyone write.
    #[serde(default)] pub agent_access: AgentAccess,
    /// Per-project extraction override. `None` means "use the global settings".
    /// `api_key` is masked to `"***"` on every read, like the global setting.
    #[serde(default)] pub extraction: Option<ProjectExtraction>,
}

/// Who may write to a project, by actor label. `None` means any actor; a list is an
/// allow-list matched against the full actor string (`claude-code/reviewer`) or the
/// part before the slash (`claude-code`). The user's own hands are always exempt:
/// see [`crate::projects::actor_is_user`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct AgentAccess {
    #[serde(default)] pub memory_writers: Option<Vec<String>>,
    #[serde(default)] pub task_movers: Option<Vec<String>>,
    /// Forces a memory written here by an agent to land `pending` instead of `active`.
    #[serde(default)] pub require_review: bool,
}

/// A project's extraction override, with the same fields as the global
/// `extraction.*` settings. An absent field falls back to the global value, so a
/// project can point one endpoint somewhere else without restating the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct ProjectExtraction {
    #[serde(default)] pub enabled: Option<bool>,
    #[serde(default)] pub base_url: Option<String>,
    #[serde(default)] pub model: Option<String>,
    #[serde(default)] pub api_key: Option<String>,
    #[serde(default)] pub auto_accept_min_confidence: Option<f64>,
}

/// A patch to a project's identity. An absent field is left alone; `git_remote` is a
/// double option, so an explicit JSON `null` clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub board_key: Option<String>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<Option<String>>,
}

/// What a log entry points at: a task (with its key), a memory, a job, a project or a
/// sync target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogRef {
    #[serde(rename = "type")] pub kind: String,
    #[serde(default)] pub id: Option<Uuid>,
    #[serde(default)] pub key: Option<String>,
}

/// One line of a project's unified log: task events, memory audit rows, project and
/// sync audit rows, and extraction jobs, merged and sorted newest first.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogEntry {
    pub time: DateTime<Utc>,
    pub source: String,
    pub kind: String,
    pub detail: String,
    #[serde(rename = "ref")] pub reference: Option<LogRef>,
}

/// Narrows a project log. `after` pages: it keeps entries strictly older than the
/// given time, which is the last entry of the previous page.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogFilter {
    #[serde(default)] pub source: Option<String>,
    #[serde(default)] pub kind: Option<String>,
    #[serde(default)] pub q: Option<String>,
    #[serde(default)] pub after: Option<DateTime<Utc>>,
    #[serde(default)] pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewAgent {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub model_hint: Option<String>,
    #[serde(default)] pub tools: Vec<String>,
    #[serde(default)] pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub model_hint: Option<String>,
    pub tools: Vec<String>,
    pub tags: Vec<String>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

str_enum!(DocKind { Practice => "practice", Workflow => "workflow" });

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewDoc {
    pub name: String,
    pub body: String,
    #[serde(default)] pub tags: Vec<String>,
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Doc {
    pub id: Uuid,
    pub kind: DocKind,
    pub name: String,
    pub body: String,
    pub tags: Vec<String>,
    pub project_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecallQuery {
    pub query: String,
    #[serde(default = "ten")] pub limit: usize,
    #[serde(default)] pub scope: Option<MemoryScope>,
    /// How `project_id` is read, the same narrowing `GET /memories` takes: `All` widens
    /// to the project plus every global memory, `ProjectOnly` keeps the project's own.
    #[serde(default = "scope_all")] pub list_scope: MemoryScopeFilter,
    #[serde(default)] pub project_id: Option<Uuid>,
    #[serde(default)] pub kinds: Vec<MemoryKind>,
    #[serde(default)] pub tags: Vec<String>,
}
fn ten() -> usize { 10 }
fn scope_all() -> MemoryScopeFilter { MemoryScopeFilter::All }

str_enum!(SyncKind {
    Claude => "claude",
    Codex => "codex",
    AgentsMd => "agents_md",
    ClaudeMd => "claude_md",
    ClaudeHook => "claude_hook",
    CodexHook => "codex_hook",
    TasksMd => "tasks_md",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum SyncAction {
    Create,
    Update,
    Unchanged,
    Skip(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SyncOp {
    pub kind: SyncKind,
    pub path: PathBuf,
    pub content: String,
    pub action: SyncAction,
}

/// Everything an agent needs to start work in a project: the project itself,
/// the memories worth reading first, and the practices and workflows in scope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectContext {
    pub project: Project,
    pub memories: Vec<RecallHit>,
    pub practices: Vec<Doc>,
    pub workflows: Vec<WorkflowSummary>,
}

/// One sync request. `root` is required unless `global` is set, in which case
/// the sync targets a home directory instead of a project: `ATLAS_SYNC_HOME`
/// when set on the daemon process, else the daemon user's own home. There is
/// no client-side override; `POST /sync` is unauthenticated, so the request
/// itself must not be able to name an arbitrary write target.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct SyncRequest {
    #[serde(default)] pub root: Option<PathBuf>,
    #[serde(default)] pub global: bool,
    #[serde(default)] pub targets: Vec<SyncKind>,
    #[serde(default)] pub check_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct SyncReport {
    pub ops: Vec<SyncOp>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Board (Phase 6)
// ---------------------------------------------------------------------------

str_enum!(TaskKind { Task => "task", Bug => "bug", Feature => "feature", Chore => "chore" });
str_enum!(TaskPriority { Low => "low", Medium => "medium", High => "high", Urgent => "urgent" });

/// One column of the board. `done` marks the terminal columns: moving into one
/// stamps `closed_at`, moving out clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Stage {
    pub name: String,
    #[serde(default)] pub done: bool,
}

/// A resolved stage list plus whether it came from a project override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StageList {
    pub stages: Vec<Stage>,
    pub overridden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Task {
    pub id: Uuid,
    pub key: String,
    pub project_id: Option<Uuid>,
    pub seq: i64,
    pub title: String,
    pub description: String,
    pub stage: String,
    pub kind: TaskKind,
    pub priority: TaskPriority,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub parent_id: Option<Uuid>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    /// Keys of the tasks this one waits on.
    pub blocked_by: Vec<String>,
    /// How many of `blocked_by` are not themselves in a done stage. The ready rule
    /// counts these and not the rest, so a badge drawn from this number agrees with
    /// `ready` instead of counting blockers that are already finished.
    pub open_blockers: usize,
    /// Computed on read, never stored: not in a done stage, every blocker done,
    /// and no open subtask.
    pub ready: bool,
    /// Why `ready` is false, when the task is open but held up.
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct NewTask {
    #[serde(default)] pub project_id: Option<Uuid>,
    pub title: String,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub kind: Option<TaskKind>,
    #[serde(default)] pub priority: Option<TaskPriority>,
    #[serde(default)] pub assignee: Option<String>,
    #[serde(default)] pub labels: Option<Vec<String>>,
    /// Parent task, by id or key.
    #[serde(default)] pub parent: Option<String>,
    /// Blocking tasks, by id or key.
    #[serde(default)] pub blocked_by: Option<Vec<String>>,
    #[serde(default)] pub stage: Option<String>,
}

/// A patch. An absent field is left alone. `assignee` and `parent` are double
/// options so an explicit JSON `null` clears them: absent is `None`, `null` is
/// `Some(None)`, a value is `Some(Some(v))`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct TaskUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub kind: Option<TaskKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub priority: Option<TaskPriority>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub labels: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub parent: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub expected_updated_at: Option<DateTime<Utc>>,
}

/// Turns a present-but-null JSON field into `Some(None)` instead of `None`, so a
/// caller can tell "clear this" apart from "leave it alone". `#[serde(default)]`
/// still supplies `None` for an absent field.
fn double_option<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub actor: String,
    /// created, edited, moved, assigned, commented, blocked, unblocked, deleted.
    pub kind: String,
    pub body: String,
    pub detail: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskDetail {
    pub task: Task,
    pub children: Vec<Task>,
    pub events: Vec<TaskEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct TaskFilter {
    #[serde(default)] pub project_id: Option<Uuid>,
    #[serde(default)] pub stage: Option<String>,
    #[serde(default)] pub assignee: Option<String>,
    /// Keep only tasks that are ready to be worked on.
    #[serde(default)] pub ready: bool,
    /// Case-insensitive substring match over key, title and description.
    #[serde(default)] pub query: Option<String>,
    #[serde(default)] pub include_done: bool,
    /// Keep only tasks with no project at all: the literal global board, distinct from
    /// a bare `project_id: None`, which leaves every project's tasks in.
    #[serde(default)] pub global_only: bool,
}

// ---- workflows ----

str_enum!(TriggerKind { Manual => "manual", Schedule => "schedule", Prompt => "prompt" });
str_enum!(NodeKind { Trigger => "trigger", Action => "action", Output => "output" });
str_enum!(RunStatus { Queued => "queued", Running => "running", Success => "success", Failed => "failed", Cancelled => "cancelled" });
str_enum!(StepStatus {
    Queued => "queued", Running => "running", Success => "success",
    Failed => "failed", Skipped => "skipped", Cancelled => "cancelled",
});
// Upper case on the wire because the log lines are read as text in the run view, where
// `INFO`/`WARN`/`ERR` line up in a fixed-width gutter.
// `Error` rather than `Err`: a variant named `Err` shadows the `Err` associated type
// `str_enum!`'s own `FromStr` impl names.
str_enum!(LogLevel { Info => "INFO", Warn => "WARN", Error => "ERR" });

impl RunStatus {
    /// Whether the run has stopped for good. A terminal status is the one moment
    /// `finished_at` is stamped and the only status `cancel_run` refuses to move.
    pub fn is_terminal(&self) -> bool {
        matches!(self, RunStatus::Success | RunStatus::Failed | RunStatus::Cancelled)
    }
}

/// What starts a workflow. `cron` is read only when `kind` is `schedule` and `prompt`
/// only when it is `prompt`; both are carried on every trigger so the editor can keep
/// a half-typed value while the user switches kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Trigger {
    pub kind: TriggerKind,
    #[serde(default)] pub cron: Option<String>,
    #[serde(default)] pub prompt: Option<String>,
}

impl Trigger {
    pub fn manual() -> Self {
        Trigger { kind: TriggerKind::Manual, cron: None, prompt: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// Which memories an action is given. Empty `kinds` or `tags` mean "no filter on
/// that axis", not "match nothing".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemorySource {
    #[serde(default)] pub kinds: Vec<String>,
    #[serde(default)] pub tags: Vec<String>,
    #[serde(default = "twenty")] pub limit: u32,
    #[serde(default)] pub project_id: Option<Uuid>,
}
fn twenty() -> u32 { 20 }

/// The body of a node, shaped by the node's `kind`. Untagged rather than tagged: the
/// three shapes have disjoint required fields (`kind` / `name` / `propose_memories`),
/// the editor sends the plain object the GUI holds, and `graph::validate` is what
/// checks the variant against the node's declared `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum NodeData {
    Trigger(Trigger),
    Action {
        name: String,
        instructions: String,
        agent: String,
        #[serde(default)] practices: Vec<String>,
        #[serde(default)] memories: Option<MemorySource>,
    },
    Output {
        propose_memories: bool,
        file_tasks: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub position: Position,
    pub data: NodeData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct Graph {
    #[serde(default)] pub nodes: Vec<Node>,
    #[serde(default)] pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    pub project_id: Option<Uuid>,
    pub description: String,
    pub trigger: Trigger,
    pub graph: Graph,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_status: Option<RunStatus>,
}

/// A workflow's shape without its graph: what an agent (over MCP) or a project's
/// context needs to decide whether a workflow is worth looking at closer, without the
/// weight of its full node/edge JSON. `get_workflow` (MCP) and the desktop's own
/// `GET /api/v1/workflows/{id}` still answer with the full [`Workflow`], graph
/// included.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowSummary {
    pub id: Uuid,
    pub name: String,
    pub trigger: TriggerKind,
    pub action_count: usize,
    pub enabled: bool,
    pub last_status: Option<RunStatus>,
}

impl From<&Workflow> for WorkflowSummary {
    fn from(w: &Workflow) -> Self {
        let action_count = w.graph.nodes.iter().filter(|n| n.kind == NodeKind::Action).count();
        WorkflowSummary { id: w.id, name: w.name.clone(), trigger: w.trigger.kind, action_count, enabled: w.enabled, last_status: w.last_status }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewWorkflow {
    pub name: String,
    #[serde(default)] pub project_id: Option<Uuid>,
    #[serde(default)] pub description: String,
    pub trigger: Trigger,
    #[serde(default)] pub graph: Graph,
    #[serde(default = "yes")] pub enabled: bool,
}
fn yes() -> bool { true }

/// A patch. `project_id` is a double option so a caller can tell "leave the project
/// alone" from "make this workflow global", the same distinction `TaskUpdate` draws.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct WorkflowPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Option<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub trigger: Option<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub graph: Option<Graph>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowRun {
    pub id: Uuid,
    pub workflow_id: Uuid,
    /// Per-workflow, starting at 1: the number a run is known by in the GUI and CLI.
    pub number: i64,
    pub trigger: TriggerKind,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub summary: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogLine {
    pub ts: DateTime<Utc>,
    pub level: LogLevel,
    pub text: String,
}

impl LogLine {
    pub fn now(level: LogLevel, text: impl Into<String>) -> Self {
        LogLine { ts: Utc::now(), level, text: text.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowStep {
    pub id: Uuid,
    pub run_id: Uuid,
    /// Zero-based index in the run's execution order.
    pub position: i32,
    /// The graph node this step ran, so a step can be traced back to the canvas.
    pub action_id: String,
    pub name: String,
    pub agent: String,
    pub status: StepStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub output: Option<String>,
    pub log: Vec<LogLine>,
}

// ---------------------------------------------------------------------------
// Framework adapters (Phase 12): Superpowers, OpenSpec, SpecKit, GSD
// ---------------------------------------------------------------------------

str_enum!(FrameworkKind {
    Superpowers => "superpowers",
    Openspec => "openspec",
    Speckit => "speckit",
    Gsd => "gsd",
});

str_enum!(FrameworkDocType {
    Spec => "spec",
    Plan => "plan",
    Tasks => "tasks",
    Roadmap => "roadmap",
    Ledger => "ledger",
    Proposal => "proposal",
    Summary => "summary",
    Todo => "todo",
});

/// What `FrameworkAdapter::detect` found for one framework: which of its roots
/// exist under the project, and shallow, listing-only counts of what it holds.
/// Stored on `ProjectProfile.planning_frameworks` on connect and refresh.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FrameworkInventory {
    pub kind: FrameworkKind,
    /// Existing root paths for this framework, relative to the project root.
    pub roots: Vec<String>,
    /// Matching document files by name and extension. Equal to `documents(root).len()`.
    pub docs: usize,
    /// Files that hold tasks (a `tasks.md`, a plan file, and so on), counted by
    /// name and extension, not by opening them: `detect` never reads a document,
    /// so this is a file count, not the exact number of importable checkbox
    /// items — call `tasks(root)` for that.
    pub tasks: usize,
    pub detected_at: DateTime<Utc>,
}

/// One document a framework adapter found: a spec, plan, ledger and so on.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FrameworkDoc {
    pub kind: FrameworkKind,
    /// Path relative to the project root, as passed back to `FrameworkAdapter::read`.
    pub path: String,
    pub title: String,
    pub doc_type: FrameworkDocType,
    pub updated_at: DateTime<Utc>,
}

/// Points an imported task or decision back at the framework file it came from.
/// `anchor` names where inside that file: a heading, a ruling label, or similarly
/// a short human-readable locator, empty when the whole file is the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SourceRef {
    pub framework: FrameworkKind,
    pub path: String,
    #[serde(default)] pub anchor: String,
}

/// A task line found by `FrameworkAdapter::tasks`, ready to become (or update) a
/// board task under `import::import_tasks`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImportedTask {
    pub title: String,
    pub description: String,
    /// Free-text status as the framework spelled it (for example `done`, `todo`),
    /// left to `import::import_tasks` to map onto a board stage.
    #[serde(default)] pub status_hint: Option<String>,
    pub source_ref: SourceRef,
}

/// A decision line found by `FrameworkAdapter::decisions`, ready to become a
/// pending memory under `import::import_decisions`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImportedDecision {
    pub text: String,
    pub source_ref: SourceRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three states a clearable field can arrive in. Without the custom
    /// `deserialize_with`, an explicit `null` and an absent field would both read
    /// back as `None` and a caller could never clear an assignee.
    #[test]
    fn a_clearable_field_tells_absent_from_null_from_a_value() {
        let absent: TaskUpdate = serde_json::from_str(r#"{"title": "t"}"#).unwrap();
        assert_eq!(absent.assignee, None);
        assert_eq!(absent.parent, None);

        let cleared: TaskUpdate = serde_json::from_str(r#"{"assignee": null, "parent": null}"#).unwrap();
        assert_eq!(cleared.assignee, Some(None));
        assert_eq!(cleared.parent, Some(None));

        let set: TaskUpdate = serde_json::from_str(r#"{"assignee": "ann", "parent": "ATL-1"}"#).unwrap();
        assert_eq!(set.assignee, Some(Some("ann".to_string())));
        assert_eq!(set.parent, Some(Some("ATL-1".to_string())));

        // A round trip keeps the distinction: absent stays absent, null stays null.
        assert_eq!(serde_json::to_string(&TaskUpdate::default()).unwrap(), "{}");
        let json = serde_json::to_string(&cleared).unwrap();
        assert_eq!(json, r#"{"assignee":null,"parent":null}"#);
        let again: TaskUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(again.assignee, Some(None));
    }
}
