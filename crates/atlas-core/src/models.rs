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
    /// MCP tool names disabled for this project on top of the global
    /// `mcp.disabled_tools` list. Empty means no project override.
    #[serde(default)] pub mcp_disabled_tools: Vec<String>,
    /// Skill ids switched off for this project. Empty means every skill that applies
    /// here is on. The ids are [`SkillSummary::id`] values, so a discovered skill keeps
    /// its meaning across restarts.
    #[serde(default)] pub skills_disabled: Vec<String>,
}

/// Who may write to a project, by actor label. `None` means any actor; a list is an
/// allow-list matched against the full actor string (`claude-code/reviewer`) or the
/// part before the slash (`claude-code`). The user's own hands are always exempt:
/// see [`crate::projects::actor_is_user`]. A project's unset field falls back to the
/// global `access.*` settings default (`require_review` as a floor it can only raise):
/// see [`crate::projects::effective_access`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct AgentAccess {
    #[serde(default)] pub memory_writers: Option<Vec<String>>,
    #[serde(default)] pub task_movers: Option<Vec<String>>,
    /// Forces a memory written here by an agent to land `pending` instead of `active`.
    #[serde(default)] pub require_review: bool,
}

/// A project's access rules in all three shapes at once: its own `agent_access`, the
/// global `access.*` defaults, and the two resolved together (`crate::projects::effective_access`).
/// `GET /api/v1/projects/{id}/access` answers with this so a client can show the rule
/// that actually applies without also fetching `GET /api/v1/settings` to compute it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectAccess {
    pub access: AgentAccess,
    pub defaults: AgentAccess,
    pub effective: AgentAccess,
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
    /// Replaces the project's MCP tool override wholesale when present. Validated
    /// against the same known-tool-name list as the global `mcp.disabled_tools`.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub mcp_disabled_tools: Option<Vec<String>>,
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

// ---------------------------------------------------------------------------
// Skills (Phase 15)
// ---------------------------------------------------------------------------

// Where a skill comes from. `native` is stored in Atlas's own database; the rest are
// `SKILL.md` folders discovered on disk, at the project level (`claude-project`,
// `codex-project`), at the user level (`claude-user`, `codex-user`), or inside an
// installed Claude Code plugin (`plugin`).
str_enum!(SkillSource {
    Native => "native",
    ClaudeProject => "claude-project",
    ClaudeUser => "claude-user",
    CodexProject => "codex-project",
    CodexUser => "codex-user",
    Plugin => "plugin",
});

/// One skill as a listing shows it, without its body.
///
/// `id` is `"<source>:<path of the skill folder relative to its source root>"` for a
/// discovered skill (a plugin skill's root is the plugin's `skills` directory, so its id
/// is `plugin:<marketplace>/<plugin>/<skill>` and survives the plugin being updated into
/// a new version directory), and the UUID for a native one. Ids are stable across
/// restarts, so a project's disabled list keeps its meaning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkillSummary {
    pub id: String,
    pub source: SkillSource,
    pub name: String,
    pub description: String,
    /// `global` for a user, plugin or project-less native skill; `project` for one that
    /// belongs to a single project. The same two words `MemoryScope` uses.
    pub scope: MemoryScope,
    pub project_id: Option<Uuid>,
    /// Absolute path of the skill folder, for a discovered skill. `None` for a native one.
    pub path: Option<String>,
    /// `"<marketplace>/<plugin>"` for a plugin skill, `None` otherwise.
    pub plugin: Option<String>,
    /// Whether Atlas can write this skill's body: every native skill, and every
    /// discovered one whose `SKILL.md` the daemon user may write.
    pub editable: bool,
    pub updated_at: Option<DateTime<Utc>>,
    /// Whether the skill is on for the project a listing was asked about. `None` when
    /// no project was given.
    #[serde(default)] pub enabled_here: Option<bool>,
}

/// One skill with its text: the whole `SKILL.md` (frontmatter included) for a
/// discovered skill, the stored body for a native one.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Skill {
    #[serde(flatten)] pub summary: SkillSummary,
    pub body: String,
    /// The other files in the skill folder, relative to it, at most 200, for display
    /// only: nothing reads or writes them.
    #[serde(default)] pub files: Vec<String>,
}

