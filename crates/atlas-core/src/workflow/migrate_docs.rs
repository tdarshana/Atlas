//! The one-time move from Markdown workflow documents to real workflows.
//!
//! Before Phase 9 a "workflow" was a document: a name and a body of instructions. Each
//! one becomes a manual workflow with a single action called `main` whose instructions
//! are the body, so nothing a user wrote is lost and the graph editor has something to
//! open. The document is deleted once its workflow exists, and the settings flag
//! [`DOCS_MIGRATED_SETTING`] stops the whole pass running twice.

use super::{DOCS_MIGRATED_SETTING, WorkflowRepo};
use crate::library::DocRepo;
use crate::models::*;
use crate::settings::SettingsRepo;
use crate::{AtlasError, Result};
use serde_json::{Map, Value};

/// The action a migrated document becomes.
const ACTION_NAME: &str = "main";
/// The agent a migrated action runs as: the desktop's own default, not a saved agent.
const ACTION_AGENT: &str = "desktop";

/// A trigger, one action and an output on a straight line, laid out left to right the
/// way the editor draws a new workflow.
fn single_action_graph(instructions: &str) -> Graph {
    Graph {
        nodes: vec![
            Node {
                id: "trigger".into(),
                kind: NodeKind::Trigger,
                position: Position { x: 0.0, y: 0.0 },
                data: NodeData::Trigger(Trigger::manual()),
            },
            Node {
                id: "action".into(),
                kind: NodeKind::Action,
                position: Position { x: 240.0, y: 0.0 },
                data: NodeData::Action {
                    name: ACTION_NAME.into(),
                    instructions: instructions.to_string(),
                    agent: ACTION_AGENT.into(),
                    practices: vec![],
                    memories: None,
                },
            },
            Node {
                id: "output".into(),
                kind: NodeKind::Output,
                position: Position { x: 480.0, y: 0.0 },
                data: NodeData::Output { propose_memories: false, file_tasks: false },
            },
        ],
        edges: vec![
            Edge { id: "trigger-action".into(), source: "trigger".into(), target: "action".into() },
            Edge { id: "action-output".into(), source: "action".into(), target: "output".into() },
        ],
    }
}

/// The document's name, or that name with a number appended when a workflow already
/// holds it. A document is never renamed away from a name that is free.
fn free_name(repo: &WorkflowRepo, name: &str) -> Result<String> {
    let mut candidate = name.to_string();
    let mut n = 2;
    loop {
        match repo.get(&candidate) {
            Err(AtlasError::NotFound(_)) => return Ok(candidate),
            Err(e) => return Err(e),
            Ok(_) => {
                candidate = format!("{name} {n}");
                n += 1;
            }
        }
    }
}

/// Turns every Markdown workflow document into a workflow and deletes it. Answers how
/// many were moved; a second call answers 0 without touching anything.
pub fn migrate_workflow_docs(docs: &DocRepo<'_>, repo: &WorkflowRepo, settings: &SettingsRepo<'_>, actor: &str) -> Result<usize> {
    if settings.get_raw(DOCS_MIGRATED_SETTING)?.and_then(|v| v.as_bool()) == Some(true) {
        return Ok(0);
    }
    let mut moved = 0;
    for doc in docs.list(None)? {
        let name = free_name(repo, &doc.name)?;
        repo.create(
            &NewWorkflow {
                name: name.clone(),
                project_id: doc.project_id,
                description: format!("Migrated from the workflow document '{}'.", doc.name),
                trigger: Trigger::manual(),
                graph: single_action_graph(&doc.body),
                enabled: true,
            },
            actor,
        )?;
        docs.delete(&doc.name, actor)?;
        tracing::info!("migrated workflow document '{}' into workflow '{name}'", doc.name);
        moved += 1;
    }
    settings.set_many(&Map::from_iter([(DOCS_MIGRATED_SETTING.to_string(), Value::Bool(true))]), actor)?;
    Ok(moved)
}
