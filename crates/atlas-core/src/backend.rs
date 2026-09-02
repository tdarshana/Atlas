use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use crate::db::Db;
use crate::export::BlockContext;
use crate::jobs::{Job, JobQueue, JobRepo};
use crate::library::{AgentRepo, DocRepo};
use crate::models::*;
use crate::paths::AtlasPaths;
use crate::projects::{build_profile, detect_root, ProjectRepo};
use crate::search::{FastEmbedder, NoopEmbedder};
use crate::service::MemoryService;
use crate::sync::{self, SyncInputs};
use crate::{AtlasError, Result};

/// How the managed block tells an agent to reach Atlas.
const MCP_COMMAND: &str = "atlas mcp";

/// Targets a sync writes when the request names none.
const DEFAULT_TARGETS: &[SyncKind] =
    &[SyncKind::Claude, SyncKind::Codex, SyncKind::AgentsMd, SyncKind::ClaudeMd, SyncKind::ClaudeHook, SyncKind::CodexHook];

/// Why a global sync reports a managed-block target as skipped.
const GLOBAL_SKIP: &str = "global sync writes agent files only";

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

#[async_trait::async_trait]
pub trait Backend: Send + Sync + 'static {
    async fn status(&self) -> Result<StatusReport>;

    // ---- memories ----
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory>;
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>>;
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory>;
    async fn get_memory(&self, id: Uuid) -> Result<Memory>;
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>) -> Result<Vec<Memory>>;
    async fn set_memory_status(&self, id: Uuid, status: MemoryStatus, actor: &str) -> Result<Memory>;

    // ---- projects ----
    async fn connect_project(&self, root: PathBuf, actor: &str) -> Result<Project>;
    async fn project_context(&self, root: PathBuf, actor: &str) -> Result<ProjectContext>;
    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn get_project(&self, id: Uuid) -> Result<Project>;
    async fn refresh_project(&self, id: Uuid) -> Result<Project>;
    async fn delete_project(&self, id: Uuid, actor: &str) -> Result<()>;

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
    /// Sends a minimal connectivity check to the configured model and answers with
    /// its reply, trimmed. `Conflict` when extraction is off or half configured, the
    /// same gate `ingest_transcript` checks.
    async fn test_extraction(&self) -> Result<String>;
}

pub struct LocalBackend {
    pub memories: Arc<MemoryService>,
    pub db: Arc<Db>,
    pub paths: AtlasPaths,
    pub port: Option<u16>,
    pub jobs: Arc<JobRepo>,
    /// Wakes the daemon's worker the moment a job is queued.
    pub queue: Arc<JobQueue>,
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
        Ok(Self { jobs: Arc::new(JobRepo::new(db.clone())), queue: Arc::new(JobQueue::new()), memories, db, paths: paths.clone(), port })
    }

    fn projects(&self) -> ProjectRepo<'_> { ProjectRepo::new(&self.db) }
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
}

#[async_trait::async_trait]
impl Backend for LocalBackend {
    async fn status(&self) -> Result<StatusReport> {
        let mut s = self.memories.status(self.port)?;
        s.db_path = self.paths.db_path().display().to_string();
        Ok(s)
    }
    async fn remember(&self, m: NewMemory, actor: &str) -> Result<Memory> { self.memories.remember(m, actor) }
    async fn recall(&self, q: RecallQuery) -> Result<Vec<RecallHit>> { self.memories.recall(&q) }
    async fn forget(&self, id: Uuid, reason: Option<String>, actor: &str) -> Result<Memory> { self.memories.forget(id, reason, actor) }
    async fn get_memory(&self, id: Uuid) -> Result<Memory> { self.memories.get(id) }
    async fn list_memories(&self, status: MemoryStatus, project_id: Option<Uuid>) -> Result<Vec<Memory>> { self.memories.list(status, None, project_id) }
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
            query, limit: 20, scope: None, project_id: Some(project.id), kinds: vec![], tags: vec![],
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
        let updated = self.projects().set_profile(id, &profile, "refresh")?;
        // A fresh profile carries no summary until the worker writes one; queue that
        // only when extraction is on, the same gate `ingest_transcript` checks.
        if self.extraction_enabled()? {
            self.jobs.enqueue("project_summary", serde_json::json!({"project_id": updated.id}))?;
            self.queue.notify.notify_one();
        }
        Ok(updated)
    }
    async fn delete_project(&self, id: Uuid, actor: &str) -> Result<()> { self.projects().delete(id, actor) }

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
        let (root, targets, block, skipped) = if req.global {
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
                    path: home.join(if matches!(kind, SyncKind::AgentsMd) { "AGENTS.md" } else { "CLAUDE.md" }),
                    kind,
                    content: String::new(),
                    action: SyncAction::Skip(GLOBAL_SKIP.into()),
                })
                .collect();
            let block = BlockContext { mcp_command: MCP_COMMAND.into(), agents: agents.clone(), practices: vec![], project_name: None };
            (home, targets, block, skipped)
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
            (PathBuf::from(project.root_path), requested, block, vec![])
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
        let mut ops = sync::plan_sync(&SyncInputs { root: &root, agents: &agents, block, targets: &targets, home: &home, hooks })?;
        ops.extend(skipped);
        if req.check_only { Ok(sync::summarize(&ops)) } else { sync::apply(&ops) }
    }

    async fn get_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>> { self.settings().get_all() }
    async fn set_settings(&self, values: serde_json::Map<String, serde_json::Value>, actor: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
        self.settings().set_many(&values, actor)?;
        self.settings().get_all()
    }

    /// The enable gate is checked here rather than in the worker alone, so a caller
    /// who has not configured extraction is told so straight away instead of
    /// getting a job id for work that will only fail later.
    async fn ingest_transcript(&self, text: String, source_tool: String, project_root: Option<PathBuf>) -> Result<Uuid> {
        crate::extract::extraction_config(&self.db)?;
        let id = self.jobs.enqueue("ingest", serde_json::json!({
            "text": text, "source_tool": source_tool, "project_root": project_root,
        }))?;
        self.queue.notify.notify_one();
        Ok(id)
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<Job>> { self.jobs.get(id) }

    async fn test_extraction(&self) -> Result<String> {
        let cfg = crate::extract::extraction_config(&self.db)?;
        let client = crate::extract::build_client(&cfg)?;
        let reply = client.chat("You are a connectivity check.", "Reply with the single word OK").await?;
        Ok(reply.trim().to_string())
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
        let hits = b.recall(crate::models::RecallQuery { query: "runtime".into(), limit: 5, scope: None, project_id: None, kinds: vec![], tags: vec![] }).await.unwrap();
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
