//! The TUI's state machine: pure data in, new state and a list of effects out.
//!
//! Nothing here touches the terminal or the daemon. The event loop turns key
//! presses and completed requests into [`Action`]s, hands them to [`reduce`],
//! and runs whatever [`Effect`]s come back.

use atlas_core::models::{
    Agent, Doc, DocKind, Memory, MemoryStatus, Project, ProjectContext, RecallHit, Stage,
    StatusReport, SyncReport, Task, TaskDetail,
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
    Board,
    Review,
    Status,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Memories,
        Tab::Projects,
        Tab::Agents,
        Tab::Practices,
        Tab::Workflows,
        Tab::Board,
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
            Tab::Board => "Board",
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

/// What the Board tab is waiting for. Every mode but `Normal` swallows keys, so
/// typing a comment cannot also quit the TUI or move a task.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BoardMode {
    #[default]
    Normal,
    /// The stages, numbered 1..n; a digit picks one.
    MovePicker,
    CommentPrompt(String),
    NewTaskPrompt(String),
}

/// The Board tab: the stage list, its tasks bucketed one vector per stage, the
/// cursor, and whatever the tab is in the middle of.
#[derive(Debug, Clone, Default)]
pub struct Board {
    pub stages: Vec<Stage>,
    pub columns: Vec<Vec<Task>>,
    pub col: usize,
    pub row: usize,
    pub detail: Option<TaskDetail>,
    pub mode: BoardMode,
}

impl Board {
    /// The task under the cursor, if the board has one.
    pub fn selected(&self) -> Option<&Task> {
        self.columns.get(self.col)?.get(self.row)
    }

    /// Buckets `tasks` into one column per stage and pulls the cursor back inside
    /// a board that may have fewer stages or fewer rows than it did.
    fn load(&mut self, stages: Vec<Stage>, tasks: Vec<Task>) {
        self.columns = stages
            .iter()
            .map(|s| tasks.iter().filter(|t| t.stage == s.name).cloned().collect())
            .collect();
        self.stages = stages;
        clamp(&mut self.col, self.stages.len());
        clamp(&mut self.row, self.columns.get(self.col).map(Vec::len).unwrap_or(0));
    }
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
    pub board: Board,
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
            board: Board::default(),
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
    /// Boxed: `ProjectContext` carries a whole `Project` and its memories, and an
    /// unboxed variant makes every `Action` as large as the biggest one.
    ProjectContextLoaded(Box<ProjectContext>),
    AgentsLoaded(Vec<Agent>),
    SyncDone(SyncReport),
    DocsLoaded(DocKind, Vec<Doc>),
    PendingLoaded(Vec<Memory>),
    /// The board's stages and every task on it, still to be bucketed.
    BoardLoaded(Vec<Stage>, Vec<Task>),
    /// Boxed for the same reason as `ProjectContextLoaded`: a `TaskDetail` carries
    /// a task, its children and its whole event log.
    TaskDetailLoaded(Box<TaskDetail>),
    /// A board write landed, so the columns are stale.
    BoardChanged,
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
    /// The stages and tasks of one project's board, or the global board.
    LoadBoard(Option<Uuid>),
    LoadTaskDetail(String),
    MoveTask { key: String, stage: String },
    CommentTask { key: String, body: String },
    CreateTask { title: String, project_id: Option<Uuid> },
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
            // The board follows the Projects tab's selection, so it cannot load
            // until the projects have; this is where it finds out which one.
            vec![Effect::LoadBoard(selected_project_id(app))]
        }
        Action::ProjectContextLoaded(ctx) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.project_context = Some(*ctx);
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
        Action::BoardLoaded(stages, tasks) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.board.load(stages, tasks);
            vec![]
        }
        Action::TaskDetailLoaded(detail) => {
            app.in_flight = app.in_flight.saturating_sub(1);
            app.board.detail = Some(*detail);
            vec![]
        }
        Action::BoardChanged => {
            app.in_flight = app.in_flight.saturating_sub(1);
            vec![Effect::LoadBoard(selected_project_id(app))]
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
    // A picker or a prompt owns every key while it is open, quit included.
    if app.tab == Tab::Board && app.board.mode != BoardMode::Normal {
        return board_prompt_key(app, code);
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
        // On the Board tab Esc backs out of the detail pane first; there is
        // nothing else it could mean while one is open.
        KeyCode::Esc if app.tab == Tab::Board && app.board.detail.is_some() => app.board.detail = None,
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
        (Tab::Board, KeyCode::Char('h') | KeyCode::Left) => {
            move_col(app, -1);
            vec![]
        }
        (Tab::Board, KeyCode::Char('l') | KeyCode::Right) => {
            move_col(app, 1);
            vec![]
        }
        (Tab::Board, KeyCode::Enter) => match app.board.selected() {
            Some(t) => vec![Effect::LoadTaskDetail(t.key.clone())],
            None => vec![],
        },
        (Tab::Board, KeyCode::Char('m')) => {
            // Nothing to move and nothing to move it to are both "no picker".
            if app.board.selected().is_some() && !app.board.stages.is_empty() {
                app.board.mode = BoardMode::MovePicker;
            }
            vec![]
        }
        (Tab::Board, KeyCode::Char('c')) => {
            if app.board.selected().is_some() {
                app.board.mode = BoardMode::CommentPrompt(String::new());
            }
            vec![]
        }
        (Tab::Board, KeyCode::Char('n')) => {
            app.board.mode = BoardMode::NewTaskPrompt(String::new());
            vec![]
        }
        (Tab::Review, KeyCode::Char('a')) => set_status(app, MemoryStatus::Active),
        (Tab::Review, KeyCode::Char('x')) => set_status(app, MemoryStatus::Rejected),
        _ => vec![],
    }
}

