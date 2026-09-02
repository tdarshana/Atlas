//! The TUI's state machine: pure data in, new state and a list of effects out.
//!
//! Nothing here touches the terminal or the daemon. The event loop turns key
//! presses and completed requests into [`Action`]s, hands them to [`reduce`],
//! and runs whatever [`Effect`]s come back.

use atlas_core::models::{
    Agent, Doc, DocKind, Memory, MemoryStatus, Project, ProjectContext, RecallHit, StatusReport,
    SyncReport,
};
use crossterm::event::{KeyCode, KeyModifiers};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Memories,
    Projects,
    Agents,
    Practices,
    Workflows,
    Review,
    Status,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Memories,
        Tab::Projects,
        Tab::Agents,
        Tab::Practices,
        Tab::Workflows,
        Tab::Review,
        Tab::Status,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Memories => "Memories",
            Tab::Projects => "Projects",
            Tab::Agents => "Agents",
            Tab::Practices => "Practices",
            Tab::Workflows => "Workflows",
            Tab::Review => "Review",
            Tab::Status => "Status",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    fn shifted(self, by: usize) -> Tab {
        Self::ALL[(self.index() + by) % Self::ALL.len()]
    }

    fn next(self) -> Tab {
        self.shifted(1)
    }

    fn prev(self) -> Tab {
        self.shifted(Self::ALL.len() - 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Search,
}

#[derive(Debug, Clone)]
pub struct App {
    pub tab: Tab,
    pub focus: Focus,
    pub quit: bool,
    pub message: Option<String>,
    pub confirm_forget: bool,
    pub query: String,
    pub memories: Vec<RecallHit>,
    pub memories_sel: usize,
    pub projects: Vec<Project>,
    pub projects_sel: usize,
    pub project_context: Option<ProjectContext>,
    pub agents: Vec<Agent>,
    pub agents_sel: usize,
    pub last_sync: Option<SyncReport>,
    pub practices: Vec<Doc>,
    pub practices_sel: usize,
    pub workflows: Vec<Doc>,
    pub workflows_sel: usize,
    pub pending: Vec<Memory>,
    pub pending_sel: usize,
    pub status: Option<StatusReport>,
    /// Effects issued but not yet answered by an action. Startup's batch is
    /// counted in by [`note_effects_started`]; everything `reduce` issues or
    /// resolves counts itself.
    pub in_flight: usize,
}

impl App {
    /// Whether anything issued from this state is still outstanding.
    pub fn is_loading(&self) -> bool {
        self.in_flight > 0
    }
}

impl Default for App {
    fn default() -> Self {
        Self {
            tab: Tab::Memories,
            focus: Focus::List,
            quit: false,
            message: None,
            confirm_forget: false,
            query: String::new(),
            memories: vec![],
            memories_sel: 0,
            projects: vec![],
            projects_sel: 0,
            project_context: None,
            agents: vec![],
            agents_sel: 0,
            last_sync: None,
            practices: vec![],
            practices_sel: 0,
            workflows: vec![],
            workflows_sel: 0,
            pending: vec![],
            pending_sel: 0,
            status: None,
            in_flight: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Key(KeyCode, KeyModifiers),
    Tick,
    MemoriesLoaded(Vec<RecallHit>),
    ProjectsLoaded(Vec<Project>),
    ProjectContextLoaded(ProjectContext),
    AgentsLoaded(Vec<Agent>),
    SyncDone(SyncReport),
    DocsLoaded(DocKind, Vec<Doc>),
    PendingLoaded(Vec<Memory>),
    StatusLoaded(StatusReport),
    MemoryForgotten(Uuid),
    MemoryStatusChanged(Memory),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Recall(String),
    ListProjects,
    ConnectCwd,
    ProjectContext(PathBuf),
    ListAgents,
    Sync(Option<PathBuf>),
    ListDocs(DocKind),
    ListPending,
    Status,
    Forget(Uuid),
    SetStatus(Uuid, MemoryStatus),
}

/// Everything the TUI loads once at startup, in the order the tabs appear.
pub fn initial_effects() -> Vec<Effect> {
    vec![
        Effect::Status,
        Effect::Recall(String::new()),
        Effect::ListProjects,
        Effect::ListAgents,
        Effect::ListDocs(DocKind::Practice),
        Effect::ListDocs(DocKind::Workflow),
        Effect::ListPending,
    ]
}

/// Counts `n` effects as started. For effects `reduce` returns, `reduce`
/// counts them itself; this is for the startup batch `run.rs` spawns outside
/// `reduce`, via [`initial_effects`].
pub fn note_effects_started(app: &mut App, n: usize) {
    app.in_flight += n;
}

pub fn reduce(app: &mut App, action: Action) -> Vec<Effect> {
    let effects = match action {
        Action::Key(code, mods) => on_key(app, code, mods),
        Action::Tick => vec![],
        Action::MemoriesLoaded(hits) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.memories = hits;
            clamp(&mut app.memories_sel, app.memories.len());
            vec![]
        }
        Action::ProjectsLoaded(projects) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.projects = projects;
            clamp(&mut app.projects_sel, app.projects.len());
            vec![]
        }
        Action::ProjectContextLoaded(ctx) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.project_context = Some(ctx);
            vec![]
        }
        Action::AgentsLoaded(agents) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.agents = agents;
            clamp(&mut app.agents_sel, app.agents.len());
            vec![]
        }
        Action::SyncDone(report) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.last_sync = Some(report);
            vec![]
        }
        Action::DocsLoaded(DocKind::Practice, docs) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.practices = docs;
            clamp(&mut app.practices_sel, app.practices.len());
            vec![]
        }
        Action::DocsLoaded(DocKind::Workflow, docs) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.workflows = docs;
            clamp(&mut app.workflows_sel, app.workflows.len());
            vec![]
        }
        Action::PendingLoaded(pending) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.pending = pending;
            clamp(&mut app.pending_sel, app.pending.len());
            vec![]
        }
        Action::StatusLoaded(status) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.status = Some(status);
            vec![]
        }
        Action::MemoryForgotten(id) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.memories.retain(|h| h.memory.id != id);
            clamp(&mut app.memories_sel, app.memories.len());
            vec![]
        }
        Action::MemoryStatusChanged(memory) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            let accepted = memory.status == MemoryStatus::Active;
            app.pending.retain(|m| m.id != memory.id);
            clamp(&mut app.pending_sel, app.pending.len());
            // Accepting a pending memory makes it active, so the Memories
            // list is stale until it refreshes.
            if accepted {
                vec![Effect::Recall(app.query.clone())]
            } else {
                vec![]
            }
        }
        Action::Error(msg) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.message = Some(msg);
            vec![]
        }
    };
    app.in_flight += effects.len();
    effects
}

