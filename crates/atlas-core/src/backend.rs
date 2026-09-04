use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use crate::board::TaskRepo;
use crate::workflow::WorkflowRepo;
use crate::db::Db;
use crate::export::BlockContext;
use crate::jobs::{Job, JobQueue, JobRepo};
use crate::library::{AgentRepo, DocRepo};
use crate::models::*;
use crate::paths::AtlasPaths;
use crate::projects::{build_profile, detect_root, ProjectRepo};
use crate::search::global::{SearchQuery, SearchResult};
use crate::search::{FastEmbedder, NoopEmbedder};
use crate::service::MemoryService;
use crate::sync::{self, SyncInputs};
use crate::{AtlasError, Result};

/// How the managed block tells an agent to reach Atlas.
const MCP_COMMAND: &str = "atlas mcp";

/// Targets a sync writes when the request names none.
const DEFAULT_TARGETS: &[SyncKind] =
    &[SyncKind::Claude, SyncKind::Codex, SyncKind::AgentsMd, SyncKind::ClaudeMd, SyncKind::ClaudeHook, SyncKind::CodexHook, SyncKind::TasksMd];

/// Why a global sync reports a managed-block target as skipped.
const GLOBAL_SKIP: &str = "global sync writes agent files only";

/// Characters a transcript may carry. Neither `POST /ingest` nor the MCP
/// `ingest_transcript` tool is authenticated, so this bounds how much any one
/// caller can push into a single model call.
pub const MAX_INGEST_CHARS: usize = 1_000_000;

/// Where a global sync writes: `ATLAS_SYNC_HOME` when set, else the daemon
/// user's home directory. Read at call time, not at startup, so a test (or a
/// headless setup with no real home) can redirect the write. This is a
/// daemon-side env var only; the request carries no override, since `POST
/// /sync` is unauthenticated and a client-supplied path would let any local
/// caller write into an arbitrary directory.
fn sync_home() -> Result<PathBuf> {
    std::env::var_os("ATLAS_SYNC_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()))
        .ok_or_else(|| AtlasError::Other("no home directory to sync into".into()))
}

/// Guards the root of a project sync. `POST /sync` writes files, so an
/// unauthenticated caller must not be able to aim it at any directory the daemon
/// can reach: the root has to be a git repository, and neither the filesystem
/// root, the real home directory, nor the `ATLAS_SYNC_HOME` override (when set).
fn check_project_root(root: &std::path::Path) -> Result<()> {
    if root.parent().is_none() {
        return Err(AtlasError::Invalid(format!("{} is not a project root", root.display())));
    }
    let guarded_homes = [
        directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()),
        std::env::var_os("ATLAS_SYNC_HOME").map(PathBuf::from),
    ];
    if guarded_homes.into_iter().flatten().any(|h| h.canonicalize().as_deref().unwrap_or(&h) == root) {
        return Err(AtlasError::Invalid("the home directory is not a project root; use a global sync".into()));
    }
    if git2::Repository::open(root).is_err() {
        return Err(AtlasError::Invalid(format!("{} is not a git repository; sync only writes into a repository", root.display())));
    }
    Ok(())
}

/// The UUID behind a native skill's id. A discovered skill's id is
/// `<source>:<path>`, which never parses as one, so the two routes that only make
/// sense for a stored row (`update_skill`, `delete_skill`) refuse a file here rather
/// than deeper down with a vaguer message.
pub fn native_skill_id(id: &str) -> Result<Uuid> {
    id.parse().map_err(|_| AtlasError::Invalid(format!("'{id}' is not an Atlas skill; only Atlas's own skills can be edited that way")))
}