/// The Board tab while a picker or a prompt is open. Esc always backs out, and
/// the mode is left behind whenever a key finishes it.
fn board_prompt_key(app: &mut App, code: KeyCode) -> Vec<Effect> {
    if code == KeyCode::Esc {
        app.board.mode = BoardMode::Normal;
        return vec![];
    }
    match app.board.mode.clone() {
        BoardMode::Normal => vec![],
        BoardMode::MovePicker => {
            // The stages are numbered from 1, so 0 picks nothing.
            let KeyCode::Char(c) = code else { return vec![] };
            let Some(n) = c.to_digit(10).filter(|n| *n > 0) else { return vec![] };
            let (Some(stage), Some(task)) = (app.board.stages.get(n as usize - 1), app.board.selected()) else {
                return vec![];
            };
            let effect = Effect::MoveTask { key: task.key.clone(), stage: stage.name.clone() };
            app.board.mode = BoardMode::Normal;
            vec![effect]
        }
        BoardMode::CommentPrompt(text) => match typed(code, text) {
            Typed::Editing(text) => {
                app.board.mode = BoardMode::CommentPrompt(text);
                vec![]
            }
            Typed::Submitted(body) => {
                app.board.mode = BoardMode::Normal;
                match (body.trim().is_empty(), app.board.selected()) {
                    (false, Some(task)) => vec![Effect::CommentTask { key: task.key.clone(), body }],
                    _ => vec![],
                }
            }
        },
        BoardMode::NewTaskPrompt(text) => match typed(code, text) {
            Typed::Editing(text) => {
                app.board.mode = BoardMode::NewTaskPrompt(text);
                vec![]
            }
            Typed::Submitted(title) => {
                app.board.mode = BoardMode::Normal;
                if title.trim().is_empty() {
                    return vec![];
                }
                vec![Effect::CreateTask { title, project_id: selected_project_id(app) }]
            }
        },
    }
}

enum Typed {
    Editing(String),
    Submitted(String),
}

/// One key of a text prompt. Anything that is not a character, a backspace or
/// Enter leaves the text as it was.
fn typed(code: KeyCode, mut text: String) -> Typed {
    match code {
        KeyCode::Char(c) => text.push(c),
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Enter => return Typed::Submitted(text),
        _ => {}
    }
    Typed::Editing(text)
}

/// Moves the board cursor a column left or right and pulls the row back inside
/// the column it lands in.
fn move_col(app: &mut App, delta: isize) {
    let len = app.board.stages.len();
    if len == 0 {
        app.board.col = 0;
    } else if delta > 0 {
        app.board.col = (app.board.col + 1).min(len - 1);
    } else {
        app.board.col = app.board.col.saturating_sub(1);
    }
    clamp(&mut app.board.row, app.board.columns.get(app.board.col).map(Vec::len).unwrap_or(0));
}

