//! Executes one workflow run: walks the graph's actions in order, calling the
//! configured model for each, then turns the last action's trailing JSON block (when
//! the output node asks for it) into proposed memories and filed tasks.
//!
//! Run by the daemon's worker for a `workflow_run` job. The signature takes the whole
//! [`Job`] rather than a bare run id (as an earlier sketch of this contract had it):
//! the job's payload is the only place the run's actor and optional input live, and
//! that matches how [`crate::extract::run_ingest`] and `run_project_summary` are
//! already called from the worker.
//!
//! # Access control
//!
//! The run's *triggering* actor (`job.payload["actor"]`, `run_actor` below) — never
//! the synthetic `workflow/<name>` identity the run's own writes are stamped with — is
//! what a project's `agent_access` is checked against. [`crate::backend::LocalBackend::run_workflow`]
//! checks it once, at trigger time, against whichever of `memory_writers`/`task_movers`
//! the output node could actually exercise (`propose_memories`/`file_tasks`), so an
//! actor a project has not admitted cannot obtain a write it could not make directly by
//! routing it through someone else's workflow. `apply_output` here also passes
//! `run_actor` (not `workflow_actor`) to `Backend::remember`, so `require_review` reads
//! the triggering actor's own exemption (the user's own hands are always exempt) rather
//! than always applying, which `workflow/<name>` — never a user identity — would.