/// A skill listing plus whatever discovery could not read. A warning never fails the
/// listing: one unreadable folder must not hide every other skill.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkillList {
    pub skills: Vec<SkillSummary>,
    #[serde(default)] pub warnings: Vec<String>,
}

/// A new Atlas-native skill. `project_id` scopes it to one project; `None` is global.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewSkill {
    #[serde(default)] pub project_id: Option<Uuid>,
    pub name: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub body: String,
}

/// A patch to a native skill. An absent field is left alone.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkillUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub body: Option<String>,
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
    FrameworkInstructions => "framework_instructions",
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
/// the memories worth reading first, and the practices, workflows and skills in scope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectContext {
    pub project: Project,
    pub memories: Vec<RecallHit>,
    pub practices: Vec<Doc>,
    pub workflows: Vec<WorkflowSummary>,
    /// The skills that apply here, minus the ones this project switched off, so an
    /// agent reading its context knows which skills are actually in play.
    #[serde(default)] pub skills: Vec<SkillSummary>,
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
    /// The parent's key and title when this is a subtask, read with the row, so a
    /// board card can name its parent even when the parent is filtered out of the
    /// same listing.
    #[serde(default)] pub parent_key: Option<String>,
    #[serde(default)] pub parent_title: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    /// Where this task was imported from, when it was. `import::import_tasks` looks
    /// tasks up by this field so a re-import updates rather than duplicates.
    #[serde(default)] pub source_ref: Option<SourceRef>,
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
    /// How many direct subtasks this task has. Computed on read, the same way
    /// `open_blockers` and `ready` are.
    pub subtasks_total: u32,
    /// How many of `subtasks_total` sit in a done stage.
    pub subtasks_done: u32,
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
    /// Set by `import::import_tasks` so a re-import finds this task again.
    #[serde(default)] pub source_ref: Option<SourceRef>,
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
    /// `Some(true)` keeps only tasks with no parent (`parent_id is null`); `Some(false)`
    /// keeps only subtasks; `None` applies no filter either way.
    #[serde(default)] pub top_level: Option<bool>,
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
    /// The parent task's own `source_ref.anchor`, when this item is a subtask of
    /// another `ImportedTask` from the same adapter run. `None` for a top-level item.
    #[serde(default)] pub parent_anchor: Option<String>,
}

/// A decision line found by `FrameworkAdapter::decisions`, ready to become a
/// pending memory under `import::import_decisions`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImportedDecision {
    pub text: String,
    pub source_ref: SourceRef,
}

// What `import_framework` (route, MCP tool and CLI) imports for one framework.
str_enum!(ImportWhat { Tasks => "tasks", Decisions => "decisions" });

/// The tally `import::import_tasks` and `import::import_decisions` answer with:
/// how many items were newly created, how many existing ones were refreshed, and
/// how many were left alone because nothing about them had changed (or, for a
/// decision, because its text already exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct ImportReport {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    /// How many existing tasks were given a different parent (or had one removed) by
    /// this import. Counted independently of `updated`: a task whose parent changed
    /// but whose title and description did not is still `reparented`, not `updated`.
    pub reparented: usize,
}

/// One framework's inventory plus the documents it holds, the shape
/// `GET /api/v1/projects/{id}/frameworks` and the MCP `framework_docs` tool answer
/// with for each framework detected in a project.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FrameworkListing {
    pub inventory: FrameworkInventory,
    pub documents: Vec<FrameworkDoc>,
}

// ---- plugin MCP tools (Phase 13b) ----

/// Whether a plugin tool only reads state or can change it. Mirrors `atlas-mcp`'s
/// `ToolScope`, which this crate cannot import: `atlas-mcp` depends on `atlas-core`,
/// not the other way round. Serialised the same way (`"read"` / `"write"`), so the
/// `/api/v1/mcp/status` tools table renders a plugin row's badge exactly like a
/// built-in's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PluginToolScope { Read, Write }

/// One MCP tool a desktop plugin contributes. The app registers a plugin's whole set
/// with `PUT /api/v1/mcp/plugin-tools/{plugin_id}`, where the body omits `plugin_id`
/// (the path already names it) and the daemon fills it in, which is why the field
/// defaults rather than being required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginToolDecl {
    #[serde(default)]
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    /// A JSON Schema object describing the tool's arguments, handed to MCP clients as
    /// the tool's `inputSchema` unchanged.
    pub args: serde_json::Value,
    pub scope: PluginToolScope,
}

