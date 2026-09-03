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
    async fn task_counts(&self, project_id: Option<Uuid>) -> Result<Vec<(String, i64)>>;

    // ---- search ----
    async fn search(&self, q: SearchQuery) -> Result<SearchResult>;
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
        Ok(Self { jobs: Arc::new(JobRepo::new(db.clone())), queue: Arc::new(JobQueue::new()), memories, db, paths: paths.clone(), port, tasks, workflows })
    }

    fn projects(&self) -> ProjectRepo<'_> { ProjectRepo::new(&self.db) }

    /// Deleting a project cascades to its tasks, blocker links and events, so it is a
    /// board write and takes the same gate every `TaskRepo` write takes, before the
    /// connection, in the order `CLAUDE.md` requires. Kept synchronous so the guard
    /// cannot be held across an await. `ProjectRepo::delete` and `MemoryRepo::audit`
    /// take the gate nowhere themselves, so nothing under the hold can ask for it again.
    fn delete_project_gated(&self, id: Uuid, actor: &str) -> Result<()> {
        let gate = self.memories.gate_handle();
        let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
        self.projects().delete(id, actor)
    }
    /// Refuses an agent the project has not admitted, and says whether a memory it
    /// writes has to wait for review. A project id that no longer resolves is not a
    /// refusal: there is no rule to apply.
    fn memory_gate(&self, project_id: Option<Uuid>, actor: &str) -> Result<bool> {
        let Some(pid) = project_id.filter(|_| !crate::projects::actor_is_user(actor)) else { return Ok(false) };
        let project = self.projects().get(pid)?;
        crate::projects::check_memory_write(actor, &project)?;
        Ok(project.agent_access.require_review)
    }

    /// Refuses an agent that may not move tasks on this task's board. The task read is
    /// skipped entirely for the user's own hands, which are always exempt.
    fn task_move_gate(&self, id_or_key: &str, actor: &str) -> Result<()> {
        if crate::projects::actor_is_user(actor) {
            return Ok(());
        }
        let Some(pid) = self.tasks.get(id_or_key)?.task.project_id else { return Ok(()) };
        crate::projects::check_task_move(actor, &self.projects().get(pid)?)
    }

    fn agents(&self) -> AgentRepo<'_> { AgentRepo::new(&self.db) }
    fn docs(&self, kind: DocKind) -> DocRepo<'_> { DocRepo::new(&self.db, kind) }
    fn settings(&self) -> crate::settings::SettingsRepo<'_> { crate::settings::SettingsRepo::new(&self.db) }

    /// Whether `extraction.enabled` is on. Read fresh on every call rather than
    /// cached, since the daemon serves every client and a setting change must take
    /// effect on the next sync or refresh, not after a restart. Shared by `sync`
    /// (whether to install the transcript hooks) and `refresh_project` (whether to
    /// enqueue a project summary).
    fn extraction_enabled(&self) -> Result<bool> {
        Ok(self.settings().get_raw("extraction.enabled")?.and_then(|v| v.as_bool()) == Some(true))
    }

    /// Whether `board.mirror_tasks_md` is on, the same fresh-read, off-by-default
    /// pattern as `extraction_enabled`. `sync` resolves it here rather than trusting
    /// the request: `POST /sync` is unauthenticated.
    fn mirror_tasks_md_enabled(&self) -> Result<bool> {
        Ok(self.settings().get_raw("board.mirror_tasks_md")?.and_then(|v| v.as_bool()) == Some(true))
    }
}