/// Keeps a selection inside a list that may have shrunk or emptied.
fn clamp(sel: &mut usize, len: usize) {
    *sel = if len == 0 { 0 } else { (*sel).min(len - 1) };
}

fn on_key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> Vec<Effect> {
    // Raw mode turns Ctrl+C and Ctrl+D into ordinary key events instead of
    // sending SIGINT, so the loop has to quit on them itself.
    if mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c') | KeyCode::Char('d')) {
        app.quit = true;
        return vec![];
    }
    if app.focus == Focus::Search {
        return search_key(app, code);
    }
    // The confirmation swallows the next key whatever it is: `y` forgets, the rest cancel.
    if app.confirm_forget {
        app.confirm_forget = false;
        return match (code, app.memories.get(app.memories_sel)) {
            (KeyCode::Char('y'), Some(hit)) => vec![Effect::Forget(hit.memory.id)],
            _ => vec![],
        };
    }
    match code {
        KeyCode::Tab => app.tab = app.tab.next(),
        KeyCode::BackTab => app.tab = app.tab.prev(),
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('j') | KeyCode::Down => move_sel(app, 1),
        KeyCode::Char('k') | KeyCode::Up => move_sel(app, -1),
        KeyCode::Esc => app.message = None,
        KeyCode::Char('r') => return vec![load_effect(app)],
        _ => return tab_key(app, code),
    }
    vec![]
}