/// The board shows the project the Projects tab has selected; with no projects
/// at all it shows the global board.
fn selected_project_id(app: &App) -> Option<Uuid> {
    app.projects.get(app.projects_sel).map(|p| p.id)
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
        Tab::Board => Effect::LoadBoard(selected_project_id(app)),
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
        Tab::Board => app.board.columns.get(app.board.col).map(Vec::len).unwrap_or(0),
        Tab::Review => app.pending.len(),
        Tab::Status => 0,
    };
    let sel = match app.tab {
        Tab::Memories => &mut app.memories_sel,
        Tab::Projects => &mut app.projects_sel,
        Tab::Agents => &mut app.agents_sel,
        Tab::Practices => &mut app.practices_sel,
        Tab::Workflows => &mut app.workflows_sel,
        Tab::Board => &mut app.board.row,
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
            board_key: None,
            board_stages: None,
            agent_access: Default::default(),
            extraction: None,
            mcp_disabled_tools: vec![],
            skills_disabled: vec![],
        }
    }

    fn stage(name: &str, done: bool) -> Stage {
        Stage { name: name.into(), done }
    }

    fn task(key: &str, stage: &str) -> Task {
        Task {
            id: Uuid::new_v4(),
            key: key.into(),
            project_id: None,
            seq: 1,
            position: 0.0,
            title: format!("{key} title"),
            description: String::new(),
            stage: stage.into(),
            kind: TaskKind::Task,
            priority: TaskPriority::Medium,
            assignee: None,
            labels: vec![],
            parent_id: None,
            parent_key: None,
            parent_title: None,
            created_by: "test".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            source_ref: None,
            persona_id: None,
            persona_name: None,
            persona_slug: None,
            blocked_by: vec![],
            open_blockers: 0,
            ready: true,
            blocked_reason: None,
            subtasks_total: 0,
            subtasks_done: 0,
        }
    }

    /// Three stages, two cards in the first column and none in the others.
    fn board_app() -> App {
        let mut a = App { tab: Tab::Board, ..Default::default() };
        reduce(
            &mut a,
            Action::BoardLoaded(
                vec![stage("Backlog", false), stage("Testing", false), stage("Done", true)],
                vec![task("ATL-1", "Backlog"), task("ATL-2", "Backlog")],
            ),
        );
        a
    }

    #[test]
    fn tab_cycles_and_wraps() {
        let mut a = App::default();
        for _ in 0..8 {
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
        for _ in 0..8 {
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
            (Tab::Board, Effect::LoadBoard(None)),
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
    fn board_columns_clamp_at_both_edges() {
        let mut a = board_app();
        assert_eq!(a.board.columns.iter().map(Vec::len).collect::<Vec<_>>(), [2, 0, 0]);
        reduce(&mut a, key('h'));
        assert_eq!(a.board.col, 0, "h at the first stage stays there");
        for _ in 0..5 {
            reduce(&mut a, key('l'));
        }
        assert_eq!(a.board.col, 2, "l stops at the last stage");
        reduce(&mut a, key('h'));
        assert_eq!(a.board.col, 1);
    }

    #[test]
    fn board_rows_clamp_inside_their_column() {
        let mut a = board_app();
        reduce(&mut a, key('k'));
        assert_eq!(a.board.row, 0, "k at the top stays there");
        reduce(&mut a, key('j'));
        assert_eq!(a.board.row, 1);
        reduce(&mut a, key('j'));
        assert_eq!(a.board.row, 1, "j stops at the last card");
        // The second column is empty, so the row has to come back with the cursor.
        reduce(&mut a, key('l'));
        assert_eq!(a.board.row, 0);
        assert!(reduce(&mut a, key('j')).is_empty());
        assert_eq!(a.board.row, 0);
    }

    #[test]
    fn board_move_picker_yields_the_numbered_stage() {
        let mut a = board_app();
        assert!(reduce(&mut a, key('m')).is_empty());
        assert_eq!(a.board.mode, BoardMode::MovePicker);
        let eff = reduce(&mut a, key('2'));
        assert_eq!(eff, vec![Effect::MoveTask { key: "ATL-1".into(), stage: "Testing".into() }]);
        assert_eq!(a.board.mode, BoardMode::Normal);
    }

    #[test]
    fn board_move_picker_ignores_a_number_with_no_stage() {
        let mut a = board_app();
        reduce(&mut a, key('m'));
        assert!(reduce(&mut a, key('9')).is_empty());
        assert!(reduce(&mut a, key('0')).is_empty());
        assert_eq!(a.board.mode, BoardMode::MovePicker, "a miss leaves the picker open");
    }

    #[test]
    fn board_comment_prompt_yields_a_comment() {
        let mut a = board_app();
        reduce(&mut a, key('c'));
        for ch in "looks good".chars() {
            reduce(&mut a, key(ch));
        }
        assert_eq!(a.board.mode, BoardMode::CommentPrompt("looks good".into()));
        reduce(&mut a, code(KeyCode::Backspace));
        let eff = reduce(&mut a, code(KeyCode::Enter));
        assert_eq!(eff, vec![Effect::CommentTask { key: "ATL-1".into(), body: "looks goo".into() }]);
        assert_eq!(a.board.mode, BoardMode::Normal);
    }

    #[test]
    fn board_new_task_prompt_yields_a_task_and_swallows_quit() {
        let mut a = board_app();
        reduce(&mut a, key('n'));
        for ch in "ship the board".chars() {
            reduce(&mut a, key(ch));
        }
        assert!(!a.quit, "q typed into a prompt is a letter, not a quit");
        let eff = reduce(&mut a, code(KeyCode::Enter));
        assert_eq!(eff, vec![Effect::CreateTask { title: "ship the board".into(), project_id: None }]);
        assert_eq!(a.board.mode, BoardMode::Normal);
    }

    #[test]
    fn board_empty_prompts_produce_nothing() {
        for open in ['c', 'n'] {
            let mut a = board_app();
            reduce(&mut a, key(open));
            assert!(reduce(&mut a, code(KeyCode::Enter)).is_empty(), "{open}");
            assert_eq!(a.board.mode, BoardMode::Normal);
        }
    }

    #[test]
    fn board_escape_cancels_every_prompt() {
        for open in ['m', 'c', 'n'] {
            let mut a = board_app();
            reduce(&mut a, key(open));
            assert_ne!(a.board.mode, BoardMode::Normal, "{open} should have opened something");
            assert!(reduce(&mut a, code(KeyCode::Esc)).is_empty());
            assert_eq!(a.board.mode, BoardMode::Normal, "Esc should have cancelled {open}");
        }
    }

    #[test]
    fn board_enter_opens_the_detail_and_escape_closes_it() {
        let mut a = board_app();
        let eff = reduce(&mut a, code(KeyCode::Enter));
        assert_eq!(eff, vec![Effect::LoadTaskDetail("ATL-1".into())]);
        reduce(
            &mut a,
            Action::TaskDetailLoaded(Box::new(TaskDetail {
                task: task("ATL-1", "Backlog"),
                children: vec![],
                events: vec![],
            })),
        );
        assert!(a.board.detail.is_some());
        reduce(&mut a, code(KeyCode::Esc));
        assert!(a.board.detail.is_none());
    }

    #[test]
    fn board_follows_the_project_the_projects_tab_has_selected() {
        let mut a = App { tab: Tab::Board, ..Default::default() };
        let eff = reduce(&mut a, Action::ProjectsLoaded(vec![project("/tmp/one"), project("/tmp/two")]));
        let first = a.projects[0].id;
        assert_eq!(eff, vec![Effect::LoadBoard(Some(first))], "the board loads once the projects have");
        assert_eq!(reduce(&mut a, key('r')), vec![Effect::LoadBoard(Some(first))]);

        // A board write reloads the same project's board.
        assert_eq!(reduce(&mut a, Action::BoardChanged), vec![Effect::LoadBoard(Some(first))]);
    }

    #[test]
    fn board_load_pulls_a_stale_cursor_back_inside() {
        let mut a = board_app();
        reduce(&mut a, key('l'));
        reduce(&mut a, key('l'));
        reduce(&mut a, key('j'));
        reduce(
            &mut a,
            Action::BoardLoaded(vec![stage("Only", false), stage("Done", true)], vec![]),
        );
        assert_eq!((a.board.col, a.board.row), (1, 0));
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
