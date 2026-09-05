//! Rendering. Pure: state in, cells out, nothing read from the terminal.
//!
//! Every screen is the same four rows: the tab bar, the current tab's body, a
//! message line, and the key legend. Bodies that have a selection are a list on
//! the left and the selected item's detail on the right.

use super::state::{App, BoardMode, Focus, Tab};
use atlas_core::models::{Agent, Doc, Memory, Project, RecallHit, Task, TaskDetail};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;

/// Every list/detail body uses the same split.
const SPLIT: [Constraint; 2] = [Constraint::Percentage(40), Constraint::Percentage(60)];

pub fn draw(f: &mut Frame, app: &App) {
    let [tab_bar, body, message, legend] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(
        Tabs::new(Tab::ALL.map(Tab::title))
            .select(app.tab.index())
            .highlight_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        tab_bar,
    );
    match app.tab {
        Tab::Memories => memories(f, app, body),
        Tab::Projects => projects(f, app, body),
        Tab::Agents => agents(f, app, body),
        Tab::Practices => docs(f, "Practices", &app.practices, app.practices_sel, body),
        Tab::Workflows => docs(f, "Workflows", &app.workflows, app.workflows_sel, body),
        Tab::Board => board(f, app, body),
        Tab::Review => review(f, app, body),
        Tab::Status => status(f, app, body),
    }
    f.render_widget(message_bar(app), message);
    f.render_widget(Paragraph::new(help(app.tab)).style(dim()), legend);
}

/// The confirmation is a question the user has to answer, so it outranks
/// anything else that wanted this row.
fn message_bar(app: &App) -> Paragraph<'static> {
    let (text, style) = if app.confirm_forget {
        ("forget the selected memory? y confirms, any other key cancels".into(), Style::new().fg(Color::Yellow))
    } else if let Some(message) = &app.message {
        (message.clone(), Style::new().fg(Color::Red))
    } else if app.is_loading() {
        ("loading…".to_string(), dim())
    } else {
        (String::new(), Style::new())
    };
    Paragraph::new(text).style(style)
}

fn help(tab: Tab) -> &'static str {
    match tab {
        Tab::Memories => "Tab switch  j/k move  / search  f forget  r refresh  q/Ctrl+C quit",
        Tab::Projects => "Tab switch  j/k move  Enter open  c connect  r refresh  q/Ctrl+C quit",
        Tab::Agents => "Tab switch  j/k move  s sync  r refresh  q/Ctrl+C quit",
        Tab::Practices | Tab::Workflows => "Tab switch  j/k move  r refresh  q/Ctrl+C quit",
        Tab::Board => "Tab switch  h/l column  j/k move  Enter detail  m move  c comment  n new  r refresh  q quit",
        Tab::Review => "Tab switch  j/k move  a accept  x reject  r refresh  q/Ctrl+C quit",
        Tab::Status => "Tab switch  r refresh  q/Ctrl+C quit",
    }
}

fn memories(f: &mut Frame, app: &App, area: Rect) {
    let area = if app.focus == Focus::Search { search_box(f, app, area) } else { area };
    let [left, right] = Layout::horizontal(SPLIT).areas(area);
    let rows: Vec<String> = app.memories.iter().map(memory_row).collect();
    // The query stays in the title once focus leaves Search, so the list
    // still shows what it is a result of.
    let suffix = if app.query.is_empty() { String::new() } else { format!("/{}", app.query) };
    list_pane_with_suffix(f, left, "Memories", &rows, Some(app.memories_sel), &suffix);
    let lines = match app.memories.get(app.memories_sel) {
        Some(hit) => memory_detail(hit),
        None => vec![Line::from("no memories")],
    };
    detail(f, right, "Memory", lines);
}

/// Draws the query above the body and returns what is left for the body.
fn search_box(f: &mut Frame, app: &App, area: Rect) -> Rect {
    let [top, rest] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    f.render_widget(
        Paragraph::new(format!("/{}", app.query)).block(Block::bordered().title("Search")),
        top,
    );
    if top.height >= 3 && top.width >= 3 {
        let offset = u16::try_from(app.query.chars().count() + 2).unwrap_or(u16::MAX);
        let x = top.x.saturating_add(offset).min(top.right().saturating_sub(1));
        f.set_cursor_position((x, top.y + 1));
    }
    rest
}

