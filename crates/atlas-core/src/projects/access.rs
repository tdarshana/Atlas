//! Who is acting and what they may do: the one place that classifies an actor label
//! and answers "may this actor do this action in this project" for every gate.
//!
//! The label itself stays a string at the edges (`X-Atlas-Actor`, the MCP
//! `source_tool/agent` pair, `workflow/<name>`, `import/<kind>`); `Actor::parse`
//! reads it and `Display` writes the same label back, so audit rows and events keep
//! the strings they have always carried.

use crate::db::Db;
use crate::models::{AgentAccess, PersonaAccess, PersonaRule, Project};
use crate::{AtlasError, Result};
use std::fmt;
use uuid::Uuid;

/// The persona an actor is acting as: the session persona the MCP router holds, or
/// the persona of the task it claimed. Only the id and slug travel; the access block
/// is read from the persona row at check time, so an edit takes effect at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaRef {
    pub id: Uuid,
    pub slug: String,
}

/// Who is acting: the parsed label plus, for an agent, the persona it is bound by.
/// The persona never shows in the label (`Display` writes the label alone), so audit
/// rows and events keep the strings they have always carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub kind: ActorKind,
    pub persona: Option<PersonaRef>,
}

/// The parsed form of an actor label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorKind {
    /// The user's own hands: `desktop`, `api`, `cli` and `cli/<name>` (`atlas task
    /// --as NAME`). Exempt from every `agent_access` rule.
    User(String),
    /// A coding agent, `<tool>` or `<tool>/<name>`. Every other label that names
    /// who is acting (`workflow/<name>`, `import/<kind>`, `sync`) is also an agent
    /// for access purposes: an allowlist admits it by full label or by tool.
    Agent { tool: String, name: Option<String> },
    /// The workflow scheduler, which fires on the user's own cron and so counts as
    /// the user's hands.
    Scheduler,
    /// A label that names nobody: empty, or with an empty tool or name around the
    /// slash (`/x`, `codex/`). Never exempt; kept verbatim so it still round-trips.
    Other(String),
}

impl ActorKind {
    /// Classifies `label` exactly as the gates always have: the user set is matched
    /// exactly rather than by prefix (`cli` alone would also exempt `cline`, a real
    /// coding agent), and anything else is an agent.
    pub fn parse(label: &str) -> ActorKind {
        let a = label.trim();
        if a == "desktop" || a == "api" || a == "cli" || a.starts_with("cli/") {
            return ActorKind::User(a.to_string());
        }
        if a == "scheduler" {
            return ActorKind::Scheduler;
        }
        match a.split_once('/') {
            None if a.is_empty() => ActorKind::Other(String::new()),
            None => ActorKind::Agent { tool: a.to_string(), name: None },
            Some((tool, name)) if tool.is_empty() || name.is_empty() => ActorKind::Other(a.to_string()),
            Some((tool, name)) => ActorKind::Agent { tool: tool.to_string(), name: Some(name.to_string()) },
        }
    }

    /// Whether this actor is exempt from a project's `agent_access` rules: the user's
    /// own hands and the scheduler.
    pub fn is_user(&self) -> bool {
        matches!(self, ActorKind::User(_) | ActorKind::Scheduler)
    }
}

impl Actor {
    /// Classifies `label` (see [`ActorKind::parse`]) with no persona.
    pub fn parse(label: &str) -> Actor {
        Actor { kind: ActorKind::parse(label), persona: None }
    }

    /// The same actor bound by `persona`. A user actor is never restricted by one,
    /// but the ref is kept so a caller can still report it.
    pub fn with_persona(mut self, persona: Option<PersonaRef>) -> Actor {
        self.persona = persona;
        self
    }

    /// Whether this actor is exempt from a project's `agent_access` rules and from
    /// any persona block: the user's own hands and the scheduler.
    pub fn is_user(&self) -> bool {
        self.kind.is_user()
    }
}

impl fmt::Display for ActorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorKind::User(s) | ActorKind::Other(s) => f.write_str(s),
            ActorKind::Agent { tool, name: Some(name) } => write!(f, "{tool}/{name}"),
            ActorKind::Agent { tool, name: None } => f.write_str(tool),
            ActorKind::Scheduler => f.write_str("scheduler"),
        }
    }
}