use crate::backend::{Backend, LocalBackend};
use crate::db::Db;
use crate::extract::{self, ExtractionConfig};
use crate::jobs::Job;
use crate::library::{AgentRepo, DocRepo};
use crate::models::*;
use crate::service::MemoryService;
use crate::{AtlasError, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use super::{graph, WorkflowRepo};

/// A candidate this confident (or more) may be stored active outright, the same rule
/// `extract::run_ingest` applies to extracted memories.
fn status_for(confidence: f64, threshold: f64) -> MemoryStatus {
    if confidence >= threshold { MemoryStatus::Active } else { MemoryStatus::Pending }
}

/// The trailing fenced block in `text`, whether or not it is tagged ` ```json `. Only
/// the block that is genuinely last counts: nothing but whitespace may follow its
/// closing fence, so a `json`-tagged block earlier in the reply, followed later by an
/// untagged (or differently tagged) one, is correctly *not* selected — the untagged one
/// is the trailing block, and it is read whether or not it says `json`. `None` when
/// there is no fence at all, when non-whitespace text follows the last one, or when its
/// body does not parse as JSON.
fn trailing_json_block(text: &str) -> Option<Value> {
    let trimmed = text.trim_end();
    let before_close = trimmed.strip_suffix("```")?;
    let fence_start = before_close.rfind("```")?;
    let body = &before_close[fence_start + 3..];
    let body = body.strip_prefix("json").unwrap_or(body);
    serde_json::from_str(body.trim()).ok()
}

/// Records a failure on a step that never got to run: a synthetic step (there is no
/// action to attach it to) carrying one ERR line, so a graph that fails re-validation
/// still leaves a readable trail in the run view. Takes the repos directly, not
/// `&LocalBackend`, so it can run inside a `blocking` closure that owns cloned `Arc`s
/// rather than a borrow of the backend.
fn fail_before_steps(workflows: &WorkflowRepo, memories: &MemoryService, run_id: Uuid, run_actor: &str, err: AtlasError) -> Result<Value> {
    let log = vec![LogLine::now(LogLevel::Error, err.to_string())];
    if let Ok(step) = workflows.append_step(run_id, 0, "", "validate", "") {
        let _ = workflows.finish_step(step.id, StepStatus::Failed, None, &log);
    }
    let _ = workflows.set_run_status(run_id, RunStatus::Failed, None);
    let _ = memories.audit(run_actor, "run", "workflow_run", Some(run_id), json!({"status": "failed", "reason": err.to_string()}));
    Err(err)
}

/// Records a failure raised while a step was in flight: the step's own log gets the
/// ERR line, the run is marked failed, and the rest of the actions are skipped.
fn fail_step(workflows: &WorkflowRepo, memories: &MemoryService, run_id: Uuid, step_id: Uuid, run_actor: &str, mut log: Vec<LogLine>, err: AtlasError) -> Result<Value> {
    log.push(LogLine::now(LogLevel::Error, err.to_string()));
    let _ = workflows.finish_step(step_id, StepStatus::Failed, None, &log);
    let _ = workflows.set_run_status(run_id, RunStatus::Failed, None);
    let _ = memories.audit(run_actor, "run", "workflow_run", Some(run_id), json!({"status": "failed", "reason": err.to_string()}));
    Err(err)
}

/// Reports a run found (or left) cancelled: an audit row recording why, and the
/// `cancelled` result the caller returns as-is. Does not itself touch `workflow_runs`:
/// either `cancel_run` already set the terminal status, or `set_run_status`'s own
/// compare-and-swap will refuse to overwrite it a moment later, so there is nothing
/// left to write here but the record of what happened.
fn cancelled_result(memories: &MemoryService, run_id: Uuid, run_actor: &str, steps: usize) -> Value {
    let _ = memories.audit(
        run_actor,
        "run",
        "workflow_run",
        Some(run_id),
        json!({"status": "cancelled", "reason": "the run was cancelled", "steps": steps}),
    );
    json!({"status": "cancelled", "steps": steps})
}

/// The memories an action sees, as the bullet list its user message carries: the
/// action's own `MemorySource` when it named one, else a source with no filters
/// bounded to the workflow's own project. Kinds and tags narrow, `limit` caps.
/// `workflow_project_id` is `Workflow::project_id`, passed separately so this can run
/// inside a `blocking` closure without borrowing the whole `Workflow`.
fn memories_section(memories: &MemoryService, workflow_project_id: Option<Uuid>, source: &Option<MemorySource>) -> String {
    let source = source.clone().unwrap_or(MemorySource { kinds: vec![], tags: vec![], limit: 20, project_id: workflow_project_id });
    let project_id = source.project_id.or(workflow_project_id);
    let mut memories = memories.list(MemoryStatus::Active, None, project_id).unwrap_or_default();
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
/// resolve produces a WARN line rather than failing the step: the action still runs,
/// just without that guidance. Returns the warnings instead of pushing into a
/// caller's `log` so this can run inside a `blocking` closure that owns no borrow of
/// the caller's state; the caller extends its own log with what comes back.
fn system_prompt(db: &Db, agent: &str, practices: &[String]) -> (String, Vec<LogLine>) {
    let mut warnings = Vec::new();
    let mut system = match AgentRepo::new(db).get(agent) {
        Ok(a) => a.instructions,
        Err(_) => format!("You are {agent}, an agent working inside Atlas."),
    };
    let docs = DocRepo::new(db, DocKind::Practice);
    for name in practices {
        match docs.get(name) {
            Ok(doc) => system.push_str(&format!("\n\n## Practice: {}\n\n{}", doc.name, doc.body)),
            Err(_) => warnings.push(LogLine::now(LogLevel::Warn, format!("practice '{name}' not found, skipping"))),
        }
    }
    (system, warnings)
}

/// The parts of `apply_output` that stay the same across every candidate memory and
/// task it writes, bundled so the function itself does not have to take them one by
/// one.
///
/// `workflow_actor` (`workflow/<name>`) labels the write — a memory's `source_agent`
/// and a task's `created_by` — so a reviewer can trace it back to the workflow that
/// made it. `run_actor` (the actor that triggered the run) is what the memory-write
/// gate and `require_review` are evaluated against: access to this project was already
/// checked against it at trigger time, and using it here too (rather than
/// `workflow_actor`, which is never a user identity) is what lets a human's own manual
/// run skip `require_review` the same way any other action of theirs would.
#[derive(Clone, Copy)]
struct OutputContext<'a> {
    workflow: &'a Workflow,
    workflow_actor: &'a str,
    run_actor: &'a str,
    threshold: f64,
}

/// Turns the last action's trailing JSON block into pending or active memories and
/// filed tasks, per the output node's flags. A block that does not parse, or is not
/// there when one was expected, is a WARN, not a failure: the run has already
/// succeeded by the time this runs.
async fn apply_output(backend: &LocalBackend, ctx: &OutputContext<'_>, output: &NodeData, last_output: &str, log: &mut Vec<LogLine>) -> (usize, usize) {
    let OutputContext { workflow, workflow_actor, run_actor, threshold } = *ctx;
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
                            source_agent: Some(workflow_actor.to_string()),
                            source_tool: Some("workflow".to_string()),
                            confidence: c.confidence,
                            status: status_for(c.confidence, threshold),
                        };
                        match backend.remember(new, run_actor).await {
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
                    source_ref: None,
                };
                match backend.create_task(new, workflow_actor).await {
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

    let (run, _) = {
        let workflows = backend.workflows.clone();
        backend.blocking(move || workflows.get_run(run_id)).await?
    };
    if run.status == RunStatus::Cancelled {
        let memories = backend.memories.clone();
        let actor = run_actor.clone();
        return backend.blocking(move || Ok(cancelled_result(&memories, run_id, &actor, 0))).await;
    }

    let workflow = {
        let workflows = backend.workflows.clone();
        let wid = workflow_id.to_string();
        backend.blocking(move || workflows.get(&wid)).await?
    };
    let order = match graph::validate(&workflow.graph) {
        Ok(order) => order,
        Err(e) => {
            let workflows = backend.workflows.clone();
            let memories = backend.memories.clone();
            let actor = run_actor.clone();
            return backend.blocking(move || fail_before_steps(&workflows, &memories, run_id, &actor, e)).await;
        }
    };

    {
        let workflows = backend.workflows.clone();
        backend.blocking(move || workflows.set_run_status(run_id, RunStatus::Running, None)).await?;
    }
    let workflow_actor = format!("workflow/{}", workflow.name);

    let mut last_output: Option<String> = None;
    let mut last_threshold = 1.0f64;
    let mut last_step: Option<WorkflowStep> = None;
    for (i, action_id) in order.iter().enumerate() {
        let (run, _) = {
            let workflows = backend.workflows.clone();
            backend.blocking(move || workflows.get_run(run_id)).await?
        };
        if run.status == RunStatus::Cancelled {
            let memories = backend.memories.clone();
            let actor = run_actor.clone();
            return backend.blocking(move || Ok(cancelled_result(&memories, run_id, &actor, i))).await;
        }

        let node = workflow.graph.nodes.iter().find(|n| &n.id == action_id).expect("action id came from graph::validate");
        let NodeData::Action { name, instructions, agent, practices, memories: memory_source } = &node.data else {
            unreachable!("graph::validate matched data to kind")
        };
        let step = {
            let workflows = backend.workflows.clone();
            let (pos, aid, name, agent) = (i as i32, action_id.clone(), name.clone(), agent.clone());
            backend.blocking(move || workflows.append_step(run_id, pos, &aid, &name, &agent)).await?
        };
        let mut log: Vec<LogLine> = vec![];

        let cfg: ExtractionConfig = {
            let db = backend.db.clone();
            let project_id = workflow.project_id;
            match backend.blocking(move || extract::resolve_extraction(&db, project_id)).await {
                Ok(cfg) => cfg,
                Err(e) => {
                    let workflows = backend.workflows.clone();
                    let memories = backend.memories.clone();
                    let actor = run_actor.clone();
                    let step_id = step.id;
                    return backend.blocking(move || fail_step(&workflows, &memories, run_id, step_id, &actor, log, e)).await;
                }
            }
        };
        last_threshold = cfg.auto_accept_min_confidence;
        let client = match extract::build_client(&cfg) {
            Ok(c) => c,
            Err(e) => {
                let workflows = backend.workflows.clone();
                let memories = backend.memories.clone();
                let actor = run_actor.clone();
                let step_id = step.id;
                return backend.blocking(move || fail_step(&workflows, &memories, run_id, step_id, &actor, log, e)).await;
            }
        };

        // `system_prompt` and `memories_section` only read the Db, so they share one
        // blocking round trip; `system_prompt`'s warnings come back rather than being
        // pushed into `log` directly, since the closure cannot borrow it.
        let (system, prompt_warnings, memories_block) = {
            let db = backend.db.clone();
            let memories_svc = backend.memories.clone();
            let agent = agent.clone();
            let practices = practices.clone();
            let source = memory_source.clone();
            let project_id = workflow.project_id;
            backend.blocking(move || {
                let (system, prompt_warnings) = system_prompt(&db, &agent, &practices);
                let memories_block = memories_section(&memories_svc, project_id, &source);
                Ok((system, prompt_warnings, memories_block))
            }).await?
        };
        log.extend(prompt_warnings);
        let mut user = instructions.clone();
        if i == 0 {
            if let Some(inp) = &input {
                user.push_str(&format!("\n\nInput: {inp}"));
            }
        }
        user.push_str("\n\nMemories:\n");
        user.push_str(&memories_block);
        user.push_str("\n\nPrevious step output:\n");
        user.push_str(last_output.as_deref().unwrap_or("none"));

        log.push(LogLine::now(LogLevel::Info, format!("request: system {} chars, user {} chars", system.chars().count(), user.chars().count())));
        let reply = match client.chat(&system, &user).await {
            Ok(r) => r,
            Err(e) => {
                let workflows = backend.workflows.clone();
                let memories = backend.memories.clone();
                let actor = run_actor.clone();
                let step_id = step.id;
                return backend.blocking(move || fail_step(&workflows, &memories, run_id, step_id, &actor, log, e)).await;
            }
        };
        log.push(LogLine::now(LogLevel::Info, format!("reply: {} chars", reply.chars().count())));

        last_step = Some({
            let workflows = backend.workflows.clone();
            let step_id = step.id;
            let (reply_c, log_c) = (reply.clone(), log.clone());
            backend.blocking(move || workflows.finish_step(step_id, StepStatus::Success, Some(&reply_c), &log_c)).await?
        });
        last_output = Some(reply);
    }

    // The last action's `client.chat().await` can still be in flight when a cancel
    // lands: nothing inside the loop observes that until its *next* iteration, and
    // there is no next iteration after the last action. Re-read the run here, before
    // any output is proposed or filed and before the run is marked done, so a run
    // cancelled during (or immediately after) its last step ends `cancelled`, not
    // `success`.
    let (run, _) = {
        let workflows = backend.workflows.clone();
        backend.blocking(move || workflows.get_run(run_id)).await?
    };
    if run.status == RunStatus::Cancelled {
        let memories = backend.memories.clone();
        let actor = run_actor.clone();
        let steps = order.len();
        return backend.blocking(move || Ok(cancelled_result(&memories, run_id, &actor, steps))).await;
    }

    let output_node = workflow.graph.nodes.iter().find(|n| n.kind == NodeKind::Output).expect("graph::validate requires exactly one output");
    let mut output_log: Vec<LogLine> = vec![];
    let output_ctx = OutputContext { workflow: &workflow, workflow_actor: &workflow_actor, run_actor: &run_actor, threshold: last_threshold };
    let (memories_proposed, tasks_filed) = match &last_output {
        Some(out) => apply_output(backend, &output_ctx, &output_node.data, out, &mut output_log).await,
        None => (0, 0),
    };
    // The output step's log is folded into the last action's own step rather than a
    // fresh one: it made no model call of its own, and a run view reader expects one
    // row per action node, not an extra row for bookkeeping.
    if let Some(last) = last_step.filter(|_| !output_log.is_empty()) {
        let workflows = backend.workflows.clone();
        let mut combined = last.log.clone();
        combined.extend(output_log);
        let (last_id, last_status, last_out) = (last.id, last.status, last.output.clone());
        backend.blocking(move || {
            let _ = workflows.finish_step(last_id, last_status, last_out.as_deref(), &combined);
            Ok(())
        }).await?;
    }

    let summary = json!({
        "steps": order.len(),
        "memories_proposed": memories_proposed,
        "tasks_filed": tasks_filed,
        "trigger": run.trigger.as_str(),
        "tokens": Value::Null,
    });
    // `set_run_status`'s own compare-and-swap refuses to move a run that is already
    // terminal (a cancellation that landed in the instant between the re-read above and
    // this call): the audit row still records what this run *tried* to finish as, but
    // the stored status stays whatever it already was.
    {
        let workflows = backend.workflows.clone();
        let memories = backend.memories.clone();
        let actor = run_actor.clone();
        let (summary_for_run, summary_for_audit) = (summary.clone(), summary.clone());
        backend.blocking(move || {
            workflows.set_run_status(run_id, RunStatus::Success, Some(summary_for_run))?;
            memories.audit(&actor, "run", "workflow_run", Some(run_id), summary_for_audit)?;
            Ok(())
        }).await?;
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_json_block_reads_a_bare_fence_with_no_json_tag() {
        let text = "Some notes.\n\n```\n{\"memories\": []}\n```";
        let block = trailing_json_block(text).expect("an untagged trailing fence should still parse");
        assert_eq!(block["memories"], serde_json::json!([]));
    }

    #[test]
    fn trailing_json_block_reads_a_json_tagged_fence() {
        let text = "Done.\n\n```json\n{\"tasks\": []}\n```";
        let block = trailing_json_block(text).expect("a json-tagged trailing fence should parse");
        assert_eq!(block["tasks"], serde_json::json!([]));
    }

    /// A `json`-tagged block that is *not* the last thing in the reply must not be
    /// picked over the block that actually trails it: here that trailing block is a
    /// plain (non-JSON) fence, so the whole reply carries no usable block at all.
    #[test]
    fn a_json_fence_earlier_in_the_reply_is_not_mistaken_for_the_trailing_block() {
        let text = "```json\n{\"memories\": [{\"text\": \"a\", \"kind\": \"fact\"}]}\n```\n\nAlso, don't do this:\n\n```\nrm -rf /\n```";
        assert!(trailing_json_block(text).is_none(), "the shell fence trails the json one and is not JSON");
    }

    /// Prose after the closing fence means the fence is not trailing at all, whatever
    /// it is tagged.
    #[test]
    fn text_after_the_closing_fence_means_there_is_no_trailing_block() {
        let text = "```json\n{\"memories\": []}\n```\n\nOne more thing to say.";
        assert!(trailing_json_block(text).is_none());
    }

    #[test]
    fn no_fence_at_all_is_none() {
        assert!(trailing_json_block("just a plain reply").is_none());
    }
}