fn memory_row(hit: &RecallHit) -> String {
    let tags = if hit.memory.tags.is_empty() {
        String::new()
    } else {
        format!("  #{}", hit.memory.tags.join(" #"))
    };
    format!("{:.2}  [{}] {}{}", hit.score, hit.memory.kind, one_line(&hit.memory.text), tags)
}

fn memory_detail(hit: &RecallHit) -> Vec<Line<'static>> {
    let m = &hit.memory;
    let mut lines = vec![Line::styled(m.text.clone(), bold()), Line::from("")];
    if !m.tags.is_empty() {
        lines.push(Line::from(format!("tags       #{}", m.tags.join(" #"))));
    }
    lines.push(Line::from(format!("kind       {}   score {:.2}   confidence {:.2}", m.kind, hit.score, m.confidence)));
    lines.push(Line::from(format!("source     {}", source_of(m))));
    lines.push(Line::from(format!("id         {}", m.id)));
    lines.push(Line::from(format!("created    {}", m.created_at.format("%Y-%m-%d %H:%M"))));
    lines
}

fn source_of(m: &Memory) -> String {
    match (&m.source_agent, &m.source_tool) {
        (Some(agent), Some(tool)) => format!("{agent} / {tool}"),
        (Some(agent), None) => agent.clone(),
        (None, Some(tool)) => tool.clone(),
        (None, None) => "-".to_string(),
    }
}

fn projects(f: &mut Frame, app: &App, area: Rect) {
    let [left, right] = Layout::horizontal(SPLIT).areas(area);
    let rows: Vec<String> =
        app.projects.iter().map(|p| format!("{}  {}", p.name, p.root_path)).collect();
    list_pane(f, left, "Projects", &rows, app.projects_sel);
    let lines = match app.projects.get(app.projects_sel) {
        Some(project) => project_detail(app, project),
        None => vec![Line::from("no projects; press c to connect the current directory")],
    };
    detail(f, right, "Project", lines);
}

fn project_detail(app: &App, p: &Project) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(p.name.clone(), bold()), Line::from(p.root_path.clone())];
    if let Some(remote) = &p.git_remote {
        lines.push(Line::from(format!("remote      {remote}")));
    }
    if let Some(profile) = &p.profile {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("languages   {}", list_or_dash(&profile.languages))));
        lines.push(Line::from(format!("frameworks  {}", list_or_dash(&profile.frameworks))));
        if !profile.recent_commits.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::styled("recent commits", bold()));
            lines.extend(profile.recent_commits.iter().take(10).map(indented));
        }
    }
    // The context is fetched one project at a time, so only show it against the
    // project it was fetched for.
    match &app.project_context {
        Some(ctx) if ctx.project.id == p.id && !ctx.memories.is_empty() => {
            lines.push(Line::from(""));
            lines.push(Line::styled("top memories", bold()));
            lines.extend(ctx.memories.iter().take(10).map(|h| indented(&h.memory.text)));
        }
        _ => {}
    }
    lines
}

fn agents(f: &mut Frame, app: &App, area: Rect) {
    let [left, right] = Layout::horizontal(SPLIT).areas(area);
    let rows: Vec<String> = app.agents.iter().map(|a| a.name.clone()).collect();
    list_pane(f, left, "Agents", &rows, app.agents_sel);
    let mut lines = match app.agents.get(app.agents_sel) {
        Some(agent) => agent_detail(agent),
        None => vec![Line::from("no agents; press s to sync")],
    };
    if let Some(sync) = &app.last_sync {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "last sync   {} created, {} updated, {} unchanged, {} skipped",
            sync.created, sync.updated, sync.unchanged, sync.skipped
        )));
    }
    detail(f, right, "Agent", lines);
}