/// The label alone; the persona goes in audit `detail`, never in the actor string.
impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// A gated action. Each maps to the `AgentAccess` list it consults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Consults `memory_writers`; the decision also reports `require_review`.
    WriteMemory,
    /// Consults `task_movers`.
    MoveTask,
    /// Consults whichever lists the workflow's output node could exercise, memories
    /// first, so the refusal names the same list it always has.
    TriggerWorkflow { propose_memories: bool, file_tasks: bool },
}

/// An admitted action. `require_review` is only ever true for `WriteMemory` by an
/// agent in a project whose effective access demands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Decision {
    pub require_review: bool,
}

/// Whether `actor` may perform `action` in the project `project_id` names, as an
/// error when it may not. The user's own hands are never checked, and neither is
/// a write with no project against the project rules (there is no rule to apply);
/// otherwise the project and the global `access.*` defaults are read once and the
/// per-action rule decides. A project id that does not resolve is an error only for
/// an agent. The actor's persona block, when it carries one, is applied after the
/// agent rules with the stricter answer winning, project or not.
pub fn check(db: &Db, actor: &Actor, action: Action, project_id: Option<Uuid>) -> Result<Decision> {
    if actor.is_user() {
        return Ok(Decision::default());
    }
    let mut decision = match project_id {
        Some(pid) => check_project(db, actor, action, pid)?,
        None => Decision::default(),
    };
    if let Some(persona) = &actor.persona {
        check_persona(db, persona, action, &mut decision)?;
    }
    Ok(decision)
}

/// The project's `agent_access` rules for one agent action.
fn check_project(db: &Db, actor: &Actor, action: Action, pid: Uuid) -> Result<Decision> {
    let project = crate::projects::ProjectRepo::new(db).get(pid)?;
    let defaults = access_defaults(&crate::settings::SettingsRepo::new(db))?;
    let label = actor.to_string();
    match action {
        Action::WriteMemory => {
            check_memory_write(&label, &project, &defaults)?;
            Ok(Decision { require_review: effective_access(&project.agent_access, &defaults).require_review })
        }
        Action::MoveTask => {
            check_task_move(&label, &project, &defaults)?;
            Ok(Decision::default())
        }
        Action::TriggerWorkflow { propose_memories, file_tasks } => {
            if propose_memories {
                check_memory_write(&label, &project, &defaults)?;
            }
            if file_tasks {
                check_task_move(&label, &project, &defaults)?;
            }
            Ok(Decision::default())
        }
    }
}

/// The persona's own block on top of `decision`: `Deny` refuses, `Review` on a
/// memory write raises `require_review`, `Allow` changes nothing. A persona row that
/// is gone is `NotFound`, never a silent pass.
fn check_persona(db: &Db, persona: &PersonaRef, action: Action, decision: &mut Decision) -> Result<()> {
    let access = persona_access(db, persona)?;
    let (rule, verb) = match action {
        Action::WriteMemory => (access.memory_write, "write memories"),
        Action::MoveTask => (access.task_move, "move tasks"),
        Action::TriggerWorkflow { .. } => (access.workflow_trigger, "trigger workflows"),
    };
    match rule {
        PersonaRule::Deny => Err(AtlasError::Conflict(format!("persona '{}' may not {verb}", persona.slug))),
        PersonaRule::Review => {
            if action == Action::WriteMemory {
                decision.require_review = true;
            }
            Ok(())
        }
        PersonaRule::Allow => Ok(()),
    }
}

