//! Executes one workflow run: walks the graph's actions in order, calling the
//! configured model for each, then turns the last action's trailing JSON block (when
//! the output node asks for it) into proposed memories and filed tasks.
//!
//! Run by the daemon's worker for a `workflow_run` job. The signature takes the whole
//! [`Job`] rather than a bare run id (as an earlier sketch of this contract had it):
//! the job's payload is the only place the run's actor and optional input live, and
//! that matches how [`crate::extract::run_ingest`] and `run_project_summary` are
//! already called from the worker.

use crate::backend::{Backend, LocalBackend};
use crate::extract::{self, ExtractionConfig};
use crate::jobs::Job;
use crate::library::{AgentRepo, DocRepo};
use crate::models::*;
use crate::{AtlasError, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use super::graph;

/// A candidate this confident (or more) may be stored active outright, the same rule
/// `extract::run_ingest` applies to extracted memories.
fn status_for(confidence: f64, threshold: f64) -> MemoryStatus {
    if confidence >= threshold { MemoryStatus::Active } else { MemoryStatus::Pending }
}

/// The last ```json (or bare ```) fenced block in `text`, parsed as a JSON value.
/// `None` when there is no fence, or its body does not parse.
fn trailing_json_block(text: &str) -> Option<Value> {
    let body_start = text.rfind("```json").map(|i| i + "```json".len()).or_else(|| text.rfind("```").map(|i| i + "```".len()))?;
    let rest = &text[body_start..];
    let body_end = rest.find("```")?;
    serde_json::from_str(rest[..body_end].trim()).ok()
}

/// Records a failure on a step that never got to run: a synthetic step (there is no
/// action to attach it to) carrying one ERR line, so a graph that fails re-validation
/// still leaves a readable trail in the run view.
fn fail_before_steps(backend: &LocalBackend, run_id: Uuid, err: AtlasError) -> Result<Value> {
    let log = vec![LogLine::now(LogLevel::Error, err.to_string())];
    if let Ok(step) = backend.workflows.append_step(run_id, 0, "", "validate", "") {
        let _ = backend.workflows.finish_step(step.id, StepStatus::Failed, None, &log);
    }
    let _ = backend.workflows.set_run_status(run_id, RunStatus::Failed, None);
    Err(err)
}

/// Records a failure raised while a step was in flight: the step's own log gets the
/// ERR line, the run is marked failed, and the rest of the actions are skipped.
fn fail_step(backend: &LocalBackend, run_id: Uuid, step_id: Uuid, mut log: Vec<LogLine>, err: AtlasError) -> Result<Value> {
    log.push(LogLine::now(LogLevel::Error, err.to_string()));
    let _ = backend.workflows.finish_step(step_id, StepStatus::Failed, None, &log);
    let _ = backend.workflows.set_run_status(run_id, RunStatus::Failed, None);
    Err(err)
}

/// The memories an action sees, as the bullet list its user message carries: the
/// action's own `MemorySource` when it named one, else a source with no filters
/// bounded to the workflow's own project. Kinds and tags narrow, `limit` caps.
fn memories_section(backend: &LocalBackend, workflow: &Workflow, source: &Option<MemorySource>) -> String {
    let source = source.clone().unwrap_or(MemorySource { kinds: vec![], tags: vec![], limit: 20, project_id: workflow.project_id });
    let project_id = source.project_id.or(workflow.project_id);
    let mut memories = backend.memories.list(MemoryStatus::Active, None, project_id).unwrap_or_default();
    if !source.kinds.is_empty() {
        memories.retain(|m| source.kinds.iter().any(|k| k == m.kind.as_str()));
    }
    if !source.tags.is_empty() {
        memories.retain(|m| source.tags.iter().any(|t| m.tags.contains(t)));
    }
    memories.truncate(source.limit as usize);
    if memories.is_empty() {
        return "(none)".to_string();
    }
    memories.iter().map(|m| format!("- ({}) {}", m.kind, m.text)).collect::<Vec<_>>().join("\n")
}

/// The system prompt for one action: a saved agent's instructions, or a generic
/// fallback naming the label the graph gave it, followed by one `## Practice: <name>`
/// section per attached practice that actually resolves. A practice that does not
/// resolve is skipped with a WARN line rather than failing the step: the action still
/// runs, just without that guidance.
fn system_prompt(backend: &LocalBackend, agent: &str, practices: &[String], log: &mut Vec<LogLine>) -> String {
    let mut system = match AgentRepo::new(&backend.db).get(agent) {
        Ok(a) => a.instructions,
        Err(_) => format!("You are {agent}, an agent working inside Atlas."),
    };
    let docs = DocRepo::new(&backend.db, DocKind::Practice);
    for name in practices {
        match docs.get(name) {
            Ok(doc) => system.push_str(&format!("\n\n## Practice: {}\n\n{}", doc.name, doc.body)),
            Err(_) => log.push(LogLine::now(LogLevel::Warn, format!("practice '{name}' not found, skipping"))),
        }
    }
    system
}

/// Turns the last action's trailing JSON block into pending or active memories and
/// filed tasks, per the output node's flags. A block that does not parse, or is not
/// there when one was expected, is a WARN, not a failure: the run has already
/// succeeded by the time this runs. `actor` is `workflow/<name>`, the same identity
/// the memory-write and task-create gates check for any other caller.
async fn apply_output(
    backend: &LocalBackend,
    workflow: &Workflow,
    output: &NodeData,
    last_output: &str,
    actor: &str,
    threshold: f64,
    log: &mut Vec<LogLine>,
) -> (usize, usize) {
    let NodeData::Output { propose_memories, file_tasks } = output else { unreachable!("checked by graph::validate") };
    if !propose_memories && !file_tasks {
        return (0, 0);
    }
    let Some(block) = trailing_json_block(last_output) else {
        log.push(LogLine::now(LogLevel::Warn, "the last step's output carried no JSON block to read memories or tasks from"));
        return (0, 0);
    };

    let mut memories_proposed = 0usize;
    if *propose_memories {
        if let Some(Value::Array(items)) = block.get("memories") {
            match serde_json::to_string(items).ok().map(|s| extract::parse_candidates(&s)) {
                Some(Ok(candidates)) => {
                    for c in candidates {
                        let new = NewMemory {
                            scope: if workflow.project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global },
                            project_id: workflow.project_id,
                            kind: c.kind,
                            text: c.text,
                            tags: c.tags,
                            source_agent: Some(actor.to_string()),
                            source_tool: Some("workflow".to_string()),
                            confidence: c.confidence,
                            status: status_for(c.confidence, threshold),
                        };
                        match backend.remember(new, actor).await {
                            Ok(_) => memories_proposed += 1,
                            Err(e) => log.push(LogLine::now(LogLevel::Warn, format!("a proposed memory was not stored: {e}"))),
                        }
                    }
                }
                Some(Err(e)) => log.push(LogLine::now(LogLevel::Warn, format!("the memories block did not parse: {e}"))),
                None => {}
            }
        }
    }

    let mut tasks_filed = 0usize;
    if *file_tasks {
        if let Some(Value::Array(items)) = block.get("tasks") {
            for item in items {
                let Some(title) = item.get("title").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) else {
                    log.push(LogLine::now(LogLevel::Warn, "a task in the block had no title, skipping"));
                    continue;
                };
                let new = NewTask {
                    project_id: workflow.project_id,
                    title: title.to_string(),
                    description: item.get("description").and_then(Value::as_str).map(str::to_string),
                    kind: item.get("kind").and_then(Value::as_str).and_then(|s| s.parse().ok()),
                    priority: item.get("priority").and_then(Value::as_str).and_then(|s| s.parse().ok()),
                    assignee: None,
                    labels: None,
                    parent: None,
                    blocked_by: None,
                    stage: None,
                };
                match backend.create_task(new, actor).await {
                    Ok(_) => tasks_filed += 1,
                    Err(e) => log.push(LogLine::now(LogLevel::Warn, format!("a filed task was not created: {e}"))),
                }
            }
        }
    }
    (memories_proposed, tasks_filed)
}