fn agent_detail(a: &Agent) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(a.name.clone(), bold())];
    if let Some(model) = &a.model_hint {
        lines.push(Line::from(format!("model       {model}")));
    }
    if !a.tools.is_empty() {
        lines.push(Line::from(format!("tools       {}", a.tools.join(", "))));
    }
    if !a.tags.is_empty() {
        lines.push(Line::from(format!("tags        #{}", a.tags.join(" #"))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(a.description.clone()));
    lines.push(Line::from(""));
    lines.push(Line::styled("instructions", bold()));
    lines.extend(body_lines(&a.instructions));
    lines
}

fn docs(f: &mut Frame, title: &str, docs: &[Doc], sel: usize, area: Rect) {
    let [left, right] = Layout::horizontal(SPLIT).areas(area);
    let rows: Vec<String> = docs.iter().map(|d| d.name.clone()).collect();
    list_pane(f, left, title, &rows, sel);
    let lines = match docs.get(sel) {
        Some(doc) => doc_detail(doc),
        None => vec![Line::from("nothing here yet")],
    };
    detail(f, right, "Body", lines);
}

fn doc_detail(d: &Doc) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(d.name.clone(), bold())];
    if !d.tags.is_empty() {
        lines.push(Line::from(format!("tags        #{}", d.tags.join(" #"))));
    }
    lines.push(Line::from(""));
    lines.extend(body_lines(&d.body));
    lines
}

/// One column per stage, plus whatever the tab is in the middle of: a picker or
/// a prompt above the columns, an open task's detail to their right.
fn board(f: &mut Frame, app: &App, area: Rect) {
    let area = prompt_box(f, app, area);
    let area = match &app.board.detail {
        Some(open) => {
            let [left, right] = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);
            detail(f, right, "Task", task_detail(open));
            left
        }
        None => area,
    };
    let count = app.board.stages.len();
    if count == 0 {
        detail(f, area, "Board", vec![Line::from("no board loaded; press r to refresh")]);
        return;
    }
    let ratio = u32::try_from(count).unwrap_or(1);
    let columns = Layout::horizontal(vec![Constraint::Ratio(1, ratio); count]).split(area);
    for (i, stage) in app.board.stages.iter().enumerate() {
        let rows: Vec<String> = app.board.columns.get(i).map(|c| c.iter().map(card).collect()).unwrap_or_default();
        // Only the focused column carries the cursor, so the board shows one.
        let sel = (i == app.board.col).then_some(app.board.row);
        list_pane_with_suffix(f, columns[i], &stage.name, &rows, sel, "");
    }
}

/// Draws the open picker or prompt above the board and returns what is left for
/// the columns. With nothing open the whole area is left alone.
fn prompt_box(f: &mut Frame, app: &App, area: Rect) -> Rect {
    let (title, text) = match &app.board.mode {
        BoardMode::Normal => return area,
        BoardMode::MovePicker => (
            "Move to (a digit picks, Esc cancels)",
            app.board
                .stages
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{} {}", i + 1, s.name))
                .collect::<Vec<_>>()
                .join("   "),
        ),
        BoardMode::CommentPrompt(text) => ("Comment (Enter sends, Esc cancels)", text.clone()),
        BoardMode::NewTaskPrompt(text) => ("New task (Enter creates, Esc cancels)", text.clone()),
    };
    let [top, rest] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    f.render_widget(Paragraph::new(text).block(Block::bordered().title(title)), top);
    rest
}

fn card(t: &Task) -> String {
    match &t.assignee {
        Some(who) => format!("{}  {} [{who}]", t.key, one_line(&t.title)),
        None => format!("{}  {}", t.key, one_line(&t.title)),
    }
}

fn task_detail(d: &TaskDetail) -> Vec<Line<'static>> {
    let t = &d.task;
    let mut lines = vec![
        Line::styled(format!("{}  {}", t.key, t.title), bold()),
        Line::from(""),
        Line::from(format!("stage       {}", t.stage)),
        Line::from(format!("kind        {}   priority {}", t.kind, t.priority)),
        Line::from(format!("assignee    {}", t.assignee.clone().unwrap_or_else(|| "-".into()))),
        Line::from(format!(
            "ready       {}",
            if t.ready { "yes".to_string() } else { format!("no  ({})", t.blocked_reason.clone().unwrap_or_else(|| "blocked".into())) }
        )),
    ];
    if !t.description.trim().is_empty() {
        lines.push(Line::from(""));
        lines.extend(body_lines(&t.description));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!("blockers    {}", list_or_dash(&t.blocked_by))));
    if !d.children.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("children", bold()));
        lines.extend(d.children.iter().map(|c| indented(format!("{}  {}  {}", c.key, c.stage, one_line(&c.title)))));
    }
    if !d.events.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("events", bold()));
        // The newest ten: an old task's log is longer than any pane.
        lines.extend(d.events.iter().rev().take(10).map(|e| {
            indented(format!("{}  {}  {}  {}", e.created_at.format("%m-%d %H:%M"), e.actor, e.kind, one_line(&e.body)))
        }));
    }
    lines
}

