//! Turns an [`Effect`] into the one `RemoteBackend` call it names, and turns the
//! result into an [`Action`] the reducer understands. Every error, from any effect,
//! collapses to `Action::Error`.

use super::state::{Action, Effect};
use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use atlas_core::models::{MemoryStatus, RecallHit, RecallQuery, SyncRequest};
use std::path::PathBuf;
use std::sync::Arc;

/// The actor every write from the TUI is recorded under.
const ACTOR: &str = "tui";

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
                .list_memories(MemoryStatus::Active, None)
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
        Effect::ListPending => Ok(Action::PendingLoaded(backend.list_memories(MemoryStatus::Pending, None).await?)),
        Effect::Status => Ok(Action::StatusLoaded(backend.status().await?)),
        Effect::Forget(id) => {
            let memory = backend.forget(id, None, ACTOR).await?;
            Ok(Action::MemoryForgotten(memory.id))
        }
        Effect::SetStatus(id, status) => Ok(Action::MemoryStatusChanged(backend.set_memory_status(id, status, ACTOR).await?)),
    }
}
