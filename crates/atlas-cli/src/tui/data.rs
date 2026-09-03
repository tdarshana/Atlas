//! Turns an [`Effect`] into the one `RemoteBackend` call it names, and turns the
//! result into an [`Action`] the reducer understands. Every error, from any effect,
//! collapses to `Action::Error`.

use super::state::{Action, Effect};
use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use atlas_core::models::{MemoryStatus, NewTask, RecallHit, RecallQuery, SyncRequest, TaskFilter};
use std::path::PathBuf;
use std::sync::Arc;

/// The actor every write from the TUI is recorded under. Board reads carry it in
/// a header rather than an argument, so `run` puts it on the client as well.
pub const ACTOR: &str = "tui";

pub async fn perform(effect: Effect, backend: Arc<RemoteBackend>, cwd: PathBuf) -> Action {
    match run(effect, backend, cwd).await {
        Ok(action) => action,
        Err(e) => Action::Error(e.to_string()),
    }
}

async fn run(effect: Effect, backend: Arc<RemoteBackend>, cwd: PathBuf) -> atlas_core::Result<Action> {
    match effect {
        Effect::Recall(query) if query.is_empty() => {
            let hits = backend
                .list_memories(MemoryStatus::Active, None, atlas_core::models::MemoryScopeFilter::All)
                .await?
                .into_iter()
                .map(|memory| RecallHit { memory, score: 0.0 })
                .collect();
            Ok(Action::MemoriesLoaded(hits))
        }
        Effect::Recall(query) => {
            let hits = backend
                .recall(RecallQuery { query, limit: 50, scope: None, project_id: None, kinds: vec![], tags: vec![] })
                .await?;
            Ok(Action::MemoriesLoaded(hits))
        }
        Effect::ListProjects => Ok(Action::ProjectsLoaded(backend.list_projects().await?)),
        Effect::ConnectCwd => {
            backend.connect_project(cwd, ACTOR).await?;
            Ok(Action::ProjectsLoaded(backend.list_projects().await?))
        }
        Effect::ProjectContext(root) => Ok(Action::ProjectContextLoaded(Box::new(backend.project_context(root, ACTOR).await?))),
        Effect::ListAgents => Ok(Action::AgentsLoaded(backend.list_agents().await?)),
        Effect::Sync(root) => {
            let global = root.is_none();
            let report = backend.sync(SyncRequest { root, global, targets: vec![], check_only: false }).await?;
            Ok(Action::SyncDone(report))
        }
        Effect::ListDocs(kind) => Ok(Action::DocsLoaded(kind, backend.list_docs(kind, None).await?)),
        Effect::ListPending => Ok(Action::PendingLoaded(backend.list_memories(MemoryStatus::Pending, None, atlas_core::models::MemoryScopeFilter::All).await?)),
        Effect::LoadBoard(project_id) => {
            let stages = backend.board_stages(project_id).await?.stages;
            let tasks = backend.list_tasks(TaskFilter { project_id, ..Default::default() }).await?;
            Ok(Action::BoardLoaded(stages, tasks))
        }
        Effect::LoadTaskDetail(key) => Ok(Action::TaskDetailLoaded(Box::new(backend.get_task(&key).await?))),
        // A move or a new task changes which column a card is in, so the reducer
        // has to reload; a comment does not, so it answers with the task itself.
        Effect::MoveTask { key, stage } => {
            backend.move_task(&key, &stage, None, ACTOR).await?;
            Ok(Action::BoardChanged)
        }
        Effect::CommentTask { key, body } => {
            backend.comment_task(&key, &body, ACTOR).await?;
            Ok(Action::TaskDetailLoaded(Box::new(backend.get_task(&key).await?)))
        }
        Effect::CreateTask { title, project_id } => {
            backend.create_task(NewTask { project_id, title, ..Default::default() }, ACTOR).await?;
            Ok(Action::BoardChanged)
        }
        Effect::Status => Ok(Action::StatusLoaded(backend.status().await?)),
        Effect::Forget(id) => {
            let memory = backend.forget(id, None, ACTOR).await?;
            Ok(Action::MemoryForgotten(memory.id))
        }
        Effect::SetStatus(id, status) => Ok(Action::MemoryStatusChanged(backend.set_memory_status(id, status, ACTOR).await?)),
    }
}