fn review(f: &mut Frame, app: &App, area: Rect) {
    let [left, right] = Layout::horizontal(SPLIT).areas(area);
    let rows: Vec<String> = app
        .pending
        .iter()
        .map(|m| format!("{:.2}  {}", m.confidence, one_line(&m.text)))
        .collect();
    list_pane(f, left, "Pending", &rows, app.pending_sel);
    let lines = match app.pending.get(app.pending_sel) {
        Some(m) => vec![
            Line::styled(m.text.clone(), bold()),
            Line::from(""),
            Line::from(format!("kind        {}   confidence {:.2}", m.kind, m.confidence)),
            Line::from(format!("source      {}", source_of(m))),
            Line::from(format!("created     {}", m.created_at.format("%Y-%m-%d %H:%M"))),
        ],
        None => vec![Line::from("nothing waiting for review")],
    };
    detail(f, right, "Pending memory", lines);
}

fn status(f: &mut Frame, app: &App, area: Rect) {
    let lines = match &app.status {
        Some(s) => vec![
            Line::from(format!("version           {}", s.version)),
            Line::from(format!(
                "port              {}",
                s.port.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string())
            )),
            Line::from(format!("database          {}", s.db_path)),
            Line::from(format!("active memories   {}", s.memories_active)),
            Line::from(format!("pending memories  {}", s.memories_pending)),
            Line::from(format!("embedding         {}", s.embedding)),
        ],
        None => vec![Line::from("status not loaded yet")],
    };
    detail(f, area, "Daemon", lines);
}

fn list_pane(f: &mut Frame, area: Rect, title: &str, rows: &[String], sel: usize) {
    list_pane_with_suffix(f, area, title, rows, Some(sel), "");
}

/// Like [`list_pane`], but with `suffix` appended to the title after the
/// count, e.g. `Memories (3) /duckdb`. `sel` is `None` for a list that is on
/// screen without the cursor in it, which only the board has.
fn list_pane_with_suffix(f: &mut Frame, area: Rect, title: &str, rows: &[String], sel: Option<usize>, suffix: &str) {
    let width = usize::from(area.width.saturating_sub(2));
    let items: Vec<ListItem> = rows.iter().map(|r| ListItem::new(truncate(r, width))).collect();
    let title = if suffix.is_empty() {
        format!("{title} ({})", rows.len())
    } else {
        format!("{title} ({}) {suffix}", rows.len())
    };
    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(sel.filter(|_| !rows.is_empty()));
    f.render_stateful_widget(list, area, &mut state);
}

fn detail(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>) {
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title.to_string()))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// One row of a list is one line, however many the stored text has.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cuts to `width` *characters*; the paragraphs wrap instead, only lists cut.
/// Control characters would move the cursor mid-row, so they become spaces.
fn truncate(s: &str, width: usize) -> String {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if s.chars().count() <= width {
        return s;
    }
    match width {
        0 | 1 => s.chars().take(width).collect(),
        _ => s.chars().take(width - 1).chain(std::iter::once('…')).collect(),
    }
}

/// Keeps a stored body's own line breaks instead of running it all together.
fn body_lines(s: &str) -> Vec<Line<'static>> {
    s.lines().map(|l| Line::from(l.to_string())).collect()
}

fn indented(s: impl AsRef<str>) -> Line<'static> {
    Line::from(format!("  {}", s.as_ref()))
}

fn list_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

#[cfg(test)]
mod tests {
    use super::draw;
    use crate::tui::state::{App, Tab};
    use atlas_core::models::*;
    use chrono::Utc;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use uuid::Uuid;