/// The access block of the persona row `persona` names.
fn persona_access(db: &Db, persona: &PersonaRef) -> Result<PersonaAccess> {
    db.with_conn(|c| {
        let mut st = c.prepare("select access::text from personas where id = ?")?;
        let mut rows = st.query(duckdb::params![persona.id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(serde_json::from_str(&r.get::<_, String>(0)?)?),
            None => Err(AtlasError::NotFound(format!("persona {}", persona.slug))),
        }
    })
}

/// Whether `actor` is the user's own hands rather than an agent, and so exempt from a
/// project's `agent_access` rules. See [`Actor::is_user`]; this is its string form.
pub fn actor_is_user(actor: &str) -> bool {
    Actor::parse(actor).is_user()
}

/// Whether `actor` is on `allowed`. A `None` list means any actor. A list matches the
/// full label (`claude-code/reviewer`) or the part before the slash (`claude-code`),
/// so a project can admit a tool without naming every agent it hosts.
fn allowed_by(allowed: &Option<Vec<String>>, actor: &str) -> bool {
    let Some(list) = allowed else { return true };
    let actor = actor.trim();
    let tool = actor.split_once('/').map(|(t, _)| t).unwrap_or(actor);
    list.iter().any(|l| l == actor || l == tool)
}

/// Resolves a project's access against the global defaults `access_defaults` reads.
/// Each list takes the project's own value when it is `Some`, else the default's;
/// `require_review` is a floor a project can only raise, never lower, so it is true
/// when either side is true.
pub fn effective_access(project: &AgentAccess, defaults: &AgentAccess) -> AgentAccess {
    AgentAccess {
        memory_writers: project.memory_writers.clone().or_else(|| defaults.memory_writers.clone()),
        task_movers: project.task_movers.clone().or_else(|| defaults.task_movers.clone()),
        require_review: project.require_review || defaults.require_review,
    }
}

/// Reads the three `access.*` settings keys into the global agent-access defaults.
/// An unset key reads as `AgentAccess::default()` (all-null, `require_review` false),
/// the same "admits anyone" meaning an unset project's own `agent_access` carries.
pub fn access_defaults(settings: &crate::settings::SettingsRepo) -> Result<AgentAccess> {
    let list = |key: &str| -> Result<Option<Vec<String>>> {
        match settings.get_raw(key)? {
            Some(v) => Ok(serde_json::from_value(v)?),
            None => Ok(None),
        }
    };
    Ok(AgentAccess {
        memory_writers: list("access.memory_writers")?,
        task_movers: list("access.task_movers")?,
        require_review: settings.get_raw("access.require_review")?.and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

/// Refuses an agent that may not write memories here, checked against the project's
/// own rule filled in by `defaults` where the project leaves a field unset.
pub fn check_memory_write(actor: &str, p: &Project, defaults: &AgentAccess) -> Result<()> {
    if actor_is_user(actor) || allowed_by(&effective_access(&p.agent_access, defaults).memory_writers, actor) {
        return Ok(());
    }
    Err(AtlasError::Conflict(format!("actor '{actor}' may not write memories in project {}", p.name)))
}

/// Refuses an agent that may not move tasks here, checked against the project's own
/// rule filled in by `defaults` where the project leaves a field unset.
pub fn check_task_move(actor: &str, p: &Project, defaults: &AgentAccess) -> Result<()> {
    if actor_is_user(actor) || allowed_by(&effective_access(&p.agent_access, defaults).task_movers, actor) {
        return Ok(());
    }
    Err(AtlasError::Conflict(format!("actor '{actor}' may not move tasks in project {}", p.name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewPersona;
    use crate::projects::{Detected, ProjectRepo};
    use std::sync::Arc;

    /// Every label shape the repo stamps on audit rows and events, and what it is.
    fn labels() -> Vec<(&'static str, ActorKind)> {
        let agent = |tool: &str, name: Option<&str>| ActorKind::Agent { tool: tool.into(), name: name.map(Into::into) };
        vec![
            ("desktop", ActorKind::User("desktop".into())),
            ("api", ActorKind::User("api".into())),
            ("cli", ActorKind::User("cli".into())),
            ("cli/claude-code", ActorKind::User("cli/claude-code".into())),
            ("cli/codex", ActorKind::User("cli/codex".into())),
            ("scheduler", ActorKind::Scheduler),
            ("claude-code", agent("claude-code", None)),
            ("codex", agent("codex", None)),
            ("claude-code/reviewer", agent("claude-code", Some("reviewer"))),
            ("stdio/claude-code", agent("stdio", Some("claude-code"))),
            ("test/agent-x", agent("test", Some("agent-x"))),
            ("import/superpowers", agent("import", Some("superpowers"))),
            ("sync", agent("sync", None)),
            ("workflow", agent("workflow", None)),
            ("workflow/nightly", agent("workflow", Some("nightly"))),
            ("extract", agent("extract", None)),
            // The user set is exact, not a prefix.
            ("cline", agent("cline", None)),
            ("clippy", agent("clippy", None)),
            ("client-x", agent("client-x", None)),
            ("cli-bot", agent("cli-bot", None)),
            ("desktop-agent", agent("desktop-agent", None)),
            ("", ActorKind::Other("".into())),
            ("/x", ActorKind::Other("/x".into())),
            ("codex/", ActorKind::Other("codex/".into())),
        ]
    }

    #[test]
    fn parse_classifies_every_label_shape_and_display_round_trips() {
        for (label, want) in labels() {
            let got = Actor::parse(label);
            assert_eq!(got.kind, want, "{label:?}");
            assert_eq!(got.to_string(), label, "{label:?} must round-trip");
        }
        assert_eq!(Actor::parse("  cli  ").kind, ActorKind::User("cli".into()), "labels are trimmed as the gates always trimmed them");
    }

    /// `is_user` is the exact set the old string rule admitted, nothing more.
    #[test]
    fn is_user_matches_the_old_exempt_set() {
        let exempt = ["desktop", "api", "cli", "scheduler"];
        for (label, actor) in labels() {
            assert_eq!(actor.is_user(), exempt.contains(&label) || label.starts_with("cli/"), "{label:?}");
            assert_eq!(actor_is_user(label), actor.is_user(), "{label:?}");
        }
    }

    fn set_access(db: &Db, access: AgentAccess) -> Project {
        let repo = ProjectRepo::new(db);
        let p = repo.upsert(&Detected { root: "/tmp/access".into(), remote: None }, None, "t").unwrap();
        repo.set_agent_access(p.id, &access, "t").unwrap()
    }

    /// `check` agrees with `check_task_move` and `check_memory_write` on every label,
    /// on the fixture the repository test uses.
    #[test]
    fn check_agrees_with_the_per_action_rules() {
        let db = Db::open_in_memory().unwrap();
        let p = set_access(
            &db,
            AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: Some(vec!["codex".into()]), require_review: true },
        );
        let defaults = AgentAccess::default();
        for (label, kind) in labels() {
            let actor = Actor { kind, persona: None };
            let moved = check(&db, &actor, Action::MoveTask, Some(p.id));
            assert_eq!(moved.is_ok(), check_task_move(label, &p, &defaults).is_ok(), "move by {label:?}");
            let wrote = check(&db, &actor, Action::WriteMemory, Some(p.id));
            assert_eq!(wrote.is_ok(), check_memory_write(label, &p, &defaults).is_ok(), "write by {label:?}");
            if let Ok(d) = wrote {
                assert_eq!(d.require_review, !actor.is_user(), "review is demanded of agents only: {label:?}");
            }
            if let Ok(d) = moved {
                assert!(!d.require_review, "a move never demands review: {label:?}");
            }
        }
        let Err(AtlasError::Conflict(msg)) = check(&db, &Actor::parse("cline"), Action::MoveTask, Some(p.id)) else { panic!() };
        assert_eq!(msg, format!("actor 'cline' may not move tasks in project {}", p.name));
        let Err(AtlasError::Conflict(msg)) = check(&db, &Actor::parse("codex"), Action::WriteMemory, Some(p.id)) else { panic!() };
        assert_eq!(msg, format!("actor 'codex' may not write memories in project {}", p.name));
    }

    /// Global `access.*` defaults fill in what the project leaves unset, exactly as
    /// `effective_access` resolves them.
    #[test]
    fn check_reads_the_global_defaults() {
        let db = Db::open_in_memory().unwrap();
        let p = set_access(&db, AgentAccess::default());
        crate::settings::SettingsRepo::new(&db)
            .set_many(
                &serde_json::Map::from_iter([
                    ("access.memory_writers".to_string(), serde_json::json!(["claude-code"])),
                    ("access.require_review".to_string(), serde_json::Value::from(true)),
                ]),
                "t",
            )
            .unwrap();
        let defaults = access_defaults(&crate::settings::SettingsRepo::new(&db)).unwrap();
        for label in ["claude-code", "claude-code/reviewer", "codex", "cli", "workflow/x"] {
            let actor = Actor::parse(label);
            let wrote = check(&db, &actor, Action::WriteMemory, Some(p.id));
            assert_eq!(wrote.is_ok(), check_memory_write(label, &p, &defaults).is_ok(), "{label:?}");
            if let Ok(d) = wrote {
                assert_eq!(d.require_review, !actor.is_user(), "{label:?}");
            }
            assert!(check(&db, &actor, Action::MoveTask, Some(p.id)).is_ok(), "an unset list admits anyone: {label:?}");
        }
    }

    /// A workflow trigger consults the lists its output node could exercise, memories
    /// first; an output that does neither is admitted.
    #[test]
    fn check_gates_a_workflow_trigger_by_what_its_output_does() {
        let db = Db::open_in_memory().unwrap();
        let p = set_access(
            &db,
            AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: Some(vec!["codex".into()]), require_review: false },
        );
        let trigger = |pm, ft| Action::TriggerWorkflow { propose_memories: pm, file_tasks: ft };
        let codex = Actor::parse("codex");
        let claude = Actor::parse("claude-code");
        assert!(check(&db, &codex, trigger(false, false), Some(p.id)).is_ok());
        assert!(check(&db, &codex, trigger(false, true), Some(p.id)).is_ok());
        assert!(matches!(check(&db, &codex, trigger(true, false), Some(p.id)), Err(AtlasError::Conflict(m)) if m.contains("may not write memories")));
        assert!(matches!(check(&db, &codex, trigger(true, true), Some(p.id)), Err(AtlasError::Conflict(m)) if m.contains("may not write memories")));
        assert!(check(&db, &claude, trigger(true, false), Some(p.id)).is_ok());
        assert!(matches!(check(&db, &claude, trigger(true, true), Some(p.id)), Err(AtlasError::Conflict(m)) if m.contains("may not move tasks")));
        assert!(check(&db, &Actor::parse("desktop"), trigger(true, true), Some(p.id)).is_ok());
        assert!(!check(&db, &claude, trigger(true, false), Some(p.id)).unwrap().require_review);
    }

    /// A persona with the given access block, as the ref an actor carries.
    fn persona(db: &Arc<Db>, name: &str, access: PersonaAccess) -> PersonaRef {
        let repo = crate::personas::PersonaRepo::new(db.clone(), Arc::new(std::sync::Mutex::new(())));
        let p = repo.create(&NewPersona { name: name.into(), access, ..Default::default() }, "t").unwrap();
        PersonaRef { id: p.id, slug: p.slug }
    }

    /// The persona block runs after the agent rules: `Deny` refuses an agent and
    /// names the persona, a user actor is never restricted, and `Review` on
    /// `memory_write` lands the memory pending.
    #[test]
    fn a_persona_denies_or_reviews_an_agent_and_never_a_user() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let p = set_access(&db, AgentAccess::default());
        let deny = persona(&db, "Locked Down", PersonaAccess { memory_write: PersonaRule::Deny, task_move: PersonaRule::Deny, workflow_trigger: PersonaRule::Deny });
        let review = persona(&db, "Careful", PersonaAccess { memory_write: PersonaRule::Review, ..Default::default() });

        let codex = Actor::parse("codex").with_persona(Some(deny.clone()));
        let Err(AtlasError::Conflict(msg)) = check(&db, &codex, Action::WriteMemory, Some(p.id)) else { panic!("deny must refuse a memory write") };
        assert_eq!(msg, "persona 'locked-down' may not write memories");
        let Err(AtlasError::Conflict(msg)) = check(&db, &codex, Action::MoveTask, Some(p.id)) else { panic!("deny must refuse a move") };
        assert_eq!(msg, "persona 'locked-down' may not move tasks");
        let trigger = Action::TriggerWorkflow { propose_memories: false, file_tasks: false };
        let Err(AtlasError::Conflict(msg)) = check(&db, &codex, trigger, Some(p.id)) else { panic!("deny must refuse a trigger") };
        assert_eq!(msg, "persona 'locked-down' may not trigger workflows");
        // The block is the persona's, not the project's, so a global write is refused too.
        assert!(matches!(check(&db, &codex, Action::WriteMemory, None), Err(AtlasError::Conflict(_))));

        for label in ["desktop", "api", "cli", "cli/x", "scheduler"] {
            let user = Actor::parse(label).with_persona(Some(deny.clone()));
            assert_eq!(check(&db, &user, Action::WriteMemory, Some(p.id)).unwrap(), Decision::default(), "{label:?}");
            assert!(check(&db, &user, Action::MoveTask, Some(p.id)).is_ok(), "{label:?}");
        }

        let careful = Actor::parse("codex").with_persona(Some(review));
        assert!(check(&db, &careful, Action::WriteMemory, Some(p.id)).unwrap().require_review);
        assert!(check(&db, &careful, Action::WriteMemory, None).unwrap().require_review, "review applies to a global write too");
        assert!(!check(&db, &careful, Action::MoveTask, Some(p.id)).unwrap().require_review);
        assert!(check(&db, &careful, trigger, Some(p.id)).is_ok());
    }

    /// An all-`Allow` persona changes nothing against the agent access fixtures: the
    /// agent rules still decide, and the project's `require_review` still holds.
    #[test]
    fn an_allow_everything_persona_leaves_the_agent_rules_alone() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let p = set_access(
            &db,
            AgentAccess { memory_writers: Some(vec!["claude-code".into()]), task_movers: Some(vec!["codex".into()]), require_review: true },
        );
        let open = persona(&db, "Open", PersonaAccess::default());
        let defaults = AgentAccess::default();
        for (label, _) in labels() {
            let plain = Actor::parse(label);
            let with = plain.clone().with_persona(Some(open.clone()));
            assert_eq!(plain.to_string(), with.to_string(), "the label never carries the persona: {label:?}");
            let moved = check(&db, &with, Action::MoveTask, Some(p.id));
            assert_eq!(moved.is_ok(), check_task_move(label, &p, &defaults).is_ok(), "move by {label:?}");
            let wrote = check(&db, &with, Action::WriteMemory, Some(p.id));
            assert_eq!(wrote.is_ok(), check_memory_write(label, &p, &defaults).is_ok(), "write by {label:?}");
            if let Ok(d) = wrote {
                assert_eq!(d.require_review, !with.is_user(), "{label:?}");
            }
        }
        // The agent rule still wins first, with its own message.
        let Err(AtlasError::Conflict(msg)) = check(&db, &Actor::parse("cline").with_persona(Some(open)), Action::MoveTask, Some(p.id)) else { panic!() };
        assert_eq!(msg, format!("actor 'cline' may not move tasks in project {}", p.name));
    }

    /// A persona that no longer exists is an error for an agent, never a silent pass.
    #[test]
    fn a_missing_persona_is_not_found() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let gone = PersonaRef { id: Uuid::new_v4(), slug: "gone".into() };
        let codex = Actor::parse("codex").with_persona(Some(gone.clone()));
        assert!(matches!(check(&db, &codex, Action::WriteMemory, None), Err(AtlasError::NotFound(m)) if m == "persona gone"));
        assert!(check(&db, &Actor::parse("desktop").with_persona(Some(gone)), Action::WriteMemory, None).is_ok());
    }

    /// No project means no rule; a project that does not resolve is an error for an
    /// agent and nothing at all for the user, who is never looked up.
    #[test]
    fn check_without_a_project_or_with_a_missing_one() {
        let db = Db::open_in_memory().unwrap();
        for label in ["codex", "desktop", "scheduler", ""] {
            let d = check(&db, &Actor::parse(label), Action::WriteMemory, None).unwrap();
            assert_eq!(d, Decision::default(), "{label:?}");
        }
        let missing = Some(Uuid::new_v4());
        assert!(matches!(check(&db, &Actor::parse("codex"), Action::MoveTask, missing), Err(AtlasError::NotFound(_))));
        assert!(matches!(check(&db, &Actor::parse(""), Action::MoveTask, missing), Err(AtlasError::NotFound(_))));
        for label in ["desktop", "api", "cli", "cli/x", "scheduler"] {
            assert!(check(&db, &Actor::parse(label), Action::WriteMemory, missing).is_ok(), "{label:?}");
        }
    }
}