/// `ProjectOnly` needs a project to narrow to. Refusing here rather than quietly
/// listing every memory means a caller who asked for one project's own rows never gets
/// another project's back.
pub fn check_scope(project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<()> {
    if scope == MemoryScopeFilter::ProjectOnly && project_id.is_none() {
        return Err(AtlasError::Invalid("scope=project_only needs a project_id".into()));
    }
    Ok(())
}

#[async_trait::async_trait]
pub trait Backend: Send + Sync + 'static {
    async fn status(&self) -> Result<StatusReport>;

    // ---- memories ----
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory>;
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>>;
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory>;
    async fn get_memory(&self, id: Uuid) -> Result<Memory>;
    /// `scope` decides how `project_id` is read: `All` widens to that project plus the
    /// global memories, `ProjectOnly` keeps only the project's own rows. `ProjectOnly`
    /// without a `project_id` is `Invalid`: there is no project to narrow to.
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<Vec<Memory>>;
    /// Kind and tag counts, plus the total, over active memories, `project_id` read the
    /// same way [`list_memories`](Self::list_memories) reads it.
    async fn memory_facets(&self, project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<MemoryFacets>;
    async fn set_memory_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory>;

    // ---- projects ----
    async fn connect_project(&self, root: PathBuf, actor: &str) -> Result<Project>;
    async fn project_context(&self, root: PathBuf, actor: &str) -> Result<ProjectContext>;
    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn get_project(&self, id: Uuid) -> Result<Project>;
    async fn refresh_project(&self, id: Uuid) -> Result<Project>;
    async fn delete_project(&self, id: Uuid, actor: &str) -> Result<()>;
    /// Renames a project, its board key (and with it every task key on that board) or
    /// its remote. `Invalid` on a malformed or already-used board key.
    async fn update_project(&self, id: Uuid, patch: ProjectPatch, actor: &str) -> Result<Project>;
    async fn set_agent_access(&self, id: Uuid, access: AgentAccess, actor: &str) -> Result<Project>;
    /// The project's own `agent_access`, the global `access.*` defaults, and the two
    /// resolved together, for `GET /api/v1/projects/{id}/access`.
    async fn project_access(&self, id: Uuid) -> Result<ProjectAccess>;
    /// Replaces the project's extraction override, or clears it with `None`. The
    /// project comes back with the key masked.
    async fn set_project_extraction(&self, id: Uuid, over: Option<ProjectExtraction>, actor: &str) -> Result<Project>;
    async fn project_log(&self, id: Uuid, f: LogFilter) -> Result<Vec<LogEntry>>;
    /// The whole log as JSON lines, uncapped.
    async fn project_log_export(&self, id: Uuid) -> Result<String>;

    // ---- library ----
    async fn list_agents(&self) -> Result<Vec<Agent>>;
    async fn get_agent(&self, name: &str) -> Result<Agent>;
    async fn save_agent(&self, a: NewAgent, actor: &str) -> Result<Agent>;
    async fn delete_agent(&self, name: &str, actor: &str) -> Result<()>;

    async fn list_docs(&self, kind: DocKind, project_id: Option<Uuid>) -> Result<Vec<Doc>>;
    async fn get_doc(&self, kind: DocKind, name: &str) -> Result<Doc>;
    async fn save_doc(&self, kind: DocKind, d: NewDoc, actor: &str) -> Result<Doc>;
    async fn delete_doc(&self, kind: DocKind, name: &str, actor: &str) -> Result<()>;

    // ---- sync ----
    async fn sync(&self, req: SyncRequest) -> Result<SyncReport>;

    // ---- settings ----
    async fn get_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>>;
    async fn set_settings(&self, values: serde_json::Map<String, serde_json::Value>, actor: &str) -> Result<serde_json::Map<String, serde_json::Value>>;

    // ---- extraction ----
    /// Queues a transcript for the extraction worker and answers with the job id.
    /// Fails with `Conflict` when extraction is off, so nothing is queued that the
    /// worker could not run.
    async fn ingest_transcript(&self, text: String, source_tool: String, project_root: Option<PathBuf>) -> Result<Uuid>;
    async fn get_job(&self, id: Uuid) -> Result<Option<Job>>;
    /// Sends a minimal connectivity check to the model configured for `project_id`
    /// (or the global one when it is `None`) and answers with its reply, trimmed.
    /// `Conflict` when extraction is off or half configured, the same gate
    /// `ingest_transcript` checks.
    async fn test_extraction_for(&self, project_id: Option<Uuid>) -> Result<String>;

    // ---- board (Phase 6) ----
    async fn list_tasks(&self, f: TaskFilter) -> Result<Vec<Task>>;
    async fn get_task(&self, id_or_key: &str) -> Result<TaskDetail>;
    async fn create_task(&self, t: NewTask, actor: &str) -> Result<Task>;
    async fn update_task(&self, id_or_key: &str, u: TaskUpdate, actor: &str) -> Result<Task>;
    async fn move_task(&self, id_or_key: &str, stage: &str, expected: Option<DateTime<Utc>>, actor: &str) -> Result<Task>;
    async fn comment_task(&self, id_or_key: &str, body: &str, actor: &str) -> Result<TaskEvent>;
    async fn claim_task(&self, id_or_key: &str, force: bool, actor: &str) -> Result<Task>;
    async fn set_task_blockers(&self, id_or_key: &str, blocked_by: Vec<String>, actor: &str) -> Result<Task>;
    async fn delete_task(&self, id_or_key: &str, actor: &str) -> Result<()>;
    async fn board_stages(&self, project_id: Option<Uuid>) -> Result<StageList>;
    async fn set_board_stages(&self, stages: Vec<Stage>, renames: HashMap<String, String>, actor: &str) -> Result<Vec<Stage>>;
    async fn set_project_stages(&self, project_id: Uuid, stages: Option<Vec<Stage>>, renames: HashMap<String, String>, actor: &str) -> Result<StageList>;
    /// Task counts per stage, scoped the same way [`list_tasks`](Self::list_tasks)
    /// reads `project_id`/`global_only`/`top_level`: neither `project_id` nor
    /// `global_only` set counts every project, `project_id` alone counts just that
    /// project, `global_only` counts just the project-less tasks. The caller (the
    /// HTTP route) refuses `project_id` and `global_only` being set together.
    /// `top_level` narrows further to parent-less tasks (`Some(true)`) or subtasks
    /// (`Some(false)`), or applies no such filter (`None`).
    async fn task_counts(&self, project_id: Option<Uuid>, global_only: bool, top_level: Option<bool>) -> Result<Vec<(String, i64)>>;

    // ---- workflows (Phase 9) ----
    async fn list_workflows(&self, project_id: Option<Uuid>) -> Result<Vec<Workflow>>;
    async fn get_workflow(&self, id_or_name: &str) -> Result<Workflow>;
    async fn create_workflow(&self, w: NewWorkflow, actor: &str) -> Result<Workflow>;
    async fn update_workflow(&self, id_or_name: &str, patch: WorkflowPatch, actor: &str) -> Result<Workflow>;
    async fn delete_workflow(&self, id_or_name: &str, actor: &str) -> Result<()>;
    /// Creates a queued run and enqueues the job that executes it. `Conflict` when the
    /// workflow already has a run queued or running: only one run of a workflow moves
    /// at a time.
    async fn run_workflow(&self, id_or_name: &str, trigger: TriggerKind, actor: &str, input: Option<String>) -> Result<WorkflowRun>;
    async fn list_runs(&self, id_or_name: &str, limit: usize) -> Result<Vec<WorkflowRun>>;
    /// Every run, across every workflow, that finished after `since`, newest first.
    async fn runs_since(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<WorkflowRun>>;
    async fn get_run(&self, run_id: Uuid) -> Result<(WorkflowRun, Vec<WorkflowStep>)>;
    async fn cancel_run(&self, run_id: Uuid, actor: &str) -> Result<WorkflowRun>;
    /// The run's full log as plain text: a header line, then one `ts level [step] text`
    /// line per log line, across every step in order.
    async fn export_run_log(&self, run_id: Uuid) -> Result<String>;

    // ---- search ----
    async fn search(&self, q: SearchQuery) -> Result<SearchResult>;

    // ---- frameworks (Phase 12) ----
    /// Every framework detected in the project, each with the documents it holds.
    async fn list_frameworks(&self, project_id: Uuid) -> Result<Vec<FrameworkListing>>;
    /// One document's text, addressed the way `documents`/`tasks` on the adapter
    /// itself hand its path back. `Invalid` for an unknown `kind` or a path that
    /// does not exist, or that resolves outside the project root.
    async fn get_framework_doc(&self, project_id: Uuid, kind: FrameworkKind, path: &str) -> Result<String>;
    /// Imports `kind`'s tasks or decisions into the project. A write: gated the
    /// same way a direct task move or memory write is, by the project's
    /// `agent_access`.
    async fn import_framework(&self, project_id: Uuid, kind: FrameworkKind, what: ImportWhat, actor: &str) -> Result<ImportReport>;

    // ---- skills (Phase 15) ----
    /// Every skill in scope: the Atlas-native ones plus the `SKILL.md` folders found
    /// under the user's home and, when `project_id` is given, under that project's
    /// root. A project also fills each summary's `enabled_here`.
    async fn list_skills(&self, project_id: Option<Uuid>) -> Result<SkillList>;
    /// One skill with its full text. `project_id` has to name the same project the id
    /// was listed under, or a project-scoped skill will not resolve.
    async fn get_skill(&self, project_id: Option<Uuid>, id: &str) -> Result<Skill>;
    async fn create_skill(&self, s: NewSkill, actor: &str) -> Result<Skill>;
    /// Edits a native skill's name, description or body. `Invalid` for a discovered
    /// skill, which has no such fields of its own: its text is edited through
    /// [`write_skill_body`](Self::write_skill_body).
    async fn update_skill(&self, id: &str, patch: SkillUpdate, actor: &str) -> Result<Skill>;
    /// Replaces a skill's text in place: the stored body for a native skill, the whole
    /// `SKILL.md` for a discovered one. `Invalid` when the skill is not editable.
    async fn write_skill_body(&self, project_id: Option<Uuid>, id: &str, body: String, actor: &str) -> Result<Skill>;
    /// Deletes a native skill. `Invalid` for a discovered one: Atlas never removes a
    /// file it only found.
    async fn delete_skill(&self, id: &str, actor: &str) -> Result<()>;
    /// Replaces the project's disabled-skill list. Every id must name a skill that
    /// applies to the project right now.
    async fn set_project_skills_disabled(&self, project_id: Uuid, ids: Vec<String>, actor: &str) -> Result<Project>;

    // ---- the agents' MCP servers (Phase 16) ----
    /// Every MCP server the user's agents are wired to. Without `project_id` this is each
    /// agent's user-level configuration, the installed plugins' servers and Atlas; with
    /// one it is that project's own files, plus the plugins and Atlas.
    async fn list_mcp_servers(&self, project_id: Option<Uuid>) -> Result<McpServerList>;
    /// Starts the server `id` names and lists its tools, capped at 15 seconds. Runs the
    /// user's own configured command, so it is always an explicit action.
    async fn check_mcp_server(&self, project_id: Option<Uuid>, id: &str) -> Result<McpCheckResult>;
    /// Flips the agent's own enable switch. `Invalid` where the agent has none.
    async fn set_mcp_server_enabled(&self, project_id: Option<Uuid>, id: &str, enabled: bool, actor: &str) -> Result<McpServerEntry>;
    /// Writes a new server into one agent's configuration. `Conflict` when that file
    /// already holds the name.
    async fn add_mcp_server(&self, input: NewMcpServer, actor: &str) -> Result<McpServerEntry>;
    /// Deletes a server from the file it came from. `Invalid` for a plugin's or Atlas's.
    async fn remove_mcp_server(&self, project_id: Option<Uuid>, id: &str, actor: &str) -> Result<()>;

    // ---- plugin MCP tools (Phase 13b) ----
    /// Every MCP tool the running desktop plugins contribute. Defaulted to empty
    /// because most backends have no plugins behind them: only the daemon (with a
    /// `PluginToolHost` set) and `RemoteBackend` (which asks the daemon) answer
    /// otherwise.
    async fn plugin_tools(&self) -> Result<Vec<PluginToolDecl>> { Ok(vec![]) }
    /// Forwards one plugin tool call to the app that declared it. Defaulted to a
    /// refusal for the same reason.
    async fn call_plugin_tool(&self, _plugin_id: &str, _name: &str, _args: serde_json::Value, _actor: &str) -> Result<serde_json::Value> {
        Err(AtlasError::Invalid("plugin tools are not available here".into()))
    }
}

pub struct LocalBackend {
    pub memories: Arc<MemoryService>,
    pub db: Arc<Db>,
    pub paths: AtlasPaths,
    pub port: Option<u16>,
    pub jobs: Arc<JobRepo>,
    /// Wakes the daemon's worker the moment a job is queued.
    pub queue: Arc<JobQueue>,
    pub tasks: Arc<TaskRepo>,
    pub workflows: Arc<WorkflowRepo>,
    /// Whoever can answer for the desktop plugins currently running, when anyone can.
    /// Set by the daemon (`LocalBackend::with_plugin_tool_host`) to its loopback
    /// WebSocket channel; `None` everywhere else, which leaves `plugin_tools` empty and
    /// `call_plugin_tool` refusing.
    pub plugin_tool_host: Option<Arc<dyn crate::plugin_tools::PluginToolHost>>,
}

/// `ProjectRepo` for `db`, built fresh each call rather than stored: it only borrows
/// `db`, so building it inside a `blocking` closure (which owns a cloned `Arc<Db>`,
/// not a borrow of `self`) is cheaper than threading a stored repo through.
fn projects_repo(db: &Db) -> ProjectRepo<'_> { ProjectRepo::new(db) }
fn agents_repo(db: &Db) -> AgentRepo<'_> { AgentRepo::new(db) }
fn docs_repo(db: &Db, kind: DocKind) -> DocRepo<'_> { DocRepo::new(db, kind) }
fn settings_repo(db: &Db) -> crate::settings::SettingsRepo<'_> { crate::settings::SettingsRepo::new(db) }

/// Refuses an agent the project has not admitted, and says whether a memory it
/// writes has to wait for review. A project id that no longer resolves is not a
/// refusal: there is no rule to apply. Takes `db` rather than `&LocalBackend` so it
/// can run inside a `blocking` closure.
fn memory_gate(db: &Db, project_id: Option<Uuid>, actor: &str) -> Result<bool> {
    let Some(pid) = project_id.filter(|_| !crate::projects::actor_is_user(actor)) else { return Ok(false) };
    let project = projects_repo(db).get(pid)?;
    let defaults = crate::projects::access_defaults(&settings_repo(db))?;
    crate::projects::check_memory_write(actor, &project, &defaults)?;
    Ok(crate::projects::effective_access(&project.agent_access, &defaults).require_review)
}

/// Refuses an agent that may not move tasks on this task's board. The task read is
/// skipped entirely for the user's own hands, which are always exempt.
fn task_move_gate(db: &Db, tasks: &TaskRepo, id_or_key: &str, actor: &str) -> Result<()> {
    if crate::projects::actor_is_user(actor) {
        return Ok(());
    }
    let Some(pid) = tasks.get(id_or_key)?.task.project_id else { return Ok(()) };
    let project = projects_repo(db).get(pid)?;
    let defaults = crate::projects::access_defaults(&settings_repo(db))?;
    crate::projects::check_task_move(actor, &project, &defaults)
}

/// Refuses to trigger a run for an actor this project's `agent_access` would
/// refuse a direct `remember` or task-board write from — checked once, here, at
/// trigger time, against whichever of `memory_writers`/`task_movers` the output
/// node could actually exercise. Without this, an actor a project has not admitted
/// to write memories or move tasks directly could obtain the same write by routing
/// it through a workflow: the run's own writes are stamped `workflow/<name>`, which
/// is a different identity from the one this checks, and a `memory_writers`/
/// `task_movers` allowlist naming `workflow` (or the specific workflow) would
/// otherwise admit it regardless of who asked for the run. The user's own hands
/// are exempt, the same as every other gate in this file; a global workflow (no
/// project) is never gated, since there is no project's `agent_access` to check.
fn workflow_trigger_gate(db: &Db, workflow: &Workflow, actor: &str) -> Result<()> {
    let Some(pid) = workflow.project_id.filter(|_| !crate::projects::actor_is_user(actor)) else { return Ok(()) };
    let Some(output) = workflow.graph.nodes.iter().find(|n| n.kind == NodeKind::Output) else { return Ok(()) };
    let NodeData::Output { propose_memories, file_tasks } = &output.data else { return Ok(()) };
    let project = projects_repo(db).get(pid)?;
    let defaults = crate::projects::access_defaults(&settings_repo(db))?;
    if *propose_memories {
        crate::projects::check_memory_write(actor, &project, &defaults)?;
    }
    if *file_tasks {
        crate::projects::check_task_move(actor, &project, &defaults)?;
    }
    Ok(())
}

/// Deleting a project cascades to its tasks, blocker links and events, so it is a
/// board write and takes the same gate every `TaskRepo` write takes, before the
/// connection, in the order `CLAUDE.md` requires. `gate` is locked here, inside the
/// `blocking` closure that calls this, never across an await, so the guard cannot be
/// held across one. `ProjectRepo::delete` and `MemoryRepo::audit` take the gate
/// nowhere themselves, so nothing under the hold can ask for it again.
fn delete_project_gated(gate: &std::sync::Mutex<()>, db: &Db, id: Uuid, actor: &str) -> Result<()> {
    let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
    projects_repo(db).delete(id, actor)
}

/// Whether `extraction.enabled` is on. Read fresh on every call rather than
/// cached, since the daemon serves every client and a setting change must take
/// effect on the next sync or refresh, not after a restart. Shared by `sync`
/// (whether to install the transcript hooks) and `refresh_project` (whether to
/// enqueue a project summary).
fn extraction_enabled(db: &Db) -> Result<bool> {
    Ok(settings_repo(db).get_raw("extraction.enabled")?.and_then(|v| v.as_bool()) == Some(true))
}

/// Whether `board.mirror_tasks_md` is on, the same fresh-read, off-by-default
/// pattern as `extraction_enabled`. `sync` resolves it here rather than trusting
/// the request: `POST /sync` is unauthenticated.
fn mirror_tasks_md_enabled(db: &Db) -> Result<bool> {
    Ok(settings_repo(db).get_raw("board.mirror_tasks_md")?.and_then(|v| v.as_bool()) == Some(true))
}

impl LocalBackend {
    pub fn open(paths: &AtlasPaths, port: Option<u16>, load_embedder: bool) -> Result<Self> {
        paths.ensure()?;
        let db = Arc::new(Db::open(&paths.db_path())?);
        let memories = Arc::new(MemoryService::new(db.clone(), Arc::new(NoopEmbedder))?);
        if load_embedder {
            memories.set_loading(true);
            let models_dir = paths.models_dir();
            let bg = memories.clone();
            std::thread::spawn(move || {
                match FastEmbedder::try_new(&models_dir) {
                    Ok(e) => {
                        if let Err(err) = bg.set_embedder(Arc::new(e)) {
                            tracing::warn!("failed to activate embedding model: {err}");
                            bg.set_embed_error(err.to_string());
                        }
                    }
                    Err(e) => {
                        tracing::warn!("embedding model unavailable, keyword-only search: {e}");
                        bg.set_embed_error(e.to_string());
                    }
                }
                bg.set_loading(false);
            });
        }
        let tasks = Arc::new(TaskRepo::new(db.clone(), memories.gate_handle()));
        let workflows = Arc::new(WorkflowRepo::new(db.clone(), memories.gate_handle()));
        Ok(Self { jobs: Arc::new(JobRepo::new(db.clone())), queue: Arc::new(JobQueue::new()), memories, db, paths: paths.clone(), port, tasks, workflows, plugin_tool_host: None })
    }

    /// Points `plugin_tools`/`call_plugin_tool` at a live host. Called once, at daemon
    /// startup, before the backend is shared.
    pub fn with_plugin_tool_host(mut self, host: Arc<dyn crate::plugin_tools::PluginToolHost>) -> Self {
        self.plugin_tool_host = Some(host);
        self
    }

    /// Runs a synchronous body that touches the Db, the BM25 index, the vectors or
    /// the embedder on the blocking thread pool, so it never stalls the async
    /// runtime: every `async fn` on `LocalBackend` that touches those runs its body
    /// through this, and the worker and scheduler in `atlasd` (which call `jobs`,
    /// `workflows` and `db` directly rather than through the `Backend` trait) use
    /// the same helper rather than duplicating it. `f` clones whatever `Arc` state
    /// it needs out of `self` before it is built, and takes the write gate, when it
    /// needs one, inside itself — never across an await. A panic on the blocking
    /// thread (a `tokio::task::JoinError`) becomes a plain `AtlasError::Internal`.
    pub async fn blocking<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        tokio::task::spawn_blocking(f).await.map_err(|e| AtlasError::Internal(format!("blocking task failed: {e}")))?
    }
}

#[async_trait::async_trait]
impl Backend for LocalBackend {
    async fn status(&self) -> Result<StatusReport> {
        let memories = self.memories.clone();
        let paths = self.paths.clone();
        let port = self.port;
        self.blocking(move || {
            let mut s = memories.status(port)?;
            s.db_path = paths.db_path().display().to_string();
            Ok(s)
        }).await
    }
    /// The project's `agent_access` is enforced here rather than in the MCP router,
    /// because MCP reaches the daemon over HTTP and the shim cannot see the rule. An
    /// actor the project has not admitted is refused, and `require_review` turns an
    /// agent's memory into a pending one.
    async fn remember(&self, mut m: NewMemory, actor: &str) -> Result<Memory> {
        let db = self.db.clone();
        let memories = self.memories.clone();
        let actor = actor.to_string();
        self.blocking(move || {
            if memory_gate(&db, m.project_id, &actor)? {
                m.status = MemoryStatus::Pending;
            }
            memories.remember(m, &actor)
        }).await
    }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> {
        check_scope(q.project_id, q.list_scope)?;
        let memories = self.memories.clone();
        self.blocking(move || memories.recall(&q)).await
    }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> {
        let memories = self.memories.clone();
        let actor = actor.to_string();
        self.blocking(move || memories.forget(id, reason, &actor)).await
    }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> {
        let memories = self.memories.clone();
        self.blocking(move || memories.get(id)).await
    }
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<Vec<Memory>> {
        check_scope(project_id, scope)?;
        let memories = self.memories.clone();
        self.blocking(move || memories.list_scoped(status, None, project_id, scope)).await
    }
    async fn memory_facets(&self, project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<MemoryFacets> {
        check_scope(project_id, scope)?;
        let memories = self.memories.clone();
        self.blocking(move || memories.facets(project_id, scope)).await
    }
    async fn set_memory_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory> {
        let memories = self.memories.clone();
        let actor = actor.to_string();
        self.blocking(move || memories.set_status(id, status, &actor)).await
    }

    /// Detects the project at `root` and records it, building a profile when the
    /// stored one is missing or stale. The upsert runs first, without a profile, so
    /// `needs_refresh` can consult what is already stored before doing the work.
    async fn connect_project(&self, root: PathBuf, actor: &str) -> Result<Project> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || {
            let detected = detect_root(&root)?;
            let repo = projects_repo(&db);
            let project = repo.upsert(&detected, None, &actor)?;
            if repo.needs_refresh(&project) {
                let profile = build_profile(&detected.root)?;
                return repo.set_profile(project.id, &profile, &actor);
            }
            Ok(project)
        }).await
    }

    async fn project_context(&self, root: PathBuf, actor: &str) -> Result<ProjectContext> {
        let project = self.connect_project(root, actor).await?;
        let memories = self.memories.clone();
        let db = self.db.clone();
        let workflows = self.workflows.clone();
        let skills_home = self.paths.skills_home.clone();
        self.blocking(move || {
            let query = match &project.profile {
                Some(p) => format!("{} {}", p.name, p.frameworks.join(" ")),
                None => project.name.clone(),
            };
            let mut hits = memories.recall(&RecallQuery {
                query, limit: 20, scope: None, list_scope: MemoryScopeFilter::All, project_id: Some(project.id), kinds: vec![], tags: vec![],
            })?;
            // Recall is a search, so a project whose memories don't happen to match its own
            // name would come back empty. Top it up with the newest project-scoped memories
            // (score 0.0: they were not ranked, they were appended) so context is never bare.
            let seen: std::collections::HashSet<Uuid> = hits.iter().map(|h| h.memory.id).collect();
            let recent = memories.list(MemoryStatus::Active, Some(MemoryScope::Project), Some(project.id))?;
            hits.extend(recent.into_iter().filter(|m| !seen.contains(&m.id)).take(10).map(|memory| RecallHit { memory, score: 0.0 }));
            // The skills an agent may actually use here, so the context says what is in
            // play rather than everything that exists. Discovery reads directories, so a
            // failure to read one must not cost the caller its whole context: an error
            // leaves the list empty rather than failing the call.
            let skills = crate::skills::list_skills(&db, Some(&project), &skills_home)
                .map(|l| l.skills.into_iter().filter(|s| s.enabled_here != Some(false)).collect())
                .unwrap_or_else(|e| {
                    tracing::warn!("skills unavailable for project context: {e}");
                    vec![]
                });
            Ok(ProjectContext {
                practices: docs_repo(&db, DocKind::Practice).list(Some(project.id))?,
                workflows: workflows.list(Some(project.id))?.iter().map(WorkflowSummary::from).collect(),
                skills,
                project,
                memories: hits,
            })
        }).await
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let db = self.db.clone();
        self.blocking(move || projects_repo(&db).list()).await
    }
    async fn get_project(&self, id: Uuid) -> Result<Project> {
        let db = self.db.clone();
        self.blocking(move || projects_repo(&db).get(id)).await
    }
    async fn refresh_project(&self, id: Uuid) -> Result<Project> {
        let db = self.db.clone();
        let gate = self.memories.gate_handle();
        let jobs = self.jobs.clone();
        let (updated, should_enqueue) = self.blocking(move || {
            let project = projects_repo(&db).get(id)?;
            let profile = build_profile(std::path::Path::new(&project.root_path))?;
            // Gated because the summary job also writes this profile: it reads one, asks
            // the model, then re-reads and writes under the same gate. Without a gate
            // shared by both writers the two interleave and one of them loses its half of
            // the profile. The repository scan above stays outside the gate.
            let updated = {
                let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
                projects_repo(&db).set_profile(id, &profile, "refresh")?
            };
            // A fresh profile carries no summary until the worker writes one; queue that
            // only when extraction is on, the same gate `ingest_transcript` checks.
            let should_enqueue = extraction_enabled(&db)?;
            if should_enqueue {
                jobs.enqueue("project_summary", serde_json::json!({"project_id": updated.id}))?;
            }
            Ok((updated, should_enqueue))
        }).await?;
        if should_enqueue {
            self.queue.notify.notify_one();
        }
        Ok(updated)
    }
    async fn delete_project(&self, id: Uuid, actor: &str) -> Result<()> {
        let db = self.db.clone();
        let gate = self.memories.gate_handle();
        let actor = actor.to_string();
        self.blocking(move || delete_project_gated(&gate, &db, id, &actor)).await
    }

    /// Gated: a board key rename rewrites every `tasks.key` on the board, which is a
    /// board write and takes the same mutex in the same order as every other one.
    /// The guard is taken inside the closure, never across an await.
    async fn update_project(&self, id: Uuid, patch: ProjectPatch, actor: &str) -> Result<Project> {
        let db = self.db.clone();
        let gate = self.memories.gate_handle();
        let actor = actor.to_string();
        self.blocking(move || {
            let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
            projects_repo(&db).update(id, &patch, &actor)
        }).await
    }
    async fn set_agent_access(&self, id: Uuid, access: AgentAccess, actor: &str) -> Result<Project> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || projects_repo(&db).set_agent_access(id, &access, &actor)).await
    }
    async fn project_access(&self, id: Uuid) -> Result<ProjectAccess> {
        let db = self.db.clone();
        self.blocking(move || {
            let access = projects_repo(&db).get(id)?.agent_access;
            let defaults = crate::projects::access_defaults(&settings_repo(&db))?;
            let effective = crate::projects::effective_access(&access, &defaults);
            Ok(ProjectAccess { access, defaults, effective })
        }).await
    }
    async fn set_project_extraction(&self, id: Uuid, over: Option<ProjectExtraction>, actor: &str) -> Result<Project> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || projects_repo(&db).set_project_extraction(id, over, &actor)).await
    }
    async fn project_log(&self, id: Uuid, f: LogFilter) -> Result<Vec<LogEntry>> {
        let db = self.db.clone();
        self.blocking(move || crate::projects::log::project_log(&db, id, &f)).await
    }
    async fn project_log_export(&self, id: Uuid) -> Result<String> {
        let db = self.db.clone();
        self.blocking(move || crate::projects::log::project_log_export(&db, id)).await
    }

    async fn list_agents(&self) -> Result<Vec<Agent>> {
        let db = self.db.clone();
        self.blocking(move || agents_repo(&db).list()).await
    }
    async fn get_agent(&self, name: &str) -> Result<Agent> {
        let db = self.db.clone();
        let name = name.to_string();
        self.blocking(move || agents_repo(&db).get(&name)).await
    }
    async fn save_agent(&self, a: NewAgent, actor: &str) -> Result<Agent> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || agents_repo(&db).save(&a, &actor)).await
    }
    async fn delete_agent(&self, name: &str, actor: &str) -> Result<()> {
        let db = self.db.clone();
        let name = name.to_string();
        let actor = actor.to_string();
        self.blocking(move || agents_repo(&db).delete(&name, &actor)).await
    }

    async fn list_docs(&self, kind: DocKind, project_id: Option<Uuid>) -> Result<Vec<Doc>> {
        let db = self.db.clone();
        self.blocking(move || docs_repo(&db, kind).list(project_id)).await
    }
    async fn get_doc(&self, kind: DocKind, name: &str) -> Result<Doc> {
        let db = self.db.clone();
        let name = name.to_string();
        self.blocking(move || docs_repo(&db, kind).get(&name)).await
    }
    async fn save_doc(&self, kind: DocKind, d: NewDoc, actor: &str) -> Result<Doc> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || docs_repo(&db, kind).save(&d, &actor)).await
    }
    async fn delete_doc(&self, kind: DocKind, name: &str, actor: &str) -> Result<()> {
        let db = self.db.clone();
        let name = name.to_string();
        let actor = actor.to_string();
        self.blocking(move || docs_repo(&db, kind).delete(&name, &actor)).await
    }

    /// Plans the sync on the daemon host and, unless `check_only`, writes it. Every
    /// step that touches the Db runs through `blocking`; `detect_root`,
    /// `check_project_root`, `sync::plan_sync` and `sync::apply` do filesystem and git
    /// work, not Db work, so they stay off the blocking helper.
    async fn sync(&self, req: SyncRequest) -> Result<SyncReport> {
        let db = self.db.clone();
        let agents = self.blocking({ let db = db.clone(); move || agents_repo(&db).list() }).await?;
        let requested = if req.targets.is_empty() { DEFAULT_TARGETS.to_vec() } else { req.targets.clone() };
        let (root, targets, block, skipped, project_id) = if req.global {
            let home = sync_home()?;
            // Home is not a project, so only the agent exporters apply: splicing a managed
            // block into ~/AGENTS.md would name practices and a project that aren't there.
            // The dropped targets are still reported, so a caller who asked for one is told
            // why nothing was written for it instead of reading a silent success.
            let (targets, filtered): (Vec<SyncKind>, Vec<SyncKind>) = requested
                .into_iter()
                .partition(|t| matches!(t, SyncKind::Claude | SyncKind::Codex | SyncKind::ClaudeHook | SyncKind::CodexHook));
            let skipped = filtered
                .into_iter()
                .map(|kind| SyncOp {
                    path: home.join(match kind {
                        SyncKind::AgentsMd => "AGENTS.md",
                        SyncKind::TasksMd => "TASKS.md",
                        _ => "CLAUDE.md",
                    }),
                    kind,
                    content: String::new(),
                    action: SyncAction::Skip(GLOBAL_SKIP.into()),
                })
                .collect();
            let block = BlockContext { mcp_command: MCP_COMMAND.into(), agents: agents.clone(), practices: vec![], project_name: None };
            (home, targets, block, skipped, None)
        } else {
            let requested_root = req.root.clone().ok_or_else(|| AtlasError::Invalid("sync requires a root unless global is set".into()))?;
            // Resolve and vet the root before anything is written or recorded: the route is
            // unauthenticated, so an arbitrary directory must not become a project row.
            let detected = detect_root(&requested_root)?;
            check_project_root(&detected.root)?;
            // Connect rather than look up, so a sync into a fresh checkout still names the
            // project in the block instead of silently writing an anonymous one; the root
            // is either already known to `ProjectRepo` or becomes known right here.
            let project = self.connect_project(detected.root, "sync").await?;
            let pid = project.id;
            let practices = self.blocking({ let db = db.clone(); move || docs_repo(&db, DocKind::Practice).list(Some(pid)) }).await?;
            let block = BlockContext { mcp_command: MCP_COMMAND.into(), agents: agents.clone(), practices, project_name: Some(project.name.clone()) };
            (PathBuf::from(project.root_path), requested, block, vec![], Some(project.id))
        };
        // The hooks feed transcripts to a model, so they are installed only once the
        // user has switched extraction on. The daemon resolves that here rather than
        // trusting the request: `POST /sync` is unauthenticated.
        let hooks = self.blocking({ let db = db.clone(); move || extraction_enabled(&db) }).await?;
        // Codex keeps one config file per user, so its hook needs the sync home even
        // when the pass writes into a project. `CodexHook` is one of the defaults, so
        // with extraction on this resolves on nearly every sync, and a machine with no
        // home to find fails the whole pass rather than just the hook.
        let home = if hooks && targets.contains(&SyncKind::CodexHook) { sync_home()? } else { PathBuf::new() };
        // The mirror is a per-project file, so a global sync (no project) never fetches
        // board data for it, even when the setting is on. Fetched only when the setting
        // is on, so an ordinary sync with the mirror off does not pay for two extra
        // queries it will not use.
        let mirror_tasks_md_setting = self.blocking({ let db = db.clone(); move || mirror_tasks_md_enabled(&db) }).await?;
        let mirror_tasks_md = mirror_tasks_md_setting && project_id.is_some();
        let (board_stages, board_tasks) = if let Some(project_id) = project_id.filter(|_| mirror_tasks_md) {
            let stages = self.board_stages(Some(project_id)).await?.stages;
            let tasks = self.list_tasks(TaskFilter { project_id: Some(project_id), include_done: true, ..Default::default() }).await?;
            (stages, tasks)
        } else {
            (Vec::new(), Vec::new())
        };
        let mut ops = sync::plan_sync(&SyncInputs {
            root: &root,
            agents: &agents,
            block,
            targets: &targets,
            home: &home,
            hooks,
            global: req.global,
            mirror_tasks_md,
            board_stages: &board_stages,
            board_tasks: &board_tasks,
        })?;
        ops.extend(skipped);
        if req.check_only {
            return Ok(sync::summarize(&ops));
        }
        let report = sync::apply(&ops)?;
        // The project log reads this row as its `synced` entry. A check-only pass writes
        // nothing and so records nothing, and a global sync belongs to no project.
        if let Some(project_id) = project_id {
            let memories = self.memories.clone();
            let (created, updated, unchanged, skipped_n) = (report.created, report.updated, report.unchanged, report.skipped);
            self.blocking(move || {
                memories.audit(
                    "sync",
                    "apply",
                    "sync",
                    Some(project_id),
                    serde_json::json!({"created": created, "updated": updated, "unchanged": unchanged, "skipped": skipped_n}),
                )
            }).await?;
        }
        Ok(report)
    }

    async fn get_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>> {
        let db = self.db.clone();
        self.blocking(move || settings_repo(&db).get_all()).await
    }
    async fn set_settings(&self, values: serde_json::Map<String, serde_json::Value>, actor: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || {
            let cleared = settings_repo(&db).set_many(&values, &actor)?;
            let mut out = settings_repo(&db).get_all()?;
            // Not one of `SETTING_KEYS`: a one-off flag telling the caller that pointing the
            // base url somewhere new dropped the key it was entered against, so the endpoint
            // it just named will not receive it.
            if cleared {
                out.insert("extraction.api_key_cleared".into(), serde_json::Value::Bool(true));
            }
            Ok(out)
        }).await
    }

    /// Every check a transcript has to pass lives here rather than in the HTTP
    /// handler, because MCP reaches this same method and is just as unauthenticated:
    /// the enable gate, so a caller who has not configured extraction is told straight
    /// away instead of getting a job id for work that will only fail later; a blank
    /// transcript, which would spend a model call on nothing; and the character cap,
    /// which bounds how much any one caller can push into a single model call.
    async fn ingest_transcript(&self, text: String, source_tool: String, project_root: Option<PathBuf>) -> Result<Uuid> {
        let db = self.db.clone();
        let jobs = self.jobs.clone();
        let id = self.blocking(move || {
            // Resolved once, here, through the same `project_for` the worker used to call:
            // a root inside a repository has to mean the same project at the gate as it does
            // when the job runs, or the access check and the extraction override both look at
            // the wrong scope. The answer travels in the payload so the worker never has to
            // ask again.
            let project_id = crate::extract::project_for(&db, project_root.clone())?;
            memory_gate(&db, project_id, &source_tool)?;
            crate::extract::resolve_extraction(&db, project_id)?;
            if text.trim().is_empty() {
                return Err(AtlasError::Invalid("ingest text is empty".into()));
            }
            if text.chars().count() > MAX_INGEST_CHARS {
                return Err(AtlasError::TooLarge("transcript too large".into()));
            }
            jobs.enqueue("ingest", serde_json::json!({
                "text": text, "source_tool": source_tool, "project_root": project_root, "project_id": project_id,
            }))
        }).await?;
        self.queue.notify.notify_one();
        Ok(id)
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<Job>> {
        let jobs = self.jobs.clone();
        self.blocking(move || jobs.get(id)).await
    }

    /// `resolve_extraction` reads the model config from the Db and runs on the
    /// blocking helper; `client.chat` is a real network call, so it stays a plain
    /// await rather than moving onto a blocking thread.
    async fn test_extraction_for(&self, project_id: Option<Uuid>) -> Result<String> {
        let db = self.db.clone();
        let cfg = self.blocking(move || crate::extract::resolve_extraction(&db, project_id)).await?;
        let client = crate::extract::build_client(&cfg)?;
        let reply = client.chat("You are a connectivity check.", "Reply with the single word OK").await?;
        Ok(reply.trim().to_string())
    }

    async fn list_tasks(&self, f: TaskFilter) -> Result<Vec<Task>> {
        let tasks = self.tasks.clone();
        self.blocking(move || tasks.list(&f)).await
    }
    async fn get_task(&self, id_or_key: &str) -> Result<TaskDetail> {
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        self.blocking(move || tasks.get(&id_or_key)).await
    }
    async fn create_task(&self, t: NewTask, actor: &str) -> Result<Task> {
        let tasks = self.tasks.clone();
        let actor = actor.to_string();
        self.blocking(move || tasks.create(&t, &actor)).await
    }
    async fn update_task(&self, id_or_key: &str, u: TaskUpdate, actor: &str) -> Result<Task> {
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        let actor = actor.to_string();
        self.blocking(move || tasks.update(&id_or_key, &u, &actor)).await
    }
    async fn move_task(&self, id_or_key: &str, stage: &str, expected: Option<DateTime<Utc>>, actor: &str) -> Result<Task> {
        let db = self.db.clone();
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        let stage = stage.to_string();
        let actor = actor.to_string();
        self.blocking(move || {
            task_move_gate(&db, &tasks, &id_or_key, &actor)?;
            tasks.move_stage(&id_or_key, &stage, expected, &actor)
        }).await
    }
    async fn comment_task(&self, id_or_key: &str, body: &str, actor: &str) -> Result<TaskEvent> {
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        let body = body.to_string();
        let actor = actor.to_string();
        self.blocking(move || tasks.comment(&id_or_key, &body, &actor)).await
    }
    async fn claim_task(&self, id_or_key: &str, force: bool, actor: &str) -> Result<Task> {
        // A claim moves the task out of the first stage, so it is a move.
        let db = self.db.clone();
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        let actor = actor.to_string();
        self.blocking(move || {
            task_move_gate(&db, &tasks, &id_or_key, &actor)?;
            tasks.claim(&id_or_key, force, &actor)
        }).await
    }
    async fn set_task_blockers(&self, id_or_key: &str, blocked_by: Vec<String>, actor: &str) -> Result<Task> {
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        let actor = actor.to_string();
        self.blocking(move || tasks.set_blockers(&id_or_key, blocked_by, &actor)).await
    }
    async fn delete_task(&self, id_or_key: &str, actor: &str) -> Result<()> {
        let tasks = self.tasks.clone();
        let id_or_key = id_or_key.to_string();
        let actor = actor.to_string();
        self.blocking(move || tasks.delete(&id_or_key, &actor)).await
    }
    async fn board_stages(&self, project_id: Option<Uuid>) -> Result<StageList> {
        let tasks = self.tasks.clone();
        self.blocking(move || tasks.effective_stages(project_id)).await
    }
    async fn set_board_stages(&self, stages: Vec<Stage>, renames: HashMap<String, String>, actor: &str) -> Result<Vec<Stage>> {
        let tasks = self.tasks.clone();
        let actor = actor.to_string();
        self.blocking(move || tasks.set_global_stages(stages, &renames, &actor)).await
    }
    async fn set_project_stages(&self, project_id: Uuid, stages: Option<Vec<Stage>>, renames: HashMap<String, String>, actor: &str) -> Result<StageList> {
        let tasks = self.tasks.clone();
        let actor = actor.to_string();
        self.blocking(move || tasks.set_project_stages(project_id, stages, &renames, &actor)).await
    }
    async fn task_counts(&self, project_id: Option<Uuid>, global_only: bool, top_level: Option<bool>) -> Result<Vec<(String, i64)>> {
        let tasks = self.tasks.clone();
        self.blocking(move || tasks.counts_by_stage(project_id, global_only, top_level)).await
    }

    async fn list_workflows(&self, project_id: Option<Uuid>) -> Result<Vec<Workflow>> {
        let workflows = self.workflows.clone();
        self.blocking(move || workflows.list(project_id)).await
    }
    async fn get_workflow(&self, id_or_name: &str) -> Result<Workflow> {
        let workflows = self.workflows.clone();
        let id_or_name = id_or_name.to_string();
        self.blocking(move || workflows.get(&id_or_name)).await
    }
    async fn create_workflow(&self, w: NewWorkflow, actor: &str) -> Result<Workflow> {
        let workflows = self.workflows.clone();
        let actor = actor.to_string();
        self.blocking(move || workflows.create(&w, &actor)).await
    }
    async fn update_workflow(&self, id_or_name: &str, patch: WorkflowPatch, actor: &str) -> Result<Workflow> {
        let workflows = self.workflows.clone();
        let id_or_name = id_or_name.to_string();
        let actor = actor.to_string();
        self.blocking(move || workflows.update(&id_or_name, &patch, &actor)).await
    }
    async fn delete_workflow(&self, id_or_name: &str, actor: &str) -> Result<()> {
        let workflows = self.workflows.clone();
        let id_or_name = id_or_name.to_string();
        let actor = actor.to_string();
        self.blocking(move || workflows.delete(&id_or_name, &actor)).await
    }
    async fn run_workflow(&self, id_or_name: &str, trigger: TriggerKind, actor: &str, input: Option<String>) -> Result<WorkflowRun> {
        let db = self.db.clone();
        let workflows = self.workflows.clone();
        let jobs = self.jobs.clone();
        let id_or_name = id_or_name.to_string();
        let actor = actor.to_string();
        let run = self.blocking(move || {
            let workflow = workflows.get(&id_or_name)?;
            workflow_trigger_gate(&db, &workflow, &actor)?;
            if workflows.has_pending_run(workflow.id)? {
                return Err(AtlasError::Conflict(format!("workflow '{}' already has a run queued or running", workflow.name)));
            }
            let run = workflows.create_run(workflow.id, trigger)?;
            jobs.enqueue(
                "workflow_run",
                serde_json::json!({"workflow_id": workflow.id, "run_id": run.id, "trigger": trigger.as_str(), "actor": actor, "input": input}),
            )?;
            Ok(run)
        }).await?;
        self.queue.notify.notify_one();
        Ok(run)
    }
    async fn list_runs(&self, id_or_name: &str, limit: usize) -> Result<Vec<WorkflowRun>> {
        let workflows = self.workflows.clone();
        let id_or_name = id_or_name.to_string();
        self.blocking(move || {
            let workflow = workflows.get(&id_or_name)?;
            workflows.list_runs(workflow.id, limit)
        }).await
    }
    async fn runs_since(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<WorkflowRun>> {
        let workflows = self.workflows.clone();
        self.blocking(move || workflows.runs_since(since, limit)).await
    }
    async fn get_run(&self, run_id: Uuid) -> Result<(WorkflowRun, Vec<WorkflowStep>)> {
        let workflows = self.workflows.clone();
        self.blocking(move || workflows.get_run(run_id)).await
    }
    async fn cancel_run(&self, run_id: Uuid, actor: &str) -> Result<WorkflowRun> {
        let workflows = self.workflows.clone();
        let actor = actor.to_string();
        self.blocking(move || workflows.cancel_run(run_id, &actor)).await
    }
    async fn export_run_log(&self, run_id: Uuid) -> Result<String> {
        let workflows = self.workflows.clone();
        self.blocking(move || {
            let (run, steps) = workflows.get_run(run_id)?;
            let mut out = format!("workflow run {} #{} — {}\n", run.id, run.number, run.status);
            for step in &steps {
                for line in &step.log {
                    out.push_str(&format!("{} {} [{}] {}\n", line.ts.to_rfc3339(), line.level, step.name, line.text));
                }
            }
            Ok(out)
        }).await
    }

    async fn search(&self, q: SearchQuery) -> Result<SearchResult> {
        let db = self.db.clone();
        let tasks = self.tasks.clone();
        let workflows = self.workflows.clone();
        self.blocking(move || {
            let memories = crate::memories::MemoryRepo::new(&db);
            let projects = projects_repo(&db);
            crate::search::global::search(&q, &tasks, &memories, &projects, &workflows)
        }).await
    }

    async fn list_frameworks(&self, project_id: Uuid) -> Result<Vec<FrameworkListing>> {
        let db = self.db.clone();
        self.blocking(move || {
            let project = projects_repo(&db).get(project_id)?;
            let root = std::path::Path::new(&project.root_path);
            let mut out = Vec::new();
            for adapter in crate::frameworks::adapters() {
                if let Some(inventory) = adapter.detect(root) {
                    let documents = adapter.documents(root);
                    out.push(FrameworkListing { inventory, documents });
                }
            }
            Ok(out)
        }).await
    }
    async fn get_framework_doc(&self, project_id: Uuid, kind: FrameworkKind, path: &str) -> Result<String> {
        let db = self.db.clone();
        let path = path.to_string();
        self.blocking(move || {
            let project = projects_repo(&db).get(project_id)?;
            let root = std::path::Path::new(&project.root_path);
            let adapter = crate::frameworks::adapters()
                .into_iter()
                .find(|a| a.kind() == kind)
                .ok_or_else(|| AtlasError::Invalid(format!("unknown framework: {kind}")))?;
            adapter.read(root, &path)
        }).await
    }
    /// Gated by the write the import actually performs: a task import needs
    /// `task_movers` (it files board tasks), a decision import needs
    /// `memory_writers` (it files memories). The user's own hands, and the actors
    /// `check_task_move`/`check_memory_write` already exempt, pass either way.
    async fn import_framework(&self, project_id: Uuid, kind: FrameworkKind, what: ImportWhat, actor: &str) -> Result<ImportReport> {
        let db = self.db.clone();
        let tasks = self.tasks.clone();
        let actor = actor.to_string();
        self.blocking(move || {
            let project = projects_repo(&db).get(project_id)?;
            let defaults = crate::projects::access_defaults(&settings_repo(&db))?;
            let memories = crate::memories::MemoryRepo::new(&db);
            match what {
                ImportWhat::Tasks => {
                    crate::projects::check_task_move(&actor, &project, &defaults)?;
                    crate::frameworks::import::import_tasks(&tasks, &memories, &project, kind, &actor)
                }
                ImportWhat::Decisions => {
                    crate::projects::check_memory_write(&actor, &project, &defaults)?;
                    crate::frameworks::import::import_decisions(&memories, &project, kind, &actor)
                }
            }
        }).await
    }

    // ---- skills (Phase 15) ----

    async fn list_skills(&self, project_id: Option<Uuid>) -> Result<SkillList> {
        let db = self.db.clone();
        let home = self.paths.skills_home.clone();
        self.blocking(move || {
            let project = project_id.map(|id| projects_repo(&db).get(id)).transpose()?;
            crate::skills::list_skills(&db, project.as_ref(), &home)
        }).await
    }
    async fn get_skill(&self, project_id: Option<Uuid>, id: &str) -> Result<Skill> {
        let db = self.db.clone();
        let id = id.to_string();
        let home = self.paths.skills_home.clone();
        self.blocking(move || {
            let project = project_id.map(|id| projects_repo(&db).get(id)).transpose()?;
            crate::skills::get_skill(&db, project.as_ref(), &id, &home)
        }).await
    }
    async fn create_skill(&self, s: NewSkill, actor: &str) -> Result<Skill> {
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || crate::skills::repo::SkillRepo::new(&db).create(&s, &actor)).await
    }
    /// Native only: `id` has to parse as a UUID, which no discovered skill's id does
    /// (they all carry their source and a colon), so a caller cannot reach a file
    /// through this route.
    async fn update_skill(&self, id: &str, patch: SkillUpdate, actor: &str) -> Result<Skill> {
        let uuid = native_skill_id(id)?;
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || crate::skills::repo::SkillRepo::new(&db).update(uuid, &patch, &actor)).await
    }
    async fn write_skill_body(&self, project_id: Option<Uuid>, id: &str, body: String, actor: &str) -> Result<Skill> {
        let db = self.db.clone();
        let id = id.to_string();
        let actor = actor.to_string();
        let home = self.paths.skills_home.clone();
        self.blocking(move || {
            let project = project_id.map(|id| projects_repo(&db).get(id)).transpose()?;
            crate::skills::write_skill_body(&db, project.as_ref(), &id, body, &actor, &home)
        }).await
    }
    async fn delete_skill(&self, id: &str, actor: &str) -> Result<()> {
        let uuid = native_skill_id(id)?;
        let db = self.db.clone();
        let actor = actor.to_string();
        self.blocking(move || crate::skills::repo::SkillRepo::new(&db).delete(uuid, &actor)).await
    }
    async fn set_project_skills_disabled(&self, project_id: Uuid, ids: Vec<String>, actor: &str) -> Result<Project> {
        let db = self.db.clone();
        let actor = actor.to_string();
        let home = self.paths.skills_home.clone();
        self.blocking(move || crate::skills::set_project_skills_disabled(&db, project_id, ids, &actor, &home)).await
    }

    // ---- the agents' MCP servers (Phase 16) ----

    async fn list_mcp_servers(&self, project_id: Option<Uuid>) -> Result<McpServerList> {
        let db = self.db.clone();
        let home = self.paths.agent_home().to_path_buf();
        self.blocking(move || {
            let project = project_id.map(|id| projects_repo(&db).get(id)).transpose()?;
            Ok(crate::mcp_servers::list_mcp_servers(&home, project.as_ref()))
        })
        .await
    }
    /// The one call here that is not blocking work: it spawns a process or opens a
    /// connection and waits on it, so it runs on the async runtime and does its own
    /// discovery on a blocking thread.
    async fn check_mcp_server(&self, project_id: Option<Uuid>, id: &str) -> Result<McpCheckResult> {
        let project = match project_id {
            None => None,
            Some(id) => {
                let db = self.db.clone();
                Some(self.blocking(move || projects_repo(&db).get(id)).await?)
            }
        };
        crate::mcp_servers::check_mcp_server(self.paths.agent_home(), project.as_ref(), id).await
    }
    async fn set_mcp_server_enabled(&self, project_id: Option<Uuid>, id: &str, enabled: bool, actor: &str) -> Result<McpServerEntry> {
        let db = self.db.clone();
        let paths = self.paths.clone();
        let (id, actor) = (id.to_string(), actor.to_string());
        self.blocking(move || {
            let project = project_id.map(|p| projects_repo(&db).get(p)).transpose()?;
            crate::mcp_servers::set_mcp_server_enabled(&paths, &db, project.as_ref(), &id, enabled, &actor)
        })
        .await
    }
    async fn add_mcp_server(&self, input: NewMcpServer, actor: &str) -> Result<McpServerEntry> {
        let db = self.db.clone();
        let paths = self.paths.clone();
        let actor = actor.to_string();
        self.blocking(move || {
            let project = input.project_id.map(|p| projects_repo(&db).get(p)).transpose()?;
            crate::mcp_servers::add_mcp_server(&paths, &db, project.as_ref(), &input, &actor)
        })
        .await
    }
    async fn remove_mcp_server(&self, project_id: Option<Uuid>, id: &str, actor: &str) -> Result<()> {
        let db = self.db.clone();
        let paths = self.paths.clone();
        let (id, actor) = (id.to_string(), actor.to_string());
        self.blocking(move || {
            let project = project_id.map(|p| projects_repo(&db).get(p)).transpose()?;
            crate::mcp_servers::remove_mcp_server(&paths, &db, project.as_ref(), &id, &actor)
        })
        .await
    }

    async fn plugin_tools(&self) -> Result<Vec<PluginToolDecl>> {
        Ok(self.plugin_tool_host.as_ref().map(|h| h.list()).unwrap_or_default())
    }

    /// Forwards the call and records it either way. The audit row is written after the
    /// answer, not before, so `ok` tells the truth; a failure to write it does not
    /// swallow the plugin's result, since the caller asked for the tool, not the row.
    async fn call_plugin_tool(&self, plugin_id: &str, name: &str, args: serde_json::Value, actor: &str) -> Result<serde_json::Value> {
        let Some(host) = self.plugin_tool_host.clone() else {
            return Err(AtlasError::Invalid(format!("plugin {plugin_id} is not running")));
        };
        let out = host.call(plugin_id, name, args).await;
        let memories = self.memories.clone();
        let detail = serde_json::json!({ "plugin_id": plugin_id, "tool": name, "ok": out.is_ok() });
        let actor = actor.to_string();
        if let Err(e) = self.blocking(move || memories.audit(&actor, "plugin_tool_call", "plugin_tool", None, detail)).await {
            tracing::warn!("failed to audit a plugin tool call: {e}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn local_backend_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::AtlasPaths::at(dir.path());
        let b = LocalBackend::open(&paths, Some(1), false).unwrap();
        let m = b.remember(crate::models::NewMemory { scope: crate::models::MemoryScope::Global, project_id: None, kind: crate::models::MemoryKind::Fact, text: "bun is the runtime".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: crate::models::MemoryStatus::Active }, "t").await.unwrap();
        let hits = b.recall(crate::models::RecallQuery { query: "runtime".into(), limit: 5, scope: None, list_scope: MemoryScopeFilter::All, project_id: None, kinds: vec![], tags: vec![] }).await.unwrap();
        assert_eq!(hits[0].memory.id, m.id);
        let st = b.status().await.unwrap();
        assert!(st.db_path.ends_with("atlas.duckdb"));
        assert_eq!(st.port, Some(1));
    }

    #[tokio::test]
    async fn open_returns_before_embedder_loads() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::AtlasPaths::at(dir.path());
        // load_embedder = false must not spawn the background download thread, so status
        // should reflect the (permanent) unavailable state, never the transient "loading" one.
        let b = LocalBackend::open(&paths, None, false).unwrap();
        let st = b.status().await.unwrap();
        assert!(st.embedding.starts_with("unavailable"), "expected unavailable, got {}", st.embedding);
        assert_ne!(st.embedding, "loading");
    }

    /// Connecting a project whose root holds a Superpowers tree should come back
    /// with `planning_frameworks` filled in, on a fresh (temp) `ATLAS_HOME`, the
    /// same path `atlas` itself takes on a real connect.
    #[tokio::test]
    async fn connect_project_fills_planning_frameworks() {
        let home = tempfile::tempdir().unwrap();
        let paths = crate::paths::AtlasPaths::at(home.path());
        let b = LocalBackend::open(&paths, None, false).unwrap();

        let project_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_root.path().join("docs/superpowers/plans")).unwrap();
        std::fs::write(
            project_root.path().join("docs/superpowers/plans/2026-01-01-fixture.md"),
            "# Fixture plan\n\n### Task 1: Do the thing\n\n- [ ] do it\n",
        )
        .unwrap();

        let project = b.connect_project(project_root.path().to_path_buf(), "test").await.unwrap();
        let profile = project.profile.expect("connect should have built a profile");
        assert_eq!(profile.planning_frameworks.len(), 1);
        assert_eq!(profile.planning_frameworks[0].kind, crate::models::FrameworkKind::Superpowers);
        assert_eq!(profile.planning_frameworks[0].tasks, 1);
    }
}