// ---------------------------------------------------------------------------
// The agents' own MCP servers (Phase 16)
// ---------------------------------------------------------------------------

// Which agent's configuration a server was read from. `atlas` is Atlas's own MCP
// server, synthesised rather than read from a file.
str_enum!(McpServerSource {
    Claude => "claude",
    Codex => "codex",
    Cursor => "cursor",
    Gemini => "gemini",
    Windsurf => "windsurf",
    Plugin => "plugin",
    Atlas => "atlas",
});

// Which of an agent's scopes the server sits in. `user` is the agent's home-directory
// configuration; `project` is a file checked into the repository (`<root>/.mcp.json`,
// `<root>/.codex/config.toml`, `<root>/.cursor/mcp.json`); `local` is Claude Code's
// per-project block inside `~/.claude.json`, which is the user's own machine-local
// setting for one repository; `plugin` is a Claude Code plugin's bundled `.mcp.json`.
str_enum!(McpServerScope {
    User => "user",
    Project => "project",
    Local => "local",
    Plugin => "plugin",
});

/// How a server is started or reached, with secrets reduced to key names. The values of
/// `env` and `headers` stay in the file they came from: [`McpTransport`] carries only
/// `env_keys` and `header_keys`, so a listing can say what a server needs without ever
/// putting a token on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)] args: Vec<String>,
        #[serde(default)] env_keys: Vec<String>,
    },
    Http {
        url: String,
        #[serde(default)] header_keys: Vec<String>,
    },
}

/// One MCP server as a listing shows it.
///
/// `id` is `"<source>:<scope>:<name>"`, with `plugin:<marketplace>/<plugin>:<name>` for a
/// plugin server and the bare `atlas` for Atlas's own. Ids are stable across restarts, so
/// a desktop row keeps its meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpServerEntry {
    pub id: String,
    /// The key the agent's own configuration files it under.
    pub name: String,
    pub source: McpServerSource,
    pub scope: McpServerScope,
    pub transport: McpTransport,
    /// Absolute path of the configuration file this entry was read from. `None` for
    /// Atlas's own synthesised entry, which no file declares.
    pub file: Option<String>,
    /// `"<marketplace>/<plugin>"` for a plugin server, `None` otherwise.
    pub plugin: Option<String>,
    /// Whether the agent will actually start this server, as its own configuration says.
    pub enabled: bool,
    /// Whether the agent has a native switch Atlas can flip. Where it is false, `Remove`
    /// is the only way to stop a server.
    pub can_toggle: bool,
    /// Whether Atlas can delete this entry from the file it came from.
    pub can_remove: bool,
    /// Atlas's own server, which the desktop renders with its client and gating detail.
    pub is_atlas: bool,
    /// The project a `project` or `local` scoped entry belongs to.
    pub project_id: Option<Uuid>,
}

/// A server listing plus whatever discovery could not read. A warning never fails the
/// listing: one unreadable agent config must not hide every other server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpServerList {
    pub servers: Vec<McpServerEntry>,
    #[serde(default)] pub warnings: Vec<String>,
}

/// One tool a checked server reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
}

/// What starting a server and asking it for its tools found. `ok: false` carries the
/// reason in `error` rather than failing the call: a server that will not start is an
/// answer, not a broken request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpCheckResult {
    pub ok: bool,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub protocol_version: Option<String>,
    #[serde(default)] pub tools: Vec<McpToolInfo>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}

/// The transport of a server being added, with the secret values the file will hold.
/// This shape only ever travels inwards: a listing answers with [`McpTransport`], which
/// has key names and no values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum McpTransportInput {
    Stdio {
        command: String,
        #[serde(default)] args: Vec<String>,
        #[serde(default)] env: std::collections::BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)] headers: std::collections::BTreeMap<String, String>,
    },
}

/// A server to write into one agent's configuration. `project_id` is required for a
/// `project` or `local` scope; a `user` scope ignores it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewMcpServer {
    pub source: McpServerSource,
    pub scope: McpServerScope,
    #[serde(default)] pub project_id: Option<Uuid>,
    pub name: String,
    pub transport: McpTransportInput,
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