#[async_trait::async_trait]
impl Backend for LocalBackend {
    async fn status(&self) -> Result<StatusReport> {
        let mut s = self.memories.status(self.port)?;
        s.db_path = self.paths.db_path().display().to_string();
        Ok(s)
    }
    /// The project's `agent_access` is enforced here rather than in the MCP router,
    /// because MCP reaches the daemon over HTTP and the shim cannot see the rule. An
    /// actor the project has not admitted is refused, and `require_review` turns an
    /// agent's memory into a pending one.
    async fn remember(&self, mut m: NewMemory, actor: &str) -> Result<Memory> {
        if self.memory_gate(m.project_id, actor)? {
            m.status = MemoryStatus::Pending;
        }
        self.memories.remember(m, actor)
    }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> {
        check_scope(q.project_id, q.list_scope)?;
        self.memories.recall(&q)
    }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> { self.memories.forget(id, reason, actor) }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> { self.memories.get(id) }
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>, scope: MemoryScopeFilter) -> Result<Vec<Memory>> {
        check_scope(project_id, scope)?;
        self.memories.list_scoped(status, None, project_id, scope)
    }
    async fn set_memory_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory> { self.memories.set_status(id, status, actor) }

    /// Detects the project at `root` and records it, building a profile when the
    /// stored one is missing or stale. The upsert runs first, without a profile, so
    /// `needs_refresh` can consult what is already stored before doing the work.
    async fn connect_project(&self, root: PathBuf, actor: &str) -> Result<Project> {
        let detected = detect_root(&root)?;
        let repo = self.projects();
        let project = repo.upsert(&detected, None, actor)?;
        if repo.needs_refresh(&project) {
            let profile = build_profile(&detected.root)?;
            return repo.set_profile(project.id, &profile, actor);
        }
        Ok(project)
    }

    async fn project_context(&self, root: PathBuf, actor: &str) -> Result<ProjectContext> {
        let project = self.connect_project(root, actor).await?;
        let query = match &project.profile {
            Some(p) => format!("{} {}", p.name, p.frameworks.join(" ")),
            None => project.name.clone(),
        };
        let mut memories = self.memories.recall(&RecallQuery {
            query, limit: 20, scope: None, list_scope: MemoryScopeFilter::All, project_id: Some(project.id), kinds: vec![], tags: vec![],
        })?;
        // Recall is a search, so a project whose memories don't happen to match its own
        // name would come back empty. Top it up with the newest project-scoped memories
        // (score 0.0: they were not ranked, they were appended) so context is never bare.
        let seen: std::collections::HashSet<Uuid> = memories.iter().map(|h| h.memory.id).collect();
        let recent = self.memories.list(MemoryStatus::Active, Some(MemoryScope::Project), Some(project.id))?;
        memories.extend(recent.into_iter().filter(|m| !seen.contains(&m.id)).take(10).map(|memory| RecallHit { memory, score: 0.0 }));
        Ok(ProjectContext {
            practices: self.docs(DocKind::Practice).list(Some(project.id))?,
            workflows: self.docs(DocKind::Workflow).list(Some(project.id))?,
            project,
            memories,
        })
    }

    async fn list_projects(&self) -> Result<Vec<Project>> { self.projects().list() }
    async fn get_project(&self, id: Uuid) -> Result<Project> { self.projects().get(id) }
    async fn refresh_project(&self, id: Uuid) -> Result<Project> {
        let project = self.projects().get(id)?;
        let profile = build_profile(std::path::Path::new(&project.root_path))?;
        // Gated because the summary job also writes this profile: it reads one, asks the
        // model, then re-reads and writes under the same gate. Without a gate shared by
        // both writers the two interleave and one of them loses its half of the profile.
        // The repository scan above stays outside the gate, and nothing awaits inside it.
        let updated = {
            let _gate = self.memories.write_gate();
            self.projects().set_profile(id, &profile, "refresh")?
        };
        // A fresh profile carries no summary until the worker writes one; queue that
        // only when extraction is on, the same gate `ingest_transcript` checks.
        if self.extraction_enabled()? {
            self.jobs.enqueue("project_summary", serde_json::json!({"project_id": updated.id}))?;
            self.queue.notify.notify_one();
        }
        Ok(updated)
    }
    async fn delete_project(&self, id: Uuid, actor: &str) -> Result<()> { self.delete_project_gated(id, actor) }

    /// Gated: a board key rename rewrites every `tasks.key` on the board, which is a
    /// board write and takes the same mutex in the same order as every other one.
    /// Kept synchronous so the guard cannot be held across an await.
    async fn update_project(&self, id: Uuid, patch: ProjectPatch, actor: &str) -> Result<Project> {
        let gate = self.memories.gate_handle();
        let _gate = gate.lock().unwrap_or_else(|e| e.into_inner());
        self.projects().update(id, &patch, actor)
    }
    async fn set_agent_access(&self, id: Uuid, access: AgentAccess, actor: &str) -> Result<Project> {
        self.projects().set_agent_access(id, &access, actor)
    }
    async fn set_project_extraction(&self, id: Uuid, over: Option<ProjectExtraction>, actor: &str) -> Result<Project> {
        self.projects().set_project_extraction(id, over, actor)
    }
    async fn project_log(&self, id: Uuid, f: LogFilter) -> Result<Vec<LogEntry>> { crate::projects::log::project_log(&self.db, id, &f) }
    async fn project_log_export(&self, id: Uuid) -> Result<String> { crate::projects::log::project_log_export(&self.db, id) }

    async fn list_agents(&self) -> Result<Vec<Agent>> { self.agents().list() }
    async fn get_agent(&self, name: &str) -> Result<Agent> { self.agents().get(name) }
    async fn save_agent(&self, a: NewAgent, actor: &str) -> Result<Agent> { self.agents().save(&a, actor) }
    async fn delete_agent(&self, name: &str, actor: &str) -> Result<()> { self.agents().delete(name, actor) }

    async fn list_docs(&self, kind: DocKind, project_id: Option<Uuid>) -> Result<Vec<Doc>> { self.docs(kind).list(project_id) }
    async fn get_doc(&self, kind: DocKind, name: &str) -> Result<Doc> { self.docs(kind).get(name) }
    async fn save_doc(&self, kind: DocKind, d: NewDoc, actor: &str) -> Result<Doc> { self.docs(kind).save(&d, actor) }
    async fn delete_doc(&self, kind: DocKind, name: &str, actor: &str) -> Result<()> { self.docs(kind).delete(name, actor) }

    /// Plans the sync on the daemon host and, unless `check_only`, writes it.
    async fn sync(&self, req: SyncRequest) -> Result<SyncReport> {
        let agents = self.agents().list()?;
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
            let practices = self.docs(DocKind::Practice).list(Some(project.id))?;
            let block = BlockContext { mcp_command: MCP_COMMAND.into(), agents: agents.clone(), practices, project_name: Some(project.name.clone()) };
            (PathBuf::from(project.root_path), requested, block, vec![], Some(project.id))
        };
        // The hooks feed transcripts to a model, so they are installed only once the
        // user has switched extraction on. The daemon resolves that here rather than
        // trusting the request: `POST /sync` is unauthenticated.
        let hooks = self.extraction_enabled()?;
        // Codex keeps one config file per user, so its hook needs the sync home even
        // when the pass writes into a project. `CodexHook` is one of the defaults, so
        // with extraction on this resolves on nearly every sync, and a machine with no
        // home to find fails the whole pass rather than just the hook.
        let home = if hooks && targets.contains(&SyncKind::CodexHook) { sync_home()? } else { PathBuf::new() };
        // The mirror is a per-project file, so a global sync (no project) never fetches
        // board data for it, even when the setting is on. Fetched only when the setting
        // is on, so an ordinary sync with the mirror off does not pay for two extra
        // queries it will not use.
        let mirror_tasks_md = self.mirror_tasks_md_enabled()? && project_id.is_some();
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
            self.memories.audit(
                "sync",
                "apply",
                "sync",
                Some(project_id),
                serde_json::json!({"created": report.created, "updated": report.updated, "unchanged": report.unchanged, "skipped": report.skipped}),
            )?;
        }
        Ok(report)
    }

    async fn get_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>> { self.settings().get_all() }
    async fn set_settings(&self, values: serde_json::Map<String, serde_json::Value>, actor: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
        let cleared = self.settings().set_many(&values, actor)?;
        let mut out = self.settings().get_all()?;
        // Not one of `SETTING_KEYS`: a one-off flag telling the caller that pointing the
        // base url somewhere new dropped the key it was entered against, so the endpoint
        // it just named will not receive it.
        if cleared {
            out.insert("extraction.api_key_cleared".into(), serde_json::Value::Bool(true));
        }
        Ok(out)
    }

    /// Every check a transcript has to pass lives here rather than in the HTTP
    /// handler, because MCP reaches this same method and is just as unauthenticated:
    /// the enable gate, so a caller who has not configured extraction is told straight
    /// away instead of getting a job id for work that will only fail later; a blank
    /// transcript, which would spend a model call on nothing; and the character cap,
    /// which bounds how much any one caller can push into a single model call.
    async fn ingest_transcript(&self, text: String, source_tool: String, project_root: Option<PathBuf>) -> Result<Uuid> {
        // Resolved once, here, through the same `project_for` the worker used to call:
        // a root inside a repository has to mean the same project at the gate as it does
        // when the job runs, or the access check and the extraction override both look at
        // the wrong scope. The answer travels in the payload so the worker never has to
        // ask again.
        let project_id = crate::extract::project_for(&self.db, project_root.clone())?;
        self.memory_gate(project_id, &source_tool)?;
        crate::extract::resolve_extraction(&self.db, project_id)?;
        if text.trim().is_empty() {
            return Err(AtlasError::Invalid("ingest text is empty".into()));
        }
        if text.chars().count() > MAX_INGEST_CHARS {
            return Err(AtlasError::TooLarge("transcript too large".into()));
        }
        let id = self.jobs.enqueue("ingest", serde_json::json!({
            "text": text, "source_tool": source_tool, "project_root": project_root, "project_id": project_id,
        }))?;
        self.queue.notify.notify_one();
        Ok(id)
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<Job>> { self.jobs.get(id) }

    async fn test_extraction_for(&self, project_id: Option<Uuid>) -> Result<String> {
        let cfg = crate::extract::resolve_extraction(&self.db, project_id)?;
        let client = crate::extract::build_client(&cfg)?;
        let reply = client.chat("You are a connectivity check.", "Reply with the single word OK").await?;
        Ok(reply.trim().to_string())
    }

    async fn list_tasks(&self, f: TaskFilter) -> Result<Vec<Task>> { self.tasks.list(&f) }
    async fn get_task(&self, id_or_key: &str) -> Result<TaskDetail> { self.tasks.get(id_or_key) }
    async fn create_task(&self, t: NewTask, actor: &str) -> Result<Task> { self.tasks.create(&t, actor) }
    async fn update_task(&self, id_or_key: &str, u: TaskUpdate, actor: &str) -> Result<Task> { self.tasks.update(id_or_key, &u, actor) }
    async fn move_task(&self, id_or_key: &str, stage: &str, expected: Option<DateTime<Utc>>, actor: &str) -> Result<Task> {
        self.task_move_gate(id_or_key, actor)?;
        self.tasks.move_stage(id_or_key, stage, expected, actor)
    }
    async fn comment_task(&self, id_or_key: &str, body: &str, actor: &str) -> Result<TaskEvent> { self.tasks.comment(id_or_key, body, actor) }
    async fn claim_task(&self, id_or_key: &str, force: bool, actor: &str) -> Result<Task> {
        // A claim moves the task out of the first stage, so it is a move.
        self.task_move_gate(id_or_key, actor)?;
        self.tasks.claim(id_or_key, force, actor)
    }
    async fn set_task_blockers(&self, id_or_key: &str, blocked_by: Vec<String>, actor: &str) -> Result<Task> {
        self.tasks.set_blockers(id_or_key, blocked_by, actor)
    }
    async fn delete_task(&self, id_or_key: &str, actor: &str) -> Result<()> { self.tasks.delete(id_or_key, actor) }
    async fn board_stages(&self, project_id: Option<Uuid>) -> Result<StageList> { self.tasks.effective_stages(project_id) }
    async fn set_board_stages(&self, stages: Vec<Stage>, renames: HashMap<String, String>, actor: &str) -> Result<Vec<Stage>> {
        self.tasks.set_global_stages(stages, &renames, actor)
    }
    async fn set_project_stages(&self, project_id: Uuid, stages: Option<Vec<Stage>>, renames: HashMap<String, String>, actor: &str) -> Result<StageList> {
        self.tasks.set_project_stages(project_id, stages, &renames, actor)
    }
    async fn task_counts(&self, project_id: Option<Uuid>) -> Result<Vec<(String, i64)>> { self.tasks.counts_by_stage(project_id) }

    async fn search(&self, q: SearchQuery) -> Result<SearchResult> {
        let memories = crate::memories::MemoryRepo::new(&self.db);
        let projects = self.projects();
        let workflows = self.docs(DocKind::Workflow);
        crate::search::global::search(&q, &self.tasks, &memories, &projects, &workflows)
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
}