fn search_key(app: &mut App, code: KeyCode) -> Vec<Effect> {
    match code {
        KeyCode::Char(c) => app.query.push(c),
        KeyCode::Backspace => {
            app.query.pop();
        }
        KeyCode::Esc => app.focus = Focus::List,
        KeyCode::Enter => {
            app.focus = Focus::List;
            return vec![Effect::Recall(app.query.clone())];
        }
        _ => {}
    }
    vec![]
}

/// Keys that only mean something on one tab.
fn tab_key(app: &mut App, code: KeyCode) -> Vec<Effect> {
    match (app.tab, code) {
        (Tab::Memories, KeyCode::Char('/')) => {
            app.focus = Focus::Search;
            vec![]
        }
        (Tab::Memories, KeyCode::Char('f')) => {
            app.confirm_forget = app.memories.get(app.memories_sel).is_some();
            vec![]
        }
        (Tab::Projects, KeyCode::Char('c')) => vec![Effect::ConnectCwd],
        (Tab::Projects, KeyCode::Enter) => match selected_project_root(app) {
            Some(root) => vec![Effect::ProjectContext(root)],
            None => vec![],
        },
        (Tab::Agents, KeyCode::Char('s')) => vec![Effect::Sync(selected_project_root(app))],
        (Tab::Review, KeyCode::Char('a')) => set_status(app, MemoryStatus::Active),
        (Tab::Review, KeyCode::Char('x')) => set_status(app, MemoryStatus::Rejected),
        _ => vec![],
    }
}

fn set_status(app: &App, status: MemoryStatus) -> Vec<Effect> {
    match app.pending.get(app.pending_sel) {
        Some(m) => vec![Effect::SetStatus(m.id, status)],
        None => vec![],
    }
}

/// A sync from the Agents tab targets whatever project the Projects tab has
/// selected; with no projects at all it syncs the home directory instead.
fn selected_project_root(app: &App) -> Option<PathBuf> {
    app.projects.get(app.projects_sel).map(|p| PathBuf::from(&p.root_path))
}

/// What `r` re-issues, and what the loop runs after a write on this tab.
fn load_effect(app: &App) -> Effect {
    match app.tab {
        Tab::Memories => Effect::Recall(app.query.clone()),
        Tab::Projects => Effect::ListProjects,
        Tab::Agents => Effect::ListAgents,
        Tab::Practices => Effect::ListDocs(DocKind::Practice),
        Tab::Workflows => Effect::ListDocs(DocKind::Workflow),
        Tab::Review => Effect::ListPending,
        Tab::Status => Effect::Status,
    }
}