    /// The drawn buffer as one string per row, joined by newlines.
    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn mem(text: &str) -> Memory {
        Memory {
            id: Uuid::new_v4(),
            scope: MemoryScope::Global,
            project_id: None,
            kind: MemoryKind::Fact,
            text: text.into(),
            tags: vec!["rust".into()],
            source_agent: Some("claude".into()),
            source_tool: Some("remember".into()),
            confidence: 0.8,
            status: MemoryStatus::Active,
            superseded_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn hit(text: &str) -> RecallHit {
        RecallHit { memory: mem(text), score: 0.5 }
    }

    fn project(name: &str) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: name.into(),
            root_path: format!("/tmp/{name}"),
            git_remote: None,
            profile: Some(ProjectProfile {
                name: name.into(),
                languages: vec!["rust".into()],
                frameworks: vec!["tokio".into()],
                recent_commits: vec!["abc init".into()],
                ..Default::default()
            }),
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

    fn agent(name: &str) -> Agent {
        Agent {
            id: Uuid::new_v4(),
            name: name.into(),
            description: "reviews code".into(),
            instructions: "be terse".into(),
            model_hint: None,
            tools: vec![],
            tags: vec![],
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn doc(kind: DocKind, name: &str) -> Doc {
        Doc {
            id: Uuid::new_v4(),
            kind,
            name: name.into(),
            body: "the body".into(),
            tags: vec![],
            project_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn memories_tab_renders_tabs_rows_and_help() {
        let app = App {
            memories: vec![hit("rust is fast"), hit("bun is quick")],
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("Memories"), "{out}");
        assert!(out.contains("Projects"), "{out}");
        assert!(out.contains("rust is fast"), "{out}");
        assert!(out.contains("q/Ctrl+C quit"), "{out}");
    }

    #[test]
    fn memories_title_shows_the_active_query_once_focus_leaves_search() {
        let app = App {
            query: "duckdb".into(),
            memories: vec![hit("rust is fast"), hit("bun is quick"), hit("uses duckdb")],
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("Memories (3) /duckdb"), "{out}");
    }

    #[test]
    fn narrow_terminal_does_not_panic() {
        let app = App { memories: vec![hit("rust is fast")], ..Default::default() };
        let out = render(&app, 40, 12);
        assert!(out.contains("Memories"), "{out}");
        // Absurdly small is still not allowed to panic.
        render(&app, 10, 3);
    }

    #[test]
    fn projects_tab_renders_the_selected_project() {
        let p = project("atlas");
        let app = App {
            tab: Tab::Projects,
            projects: vec![p.clone()],
            project_context: Some(ProjectContext {
                project: p,
                memories: vec![hit("uses duckdb")],
                practices: vec![],
                workflows: vec![],
                skills: vec![],
                personas: None,
            }),
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("atlas"), "{out}");
        assert!(out.contains("uses duckdb"), "{out}");
        assert!(out.contains("c connect"), "{out}");
    }

    #[test]
    fn agents_tab_renders_the_selected_agent() {
        let app = App {
            tab: Tab::Agents,
            agents: vec![agent("reviewer")],
            last_sync: Some(SyncReport { created: 1, ..Default::default() }),
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("reviewer"), "{out}");
        assert!(out.contains("reviews code"), "{out}");
        assert!(out.contains("s sync"), "{out}");
    }

    #[test]
    fn practices_tab_renders_the_doc() {
        let app = App {
            tab: Tab::Practices,
            practices: vec![doc(DocKind::Practice, "tdd")],
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("tdd"), "{out}");
        assert!(out.contains("the body"), "{out}");
    }

    #[test]
    fn workflows_tab_renders_the_doc() {
        let app = App {
            tab: Tab::Workflows,
            workflows: vec![doc(DocKind::Workflow, "release")],
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("release"), "{out}");
    }

    fn board_state(mode: crate::tui::state::BoardMode) -> App {
        let stages: Vec<Stage> = [("Backlog", false), ("In Progress", false), ("Testing", false), ("Done", true)]
            .into_iter()
            .map(|(name, done)| Stage { name: name.into(), done })
            .collect();
        let mut card = task("ATL-1", "Backlog");
        card.assignee = Some("codex".into());
        let columns = vec![vec![card], vec![], vec![task("ATL-2", "Testing")], vec![]];
        App {
            tab: Tab::Board,
            board: crate::tui::state::Board { stages, columns, col: 0, row: 0, detail: None, mode },
            ..Default::default()
        }
    }

    fn task(key: &str, stage: &str) -> Task {
        Task {
            id: Uuid::new_v4(),
            key: key.into(),
            project_id: None,
            seq: 1,
            title: format!("{key} needs doing"),
            description: "the long form".into(),
            stage: stage.into(),
            kind: TaskKind::Task,
            priority: TaskPriority::High,
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
            blocked_by: vec!["ATL-9".into()],
            open_blockers: 1,
            ready: false,
            blocked_reason: Some("waiting on ATL-9".into()),
            subtasks_total: 0,
            subtasks_done: 0,
        }
    }

    #[test]
    fn board_tab_renders_a_column_per_stage() {
        use crate::tui::state::BoardMode;
        // Wide enough that a card is not cut off: four columns share the width.
        let out = render(&board_state(BoardMode::Normal), 160, 24);
        for stage in ["Backlog", "In Progress", "Testing", "Done"] {
            assert!(out.contains(stage), "the board should show the {stage} column: {out}");
        }
        assert!(out.contains("ATL-1"), "cards carry their key: {out}");
        assert!(out.contains("codex"), "cards carry the assignee: {out}");
        assert!(out.contains("m move"), "{out}");
    }

    #[test]
    fn board_tab_renders_the_picker_the_prompts_and_the_detail() {
        use crate::tui::state::BoardMode;
        let out = render(&board_state(BoardMode::MovePicker), 120, 24);
        assert!(out.contains("1 Backlog"), "the picker numbers the stages: {out}");

        let out = render(&board_state(BoardMode::CommentPrompt("looks good".into())), 120, 24);
        assert!(out.contains("looks good"), "{out}");

        let out = render(&board_state(BoardMode::NewTaskPrompt("ship it".into())), 120, 24);
        assert!(out.contains("ship it"), "{out}");

        let mut app = board_state(BoardMode::Normal);
        app.board.detail = Some(TaskDetail {
            task: task("ATL-1", "Backlog"),
            children: vec![task("ATL-3", "Backlog")],
            events: vec![TaskEvent {
                id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                actor: "codex".into(),
                kind: "commented".into(),
                body: "verified by hand".into(),
                detail: None,
                created_at: Utc::now(),
            }],
        });
        let out = render(&app, 120, 24);
        assert!(out.contains("the long form"), "the detail shows the description: {out}");
        assert!(out.contains("ATL-3"), "the detail lists the children: {out}");
        assert!(out.contains("verified"), "the detail lists the events: {out}");
    }

    #[test]
    fn board_tab_survives_a_narrow_terminal_and_an_empty_board() {
        use crate::tui::state::BoardMode;
        render(&board_state(BoardMode::Normal), 40, 12);
        render(&board_state(BoardMode::MovePicker), 20, 6);
        let out = render(&App { tab: Tab::Board, ..Default::default() }, 80, 24);
        assert!(out.contains("press r to refresh"), "{out}");
    }

    #[test]
    fn review_tab_renders_pending_memories() {
        let app = App {
            tab: Tab::Review,
            pending: vec![mem("maybe true")],
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("maybe true"), "{out}");
        assert!(out.contains("a accept"), "{out}");
    }

    #[test]
    fn status_tab_renders_the_report() {
        let app = App {
            tab: Tab::Status,
            status: Some(StatusReport {
                version: "9.9.9".into(),
                db_path: "/tmp/atlas.duckdb".into(),
                memories_active: 42,
                memories_pending: 3,
                embedding: "off".into(),
                port: Some(7433),
                token_sha256: None,
            }),
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("9.9.9"), "{out}");
        assert!(out.contains("42"), "{out}");
    }

    #[test]
    fn search_focus_shows_the_query_and_message_bar_shows_errors() {
        let app = App {
            focus: crate::tui::state::Focus::Search,
            query: "duckdb".into(),
            message: Some("daemon unreachable".into()),
            ..Default::default()
        };
        let out = render(&app, 80, 24);
        assert!(out.contains("duckdb"), "{out}");
        assert!(out.contains("daemon unreachable"), "{out}");
    }
}
