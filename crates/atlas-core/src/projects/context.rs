//! Assembles a project's context: the project row, the memories worth reading first,
//! and the practices, workflows and skills in play there. Pure over the repositories,
//! so it can be exercised on an in-memory database; the backend only connects the
//! project and moves the call off the runtime thread.

use crate::db::Db;
use crate::library::DocRepo;
use crate::models::{DocKind, MemoryScope, MemoryScopeFilter, MemoryStatus, Project, ProjectContext, RecallHit, RecallQuery, WorkflowSummary};
use crate::service::MemoryService;
use crate::workflow::WorkflowRepo;
use crate::Result;
use std::path::Path;
use uuid::Uuid;

/// How many ranked memories the context opens with.
const RECALL_LIMIT: usize = 20;
/// How many of the newest project memories top the ranked ones up.
const RECENT_TOP_UP: usize = 10;

/// The context for `project`: ranked memories (queried by the project's name and
/// frameworks) topped up with its newest ones, then its practices, workflows and the
/// skills switched on there. `personas` is left for the MCP router, which has a
/// session; the daemon route has none.
pub fn build(db: &Db, memories: &MemoryService, workflows: &WorkflowRepo, skills_home: &Path, project: Project) -> Result<ProjectContext> {
    let query = match &project.profile {
        Some(p) => format!("{} {}", p.name, p.frameworks.join(" ")),
        None => project.name.clone(),
    };
    let mut hits = memories.recall(&RecallQuery {
        query, limit: RECALL_LIMIT, scope: None, list_scope: MemoryScopeFilter::All, project_id: Some(project.id), kinds: vec![], tags: vec![],
    })?;
    // Recall is a search, so a project whose memories don't happen to match its own
    // name would come back empty. Top it up with the newest project-scoped memories
    // (score 0.0: they were not ranked, they were appended) so context is never bare.
    let seen: std::collections::HashSet<Uuid> = hits.iter().map(|h| h.memory.id).collect();
    let recent = memories.list(MemoryStatus::Active, Some(MemoryScope::Project), Some(project.id))?;
    hits.extend(recent.into_iter().filter(|m| !seen.contains(&m.id)).take(RECENT_TOP_UP).map(|memory| RecallHit { memory, score: 0.0 }));
    // The skills an agent may actually use here, so the context says what is in
    // play rather than everything that exists. Discovery reads directories, so a
    // failure to read one must not cost the caller its whole context: an error
    // leaves the list empty rather than failing the call.
    let skills = crate::skills::list_skills(db, Some(&project), skills_home)
        .map(|l| l.skills.into_iter().filter(|s| s.enabled_here != Some(false)).collect())
        .unwrap_or_else(|e| {
            tracing::warn!("skills unavailable for project context: {e}");
            vec![]
        });
    Ok(ProjectContext {
        practices: DocRepo::new(db, DocKind::Practice).list(Some(project.id))?,
        workflows: workflows.list(Some(project.id))?.iter().map(WorkflowSummary::from).collect(),
        skills,
        project,
        memories: hits,
        personas: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MemoryKind, NewDoc, NewMemory};
    use crate::projects::{Detected, ProjectRepo};
    use crate::search::NoopEmbedder;
    use std::sync::Arc;

    /// A project's context carries its own practices and every project memory, the
    /// ones recall did not rank appended with score 0, and never another project's.
    #[test]
    fn context_carries_the_projects_own_memories_and_practices() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let memories = MemoryService::new(db.clone(), Arc::new(NoopEmbedder)).unwrap();
        let workflows = WorkflowRepo::new(db.clone(), memories.gate_handle());
        let repo = ProjectRepo::new(&db);
        let mine = repo.upsert(&Detected { root: "/tmp/ctx-mine".into(), remote: None }, None, "t").unwrap();
        let other = repo.upsert(&Detected { root: "/tmp/ctx-other".into(), remote: None }, None, "t").unwrap();
        for (project, text) in [(&mine, "deploys to fly.io"), (&mine, "uses bun"), (&other, "elsewhere")] {
            memories
                .remember(
                    NewMemory {
                        scope: MemoryScope::Project,
                        project_id: Some(project.id),
                        kind: MemoryKind::Fact,
                        text: text.into(),
                        tags: vec![],
                        source_agent: None,
                        source_tool: Some("test".into()),
                        confidence: 1.0,
                        status: MemoryStatus::Active,
                    },
                    "t",
                )
                .unwrap();
        }
        DocRepo::new(&db, DocKind::Practice).save(&NewDoc { name: "commits".into(), body: "imperative".into(), tags: vec![], project_id: Some(mine.id) }, "t").unwrap();
        DocRepo::new(&db, DocKind::Practice).save(&NewDoc { name: "theirs".into(), body: "x".into(), tags: vec![], project_id: Some(other.id) }, "t").unwrap();

        let skills_home = tempfile::tempdir().unwrap();
        let ctx = build(&db, &memories, &workflows, skills_home.path(), mine.clone()).unwrap();

        assert_eq!(ctx.project.id, mine.id);
        let texts: Vec<&str> = ctx.memories.iter().map(|h| h.memory.text.as_str()).collect();
        assert!(texts.contains(&"deploys to fly.io") && texts.contains(&"uses bun"), "{texts:?}");
        assert!(!texts.contains(&"elsewhere"), "another project's memory leaked in: {texts:?}");
        assert!(ctx.memories.iter().all(|h| h.memory.project_id == Some(mine.id)));
        assert_eq!(ctx.practices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), vec!["commits"]);
        assert!(ctx.workflows.is_empty());
        assert!(ctx.personas.is_none(), "the session-bound block is the router's to fill");
    }
}