fn move_sel(app: &mut App, delta: isize) {
    let len = match app.tab {
        Tab::Memories => app.memories.len(),
        Tab::Projects => app.projects.len(),
        Tab::Agents => app.agents.len(),
        Tab::Practices => app.practices.len(),
        Tab::Workflows => app.workflows.len(),
        Tab::Review => app.pending.len(),
        Tab::Status => 0,
    };
    let sel = match app.tab {
        Tab::Memories => &mut app.memories_sel,
        Tab::Projects => &mut app.projects_sel,
        Tab::Agents => &mut app.agents_sel,
        Tab::Practices => &mut app.practices_sel,
        Tab::Workflows => &mut app.workflows_sel,
        Tab::Review => &mut app.pending_sel,
        Tab::Status => return,
    };
    if len == 0 {
        *sel = 0;
    } else if delta > 0 {
        *sel = (*sel + 1).min(len - 1);
    } else {
        *sel = sel.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::models::*;
    use chrono::Utc;
    use std::path::Path;
    use uuid::Uuid;

    fn key(c: char) -> Action { Action::Key(KeyCode::Char(c), KeyModifiers::NONE) }
    fn code(c: KeyCode) -> Action { Action::Key(c, KeyModifiers::NONE) }

    fn mem(text: &str) -> Memory {
        Memory {
            id: Uuid::new_v4(),
            scope: MemoryScope::Global,
            project_id: None,
            kind: MemoryKind::Fact,
            text: text.into(),
            tags: vec![],
            source_agent: None,
            source_tool: None,
            confidence: 1.0,
            status: MemoryStatus::Active,
            superseded_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn hit(text: &str) -> RecallHit { RecallHit { memory: mem(text), score: 0.0 } }

    fn project(root: &str) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: root.into(),
            root_path: root.into(),
            git_remote: None,
            profile: None,
            created_at: Utc::now(),
            last_seen_at: Utc::now(),
        }
    }

    #[test]
    fn tab_cycles_and_wraps() {
        let mut a = App::default();
        for _ in 0..7 {
            reduce(&mut a, code(KeyCode::Tab));
        }
        assert_eq!(a.tab, Tab::Memories);
        reduce(&mut a, code(KeyCode::BackTab));
        assert_eq!(a.tab, Tab::Status);
    }

    #[test]
    fn search_flow_emits_recall() {
        let mut a = App::default();
        reduce(&mut a, key('/'));
        assert_eq!(a.focus, Focus::Search);
        for ch in "bun".chars() {
            reduce(&mut a, key(ch));
        }
        assert_eq!(a.query, "bun");
        let eff = reduce(&mut a, code(KeyCode::Enter));
        assert!(matches!(eff.as_slice(), [Effect::Recall(q)] if q == "bun"));
        assert_eq!(a.focus, Focus::List);
    }

    #[test]
    fn search_backspace_and_escape() {
        let mut a = App::default();
        reduce(&mut a, key('/'));
        for ch in "bun".chars() {
            reduce(&mut a, key(ch));
        }
        reduce(&mut a, code(KeyCode::Backspace));
        assert_eq!(a.query, "bu");
        let eff = reduce(&mut a, code(KeyCode::Esc));
        assert!(eff.is_empty());
        assert_eq!(a.focus, Focus::List);
    }

    #[test]
    fn forget_requires_confirmation() {
        let mut a = App { memories: vec![hit("x")], ..Default::default() };
        assert!(reduce(&mut a, key('f')).is_empty());
        assert!(a.confirm_forget);
        let eff = reduce(&mut a, key('y'));
        assert!(matches!(eff.as_slice(), [Effect::Forget(_)]));
        assert!(!a.confirm_forget);
    }

    #[test]
    fn forget_cancelled_by_other_key() {
        let mut a = App { memories: vec![hit("x")], ..Default::default() };
        reduce(&mut a, key('f'));
        assert!(a.confirm_forget);
        let eff = reduce(&mut a, key('n'));
        assert!(eff.is_empty());
        assert!(!a.confirm_forget);
    }

    #[test]
    fn selection_clamps_after_load() {
        let mut a = App { memories_sel: 10, ..Default::default() };
        reduce(&mut a, Action::MemoriesLoaded(vec![hit("a"), hit("b")]));
        assert_eq!(a.memories_sel, 1);
        assert!(!a.is_loading());
        reduce(&mut a, Action::MemoriesLoaded(vec![]));
        assert_eq!(a.memories_sel, 0);
    }

    #[test]
    fn movement_is_safe_on_empty_lists() {
        let mut a = App::default();
        for _ in 0..7 {
            assert!(reduce(&mut a, key('j')).is_empty());
            assert!(reduce(&mut a, key('k')).is_empty());
            assert!(reduce(&mut a, code(KeyCode::Enter)).is_empty());
            assert!(reduce(&mut a, key('f')).is_empty());
            assert!(reduce(&mut a, key('a')).is_empty());
            assert!(reduce(&mut a, key('x')).is_empty());
            reduce(&mut a, code(KeyCode::Tab));
        }
        assert_eq!(a.tab, Tab::Memories);
    }

    #[test]
    fn review_accept_and_reject() {
        let m = mem("pending");
        let id = m.id;
        let mut a = App { tab: Tab::Review, pending: vec![m], ..Default::default() };
        let eff = reduce(&mut a, key('a'));
        assert!(matches!(eff.as_slice(), [Effect::SetStatus(i, MemoryStatus::Active)] if *i == id));
        let eff = reduce(&mut a, key('x'));
        assert!(matches!(eff.as_slice(), [Effect::SetStatus(i, MemoryStatus::Rejected)] if *i == id));
    }

    #[test]
    fn accepting_a_pending_memory_refreshes_the_memories_list() {
        let mut m = mem("promoted");
        m.status = MemoryStatus::Active;
        let mut a = App { query: "duckdb".into(), pending: vec![m.clone()], ..Default::default() };
        let eff = reduce(&mut a, Action::MemoryStatusChanged(m));
        assert_eq!(eff, vec![Effect::Recall("duckdb".into())]);
    }

    #[test]
    fn rejecting_a_pending_memory_does_not_refresh_the_memories_list() {
        let mut m = mem("rejected");
        m.status = MemoryStatus::Rejected;
        let mut a = App { pending: vec![m.clone()], ..Default::default() };
        let eff = reduce(&mut a, Action::MemoryStatusChanged(m));
        assert!(eff.is_empty());
    }

    #[test]
    fn projects_connect_and_open() {
        let mut a = App { tab: Tab::Projects, projects: vec![project("/tmp/one")], ..Default::default() };
        assert!(matches!(reduce(&mut a, key('c')).as_slice(), [Effect::ConnectCwd]));
        let eff = reduce(&mut a, code(KeyCode::Enter));
        assert!(matches!(eff.as_slice(), [Effect::ProjectContext(p)] if p == Path::new("/tmp/one")));
    }

    #[test]
    fn agents_sync_uses_selected_project_or_global() {
        let mut a = App {
            tab: Tab::Agents,
            projects: vec![project("/tmp/one"), project("/tmp/two")],
            projects_sel: 1,
            ..Default::default()
        };
        let eff = reduce(&mut a, key('s'));
        assert!(matches!(eff.as_slice(), [Effect::Sync(Some(p))] if p == Path::new("/tmp/two")));

        let mut b = App { tab: Tab::Agents, ..Default::default() };
        let eff = reduce(&mut b, key('s'));
        assert!(matches!(eff.as_slice(), [Effect::Sync(None)]));
    }

    #[test]
    fn reload_reissues_the_tabs_load_effect() {
        let cases = [
            (Tab::Projects, Effect::ListProjects),
            (Tab::Agents, Effect::ListAgents),
            (Tab::Practices, Effect::ListDocs(DocKind::Practice)),
            (Tab::Workflows, Effect::ListDocs(DocKind::Workflow)),
            (Tab::Review, Effect::ListPending),
            (Tab::Status, Effect::Status),
        ];
        for (tab, want) in cases {
            let mut a = App { tab, ..Default::default() };
            assert_eq!(reduce(&mut a, key('r')), vec![want]);
            assert!(a.is_loading());
        }
        let mut a = App { query: "bun".into(), ..Default::default() };
        assert_eq!(reduce(&mut a, key('r')), vec![Effect::Recall("bun".into())]);
    }

    #[test]
    fn error_sets_message_and_clears_in_flight() {
        let mut a = App { in_flight: 1, ..Default::default() };
        assert!(a.is_loading());
        let eff = reduce(&mut a, Action::Error("boom".into()));
        assert!(eff.is_empty());
        assert_eq!(a.message.as_deref(), Some("boom"));
        assert!(!a.is_loading());
    }

    #[test]
    fn ctrl_c_on_projects_tab_quits_without_an_effect() {
        let mut a = App { tab: Tab::Projects, ..Default::default() };
        let eff = reduce(&mut a, Action::Key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(eff.is_empty());
        assert!(a.quit);
    }

    #[test]
    fn ctrl_c_in_search_quits_without_touching_the_query() {
        let mut a = App::default();
        reduce(&mut a, key('/'));
        reduce(&mut a, key('a'));
        let eff = reduce(&mut a, Action::Key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(eff.is_empty());
        assert!(a.quit);
        assert_eq!(a.query, "a");
    }

    #[test]
    fn q_quits() {
        let mut a = App::default();
        reduce(&mut a, key('q'));
        assert!(a.quit);
    }

    #[test]
    fn q_in_search_types_instead_of_quitting() {
        let mut a = App::default();
        reduce(&mut a, key('/'));
        reduce(&mut a, key('q'));
        assert!(!a.quit);
        assert_eq!(a.query, "q");
    }

    #[test]
    fn initial_effects_cover_every_tab() {
        assert_eq!(
            initial_effects(),
            vec![
                Effect::Status,
                Effect::Recall(String::new()),
                Effect::ListProjects,
                Effect::ListAgents,
                Effect::ListDocs(DocKind::Practice),
                Effect::ListDocs(DocKind::Workflow),
                Effect::ListPending,
            ]
        );
    }
}