/// Runs the `workflow_run` job named by `job.payload`: `{ workflow_id, run_id,
/// trigger, actor, input? }`. `run_id` is enough to find the workflow and the run
/// itself, but the run's own actor and optional input are not persisted anywhere but
/// the job payload, so both travel with it.
pub async fn run_workflow(job: &Job, backend: &LocalBackend) -> Result<Value> {
    let run_id = job.payload["run_id"].as_str().and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AtlasError::Invalid("workflow_run job has no run_id".into()))?;
    let workflow_id = job.payload["workflow_id"].as_str().and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AtlasError::Invalid("workflow_run job has no workflow_id".into()))?;
    let run_actor = job.payload["actor"].as_str().unwrap_or("scheduler").to_string();
    let input = job.payload["input"].as_str().map(str::to_string);

    let (run, _) = backend.workflows.get_run(run_id)?;
    if run.status == RunStatus::Cancelled {
        return Ok(json!({"status": "cancelled", "steps": 0}));
    }

    let workflow = backend.workflows.get(&workflow_id.to_string())?;
    let order = match graph::validate(&workflow.graph) {
        Ok(order) => order,
        Err(e) => return fail_before_steps(backend, run_id, e),
    };

    backend.workflows.set_run_status(run_id, RunStatus::Running, None)?;
    let workflow_actor = format!("workflow/{}", workflow.name);

    let mut last_output: Option<String> = None;
    let mut last_threshold = 1.0f64;
    let mut last_step: Option<WorkflowStep> = None;
    for (i, action_id) in order.iter().enumerate() {
        let (run, _) = backend.workflows.get_run(run_id)?;
        if run.status == RunStatus::Cancelled {
            return Ok(json!({"status": "cancelled", "steps": i}));
        }

        let node = workflow.graph.nodes.iter().find(|n| &n.id == action_id).expect("action id came from graph::validate");
        let NodeData::Action { name, instructions, agent, practices, memories } = &node.data else {
            unreachable!("graph::validate matched data to kind")
        };
        let step = backend.workflows.append_step(run_id, i as i32, action_id, name, agent)?;
        let mut log: Vec<LogLine> = vec![];

        let cfg: ExtractionConfig = match extract::resolve_extraction(&backend.db, workflow.project_id) {
            Ok(cfg) => cfg,
            Err(e) => return fail_step(backend, run_id, step.id, log, e),
        };
        last_threshold = cfg.auto_accept_min_confidence;
        let client = match extract::build_client(&cfg) {
            Ok(c) => c,
            Err(e) => return fail_step(backend, run_id, step.id, log, e),
        };

        let system = system_prompt(backend, agent, practices, &mut log);
        let mut user = instructions.clone();
        if i == 0 {
            if let Some(inp) = &input {
                user.push_str(&format!("\n\nInput: {inp}"));
            }
        }
        user.push_str("\n\nMemories:\n");
        user.push_str(&memories_section(backend, &workflow, memories));
        user.push_str("\n\nPrevious step output:\n");
        user.push_str(last_output.as_deref().unwrap_or("none"));

        log.push(LogLine::now(LogLevel::Info, format!("request: system {} chars, user {} chars", system.chars().count(), user.chars().count())));
        let reply = match client.chat(&system, &user).await {
            Ok(r) => r,
            Err(e) => return fail_step(backend, run_id, step.id, log, e),
        };
        log.push(LogLine::now(LogLevel::Info, format!("reply: {} chars", reply.chars().count())));

        last_step = Some(backend.workflows.finish_step(step.id, StepStatus::Success, Some(&reply), &log)?);
        last_output = Some(reply);
    }

    let output_node = workflow.graph.nodes.iter().find(|n| n.kind == NodeKind::Output).expect("graph::validate requires exactly one output");
    let mut output_log: Vec<LogLine> = vec![];
    let (memories_proposed, tasks_filed) = match &last_output {
        Some(out) => apply_output(backend, &workflow, &output_node.data, out, &workflow_actor, last_threshold, &mut output_log).await,
        None => (0, 0),
    };
    // The output step's log is folded into the last action's own step rather than a
    // fresh one: it made no model call of its own, and a run view reader expects one
    // row per action node, not an extra row for bookkeeping.
    if let Some(last) = last_step.filter(|_| !output_log.is_empty()) {
        let mut combined = last.log.clone();
        combined.extend(output_log);
        let _ = backend.workflows.finish_step(last.id, last.status, last.output.as_deref(), &combined);
    }

    let summary = json!({
        "steps": order.len(),
        "memories_proposed": memories_proposed,
        "tasks_filed": tasks_filed,
        "trigger": run.trigger.as_str(),
        "tokens": Value::Null,
    });
    backend.workflows.set_run_status(run_id, RunStatus::Success, Some(summary.clone()))?;
    backend.memories.audit(&run_actor, "run", "workflow_run", Some(run_id), summary.clone())?;
    Ok(summary)
}
