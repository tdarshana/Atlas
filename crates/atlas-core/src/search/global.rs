//! Cross-entity search for the daemon: one query fans out over tasks, memories,
//! projects, project files and commits, task/audit events, and workflows.
//!
//! Every group is scored independently with the same rule (an exact key/name match
//! beats a title substring beats a body substring; ties break by recency) and capped
//! at the caller's limit. Tasks, memories, task/audit events and workflows are first
//! prefiltered in SQL with a bound, escaped `LIKE '%...%'` over their text columns,
//! newest first, capped at 500 rows per source (see each repo's `search_candidates` /
//! `events_for_search` / `list_audit_for_search`) before this module re-scores and
//! re-ranks them in memory; `total` counts every one of *those* (already-capped)
//! candidates that scored, not every true match in the table, so a source with more
//! than 500 matches reports the 500 newest rather than scanning it whole on every
//! keystroke. Files and commits are the exception: they come straight from the
//! (scoped) projects' already-loaded profiles, since there's no table row to prefilter.
//! `highlights` are **char** offsets (not byte offsets) into `title`, so a multi-byte
//! character in a title still lines up correctly for a caller that indexes by
//! character rather than by byte.

use crate::board::TaskRepo;
use crate::memories::MemoryRepo;
use crate::models::Project;
use crate::projects::ProjectRepo;
use crate::workflow::WorkflowRepo;
use crate::{AtlasError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// `limit` when a caller names none.
pub const DEFAULT_LIMIT: usize = 20;
/// The most items any one group can hold, regardless of what the caller asks for.
pub const MAX_LIMIT: usize = 50;

/// The seven kinds of thing a search result can hold, in the fixed order they render.
pub const ORDER: [SearchKind; 7] = [
    SearchKind::Task,
    SearchKind::Memory,
    SearchKind::Project,
    SearchKind::File,
    SearchKind::Commit,
    SearchKind::Event,
    SearchKind::Workflow,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    Task,
    Memory,
    Project,
    File,
    Commit,
    Event,
    Workflow,
}

impl SearchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Memory => "memory",
            Self::Project => "project",
            Self::File => "file",
            Self::Commit => "commit",
            Self::Event => "event",
            Self::Workflow => "workflow",
        }
    }
}

impl std::fmt::Display for SearchKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SearchKind {
    type Err = AtlasError;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "task" => Ok(Self::Task),
            "memory" => Ok(Self::Memory),
            "project" => Ok(Self::Project),
            "file" => Ok(Self::File),
            "commit" => Ok(Self::Commit),
            "event" => Ok(Self::Event),
            "workflow" => Ok(Self::Workflow),
            other => Err(AtlasError::Invalid(format!("unknown search kind: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchHit {
    pub kind: SearchKind,
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub project_id: Option<Uuid>,
    pub reference: Option<String>,
    pub score: f64,
    /// Char offsets `(start, end)` into `title` of the first case-insensitive match of
    /// the query, or empty when the hit scored on a field other than `title`.
    pub highlights: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchGroup {
    pub kind: SearchKind,
    pub items: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub kinds: Option<Vec<SearchKind>>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchResult {
    pub groups: Vec<SearchGroup>,
    pub total: usize,
    pub took_ms: u64,
}

/// One scored candidate before it is capped into a group. Carried alongside its
/// finished `SearchHit` so sorting and capping never has to re-derive the score or
/// the recency tie-break from the hit's own (already-rounded) fields.
struct Candidate {
    hit: SearchHit,
    score: f64,
    recency: DateTime<Utc>,
}

/// The highest weight that applies: an exact (trimmed, case-insensitive) match on
/// `exact` beats a substring match on `title`, which beats one on `body`. `q` is
/// already trimmed and lowercased by the caller. Fields that don't apply to a given
/// kind are passed as `None` and simply can't contribute their weight.
fn score_of(q: &str, exact: Option<&str>, title: Option<&str>, body: Option<&str>) -> f64 {
    if let Some(e) = exact {
        if e.trim().to_lowercase() == q {
            return 5.0;
        }
    }
    if let Some(t) = title {
        if t.to_lowercase().contains(q) {
            return 3.0;
        }
    }
    if let Some(b) = body {
        if b.to_lowercase().contains(q) {
            return 1.0;
        }
    }
    0.0
}

/// The char range of the first case-insensitive match of `q` in `title`, or empty
/// when `title` doesn't contain it (the hit scored on a different field) or when
/// lower-casing `title` changed its character count (a rare case-folding expansion:
/// `'İ'` (U+0130, LATIN CAPITAL LETTER I WITH DOT ABOVE) lowers to two chars, `i`
/// plus a combining dot above, where a byte/char-safe match can't be recovered).
fn highlight(q: &str, title: &str) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return Vec::new();
    }
    let title_chars: Vec<char> = title.chars().collect();
    let title_lower: Vec<char> = title.to_lowercase().chars().collect();
    let q_chars: Vec<char> = q.chars().collect();
    if title_lower.len() != title_chars.len() || q_chars.len() > title_lower.len() {
        return Vec::new();
    }
    for start in 0..=(title_lower.len() - q_chars.len()) {
        if title_lower[start..start + q_chars.len()] == q_chars[..] {
            return vec![(start, start + q_chars.len())];
        }
    }
    Vec::new()
}

fn project_scoped(project_id: Option<Uuid>, candidate: Uuid) -> bool {
    project_id.map(|p| p == candidate).unwrap_or(true)
}

/// Turns `q` (already trimmed and lowercased) into a `%...%` pattern safe to bind
/// into a `LIKE ... ESCAPE '\'` clause: `%`, `_` and the escape character itself are
/// each escaped so they act as literal characters in the query rather than wildcards.
fn like_pattern(q: &str) -> String {
    let mut escaped = String::with_capacity(q.len());
    for c in q.chars() {
        if c == '\\' || c == '%' || c == '_' {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    format!("%{escaped}%")
}

/// Shifts each highlight range by `by` chars, for a field matched inside a larger
/// concatenated title (an event's `"<actor> <mid> <tail>"`) rather than at the
/// title's own start.
fn shift(highlights: Vec<(usize, usize)>, by: usize) -> Vec<(usize, usize)> {
    highlights.into_iter().map(|(s, e)| (s + by, e + by)).collect()
}

/// Highlights for an event's `"<actor> <mid> <tail>"` title (`mid` is `kind`/`action`,
/// `tail` is `body`/`detail`), computed against whichever of the two actually produced
/// `score` and then shifted to that field's offset inside the full title — never
/// against the concatenation itself, so a coincidental match inside `actor` can't be
/// reported as the reason this hit scored. `score_of` never passes `exact` for an
/// event, so `score` is always 3.0 (matched `mid`), 1.0 (matched `tail`), or this is
/// never called (0.0, already filtered out by the caller).
fn event_highlights(q: &str, actor: &str, mid: &str, tail: &str, score: f64) -> Vec<(usize, usize)> {
    if score >= 3.0 {
        shift(highlight(q, mid), actor.chars().count() + 1)
    } else {
        shift(highlight(q, tail), actor.chars().count() + 1 + mid.chars().count() + 1)
    }
}

fn search_tasks(q: &str, pattern: &str, project_id: Option<Uuid>, tasks: &TaskRepo, name_of: &HashMap<Uuid, String>) -> Result<Vec<Candidate>> {
    let list = tasks.search_candidates(project_id, pattern)?;
    let mut out = Vec::with_capacity(list.len());
    for t in list {
        let score = score_of(q, Some(&t.key), Some(&t.title), Some(&t.description));
        if score <= 0.0 {
            continue;
        }
        let highlights = highlight(q, &t.title);
        out.push(Candidate {
            hit: SearchHit {
                kind: SearchKind::Task,
                id: t.id.to_string(),
                title: t.title.clone(),
                subtitle: t.project_id.and_then(|p| name_of.get(&p).cloned()),
                project_id: t.project_id,
                reference: Some(t.key.clone()),
                score,
                highlights,
            },
            score,
            recency: t.updated_at,
        });
    }
    Ok(out)
}

fn search_memories(q: &str, pattern: &str, project_id: Option<Uuid>, memories: &MemoryRepo, name_of: &HashMap<Uuid, String>) -> Result<Vec<Candidate>> {
    // `search_candidates` widens rather than narrows: with a project given it matches
    // that project's memories plus every global one, which is exactly the scoping
    // rule this group needs.
    let list = memories.search_candidates(project_id, pattern)?;
    let mut out = Vec::with_capacity(list.len());
    for m in list {
        let score = score_of(q, None, None, Some(&m.text));
        if score <= 0.0 {
            continue;
        }
        let highlights = highlight(q, &m.text);
        out.push(Candidate {
            hit: SearchHit {
                kind: SearchKind::Memory,
                id: m.id.to_string(),
                title: m.text.clone(),
                subtitle: m.project_id.and_then(|p| name_of.get(&p).cloned()),
                project_id: m.project_id,
                reference: None,
                score,
                highlights,
            },
            score,
            recency: m.updated_at,
        });
    }
    Ok(out)
}

fn search_projects(q: &str, pattern: &str, project_id: Option<Uuid>, projects: &ProjectRepo) -> Result<Vec<Candidate>> {
    let list = projects.search_candidates(project_id, pattern)?;
    let mut out = Vec::with_capacity(list.len());
    for p in list {
        let body = p.profile.as_ref().and_then(|pr| pr.summary.clone().or_else(|| Some(pr.readme_head.clone())));
        let score = score_of(q, Some(&p.name), Some(&p.name), body.as_deref());
        if score <= 0.0 {
            continue;
        }
        let highlights = highlight(q, &p.name);
        out.push(Candidate {
            hit: SearchHit {
                kind: SearchKind::Project,
                id: p.id.to_string(),
                title: p.name.clone(),
                subtitle: Some(p.root_path.clone()),
                project_id: Some(p.id),
                reference: None,
                score,
                highlights,
            },
            score,
            recency: p.last_seen_at,
        });
    }
    Ok(out)
}

fn search_files(q: &str, project_id: Option<Uuid>, all: &[Project]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for p in all.iter().filter(|p| project_scoped(project_id, p.id)) {
        let Some(profile) = &p.profile else { continue };
        for path in &profile.tree {
            let score = score_of(q, Some(path), Some(path), None);
            if score <= 0.0 {
                continue;
            }
            let highlights = highlight(q, path);
            out.push(Candidate {
                hit: SearchHit {
                    kind: SearchKind::File,
                    id: format!("{}:{path}", p.id),
                    title: path.clone(),
                    subtitle: Some(p.name.clone()),
                    project_id: Some(p.id),
                    reference: None,
                    score,
                    highlights,
                },
                score,
                recency: p.last_seen_at,
            });
        }
    }
    out
}

fn search_commits(q: &str, project_id: Option<Uuid>, all: &[Project]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for p in all.iter().filter(|p| project_scoped(project_id, p.id)) {
        let Some(profile) = &p.profile else { continue };
        for (i, line) in profile.recent_commits.iter().enumerate() {
            let score = score_of(q, Some(line), Some(line), None);
            if score <= 0.0 {
                continue;
            }
            let highlights = highlight(q, line);
            out.push(Candidate {
                hit: SearchHit {
                    kind: SearchKind::Commit,
                    id: format!("{}:{i}", p.id),
                    title: line.clone(),
                    subtitle: Some(p.name.clone()),
                    project_id: Some(p.id),
                    reference: None,
                    score,
                    highlights,
                },
                score,
                recency: p.last_seen_at,
            });
        }
    }
    out
}

fn search_events(q: &str, pattern: &str, project_id: Option<Uuid>, tasks: &TaskRepo, memories: &MemoryRepo, name_of: &HashMap<Uuid, String>) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for (key, task_project, ev) in tasks.events_for_search(project_id, pattern)? {
        let score = score_of(q, None, Some(&ev.kind), Some(&ev.body));
        if score <= 0.0 {
            continue;
        }
        let title = format!("{} {} {}", ev.actor, ev.kind, ev.body);
        let highlights = event_highlights(q, &ev.actor, &ev.kind, &ev.body, score);
        out.push(Candidate {
            hit: SearchHit {
                kind: SearchKind::Event,
                id: ev.id.to_string(),
                title,
                subtitle: task_project.and_then(|p| name_of.get(&p).cloned()),
                project_id: task_project,
                reference: Some(key),
                score,
                highlights,
            },
            score,
            recency: ev.created_at,
        });
    }
    // `audit` carries no `project_id` column, so an audit-backed event has no owning
    // project to scope against; a project-scoped search leaves this half of the group
    // out entirely rather than showing every project's audit trail under one project's
    // results.
    if project_id.is_none() {
        for a in memories.list_audit_for_search(pattern)? {
            let detail_text = a.detail.as_ref().map(|d| d.to_string()).unwrap_or_default();
            let score = score_of(q, None, Some(&a.action), Some(&detail_text));
            if score <= 0.0 {
                continue;
            }
            let title = format!("{} {} {}", a.actor, a.action, detail_text);
            let highlights = event_highlights(q, &a.actor, &a.action, &detail_text, score);
            out.push(Candidate {
                hit: SearchHit {
                    kind: SearchKind::Event,
                    id: a.id.to_string(),
                    title,
                    subtitle: None,
                    project_id: None,
                    reference: a.entity_id.map(|u| u.to_string()),
                    score,
                    highlights,
                },
                score,
                recency: a.at,
            });
        }
    }
    Ok(out)
}

/// Unlike the other prefiltered sources, workflows have no SQL `LIKE` prefilter of
/// their own: `WorkflowRepo` carries no `search_candidates`, so this lists every
/// workflow in scope (widened by `project_id`, the same as `WorkflowRepo::list`) and
/// scores each in memory. Acceptable because the table is small; if that stops being
/// true, give `WorkflowRepo` a SQL prefilter like `DocRepo`'s.
fn search_workflows(q: &str, project_id: Option<Uuid>, workflows: &WorkflowRepo, name_of: &HashMap<Uuid, String>) -> Result<Vec<Candidate>> {
    let list = workflows.list(project_id)?;
    let mut out = Vec::with_capacity(list.len());
    for w in list {
        let score = score_of(q, Some(&w.name), Some(&w.name), Some(&w.description));
        if score <= 0.0 {
            continue;
        }
        let highlights = highlight(q, &w.name);
        out.push(Candidate {
            hit: SearchHit {
                kind: SearchKind::Workflow,
                id: w.id.to_string(),
                title: w.name.clone(),
                subtitle: w.project_id.and_then(|p| name_of.get(&p).cloned()),
                project_id: w.project_id,
                reference: None,
                score,
                highlights,
            },
            score,
            recency: w.updated_at,
        });
    }
    Ok(out)
}

/// One query across every entity kind.
///
/// A blank or whitespace-only `q` answers with no groups and a total of 0, without
/// touching the database: an empty search is not "everything", it's nothing yet.
///
/// Every other kind's SQL prefilter caps at 500 rows per source (newest first; see
/// the module doc comment), so `total` reflects at most 500 candidates from any one
/// of them even when the table holds far more true matches. Workflows are the
/// exception: see [`search_workflows`].
pub fn search(query: &SearchQuery, tasks: &TaskRepo, memories: &MemoryRepo, projects: &ProjectRepo, workflows: &WorkflowRepo) -> Result<SearchResult> {
    let start = std::time::Instant::now();
    let q = query.q.trim().to_lowercase();
    if q.is_empty() {
        return Ok(SearchResult { groups: Vec::new(), total: 0, took_ms: start.elapsed().as_millis() as u64 });
    }
    let limit = query.limit.min(MAX_LIMIT);
    let wanted: &[SearchKind] = query.kinds.as_deref().unwrap_or(&ORDER);
    let pattern = like_pattern(&q);

    // Files and commits read a project's already-loaded profile rather than a SQL
    // prefilter (see the module doc comment), so they still need the full, unfiltered
    // (scoped) project list; `name_of` (subtitles elsewhere) is built from the same
    // fetch rather than a second one.
    let all_projects = projects.list()?;
    let name_of: HashMap<Uuid, String> = all_projects.iter().map(|p| (p.id, p.name.clone())).collect();

    let mut total = 0usize;
    let mut groups = Vec::new();
    for kind in ORDER.into_iter().filter(|k| wanted.contains(k)) {
        let mut candidates = match kind {
            SearchKind::Task => search_tasks(&q, &pattern, query.project_id, tasks, &name_of)?,
            SearchKind::Memory => search_memories(&q, &pattern, query.project_id, memories, &name_of)?,
            SearchKind::Project => search_projects(&q, &pattern, query.project_id, projects)?,
            SearchKind::File => search_files(&q, query.project_id, &all_projects),
            SearchKind::Commit => search_commits(&q, query.project_id, &all_projects),
            SearchKind::Event => search_events(&q, &pattern, query.project_id, tasks, memories, &name_of)?,
            SearchKind::Workflow => search_workflows(&q, query.project_id, workflows, &name_of)?,
        };
        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| b.recency.cmp(&a.recency)));
        total += candidates.len();
        let items: Vec<SearchHit> = candidates.into_iter().take(limit).map(|c| c.hit).collect();
        if !items.is_empty() {
            groups.push(SearchGroup { kind, items });
        }
    }

    Ok(SearchResult { groups, total, took_ms: start.elapsed().as_millis() as u64 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::TaskRepo;
    use crate::db::Db;
    use crate::models::{NewMemory, NewTask, MemoryKind, MemoryScope, MemoryStatus};
    use crate::projects::detect::Detected;
    use std::sync::{Arc, Mutex};

    fn env() -> Arc<Db> {
        Arc::new(Db::open_in_memory().unwrap())
    }

    fn task_repo(db: &Arc<Db>) -> TaskRepo {
        TaskRepo::new(db.clone(), Arc::new(Mutex::new(())))
    }

    fn q(text: &str) -> SearchQuery {
        SearchQuery { q: text.into(), project_id: None, kinds: None, limit: DEFAULT_LIMIT }
    }

    fn run(db: &Arc<Db>, query: &SearchQuery) -> SearchResult {
        let tasks = task_repo(db);
        let memories = MemoryRepo::new(db);
        let projects = ProjectRepo::new(db);
        let workflows = WorkflowRepo::new(db.clone(), Arc::new(Mutex::new(())));
        search(query, &tasks, &memories, &projects, &workflows).unwrap()
    }

    fn group(r: &SearchResult, kind: SearchKind) -> Option<&SearchGroup> {
        r.groups.iter().find(|g| g.kind == kind)
    }

    #[test]
    fn exact_key_beats_title_beats_body() {
        let db = env();
        let tasks = task_repo(&db);
        let a = tasks.create(&NewTask { title: "unrelated".into(), description: Some("unrelated".into()), ..Default::default() }, "t").unwrap();
        let key = a.key.clone(); // e.g. "ATLAS-1"
        tasks.create(&NewTask { title: format!("mentions {key} explicitly"), ..Default::default() }, "t").unwrap();
        tasks.create(&NewTask { title: "y".into(), description: Some(format!("body mentions {key} here")), ..Default::default() }, "t").unwrap();

        let r = run(&db, &q(&key));
        let items = &group(&r, SearchKind::Task).unwrap().items;
        assert_eq!(items.len(), 3, "{items:?}");
        assert_eq!(items[0].id, a.id.to_string(), "exact key match must rank first: {items:?}");
        assert!(items[0].score > items[1].score && items[1].score > items[2].score, "{items:?}");
    }

    #[test]
    fn project_scoping_excludes_other_projects_but_keeps_global_memories() {
        let db = env();
        let tasks = task_repo(&db);
        let memories = MemoryRepo::new(&db);
        let projects = ProjectRepo::new(&db);
        let a = projects.upsert(&Detected { root: "/tmp/a".into(), remote: None }, None, "t").unwrap();
        let b = projects.upsert(&Detected { root: "/tmp/b".into(), remote: None }, None, "t").unwrap();

        tasks.create(&NewTask { project_id: Some(a.id), title: "widget for a".into(), ..Default::default() }, "t").unwrap();
        tasks.create(&NewTask { project_id: Some(b.id), title: "widget for b".into(), ..Default::default() }, "t").unwrap();

        memories.insert(&NewMemory { scope: MemoryScope::Project, project_id: Some(a.id), kind: MemoryKind::Fact, text: "widget note for a".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: MemoryStatus::Active }, "t").unwrap();
        memories.insert(&NewMemory { scope: MemoryScope::Project, project_id: Some(b.id), kind: MemoryKind::Fact, text: "widget note for b".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: MemoryStatus::Active }, "t").unwrap();
        memories.insert(&NewMemory { scope: MemoryScope::Global, project_id: None, kind: MemoryKind::Fact, text: "widget note global".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: MemoryStatus::Active }, "t").unwrap();

        let query = SearchQuery { q: "widget".into(), project_id: Some(a.id), kinds: None, limit: DEFAULT_LIMIT };
        let r = run(&db, &query);

        let task_titles: Vec<&str> = group(&r, SearchKind::Task).unwrap().items.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(task_titles, vec!["widget for a"], "{task_titles:?}");

        let mem_texts: Vec<&str> = group(&r, SearchKind::Memory).unwrap().items.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(mem_texts.len(), 2, "{mem_texts:?}");
        assert!(mem_texts.contains(&"widget note for a"), "{mem_texts:?}");
        assert!(mem_texts.contains(&"widget note global"), "{mem_texts:?}");
        assert!(!mem_texts.contains(&"widget note for b"), "{mem_texts:?}");
    }

    #[test]
    fn limit_caps_each_group_but_total_counts_every_match() {
        let db = env();
        let tasks = task_repo(&db);
        for i in 0..5 {
            tasks.create(&NewTask { title: format!("zeta task {i}"), ..Default::default() }, "t").unwrap();
        }
        let query = SearchQuery { q: "zeta".into(), project_id: None, kinds: Some(vec![SearchKind::Task]), limit: 2 };
        let r = run(&db, &query);
        assert_eq!(group(&r, SearchKind::Task).unwrap().items.len(), 2);
        assert_eq!(r.total, 5, "total must count every match before the per-group cap");
    }

    #[test]
    fn a_limit_above_the_max_clamps() {
        let db = env();
        let tasks = task_repo(&db);
        for i in 0..3 {
            tasks.create(&NewTask { title: format!("clampword {i}"), ..Default::default() }, "t").unwrap();
        }
        let query = SearchQuery { q: "clampword".into(), project_id: None, kinds: Some(vec![SearchKind::Task]), limit: 10_000 };
        let r = run(&db, &query);
        assert_eq!(group(&r, SearchKind::Task).unwrap().items.len(), 3);
    }

    #[test]
    fn empty_or_blank_query_returns_no_groups() {
        let db = env();
        let tasks = task_repo(&db);
        tasks.create(&NewTask { title: "something".into(), ..Default::default() }, "t").unwrap();
        for text in ["", "   "] {
            let r = run(&db, &q(text));
            assert!(r.groups.is_empty(), "{text:?}: {:?}", r.groups);
            assert_eq!(r.total, 0);
        }
    }

    #[test]
    fn an_unknown_kind_string_fails_to_parse() {
        assert!("bogus".parse::<SearchKind>().is_err());
        assert!("task".parse::<SearchKind>().is_ok());
        assert_eq!("Workflow".parse::<SearchKind>().unwrap(), SearchKind::Workflow);
    }

    #[test]
    fn kinds_filter_restricts_which_groups_run() {
        let db = env();
        let tasks = task_repo(&db);
        let memories = MemoryRepo::new(&db);
        tasks.create(&NewTask { title: "narrow word".into(), ..Default::default() }, "t").unwrap();
        memories.insert(&NewMemory { scope: MemoryScope::Global, project_id: None, kind: MemoryKind::Fact, text: "narrow word".into(), tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status: MemoryStatus::Active }, "t").unwrap();

        let query = SearchQuery { q: "narrow".into(), project_id: None, kinds: Some(vec![SearchKind::Task]), limit: DEFAULT_LIMIT };
        let r = run(&db, &query);
        assert!(group(&r, SearchKind::Task).is_some());
        assert!(group(&r, SearchKind::Memory).is_none(), "{:?}", r.groups);
    }

    #[test]
    fn highlights_are_char_offsets_into_title() {
        let db = env();
        let tasks = task_repo(&db);
        tasks.create(&NewTask { title: "café zeta".into(), ..Default::default() }, "t").unwrap();
        let r = run(&db, &q("zeta"));
        let items = &group(&r, SearchKind::Task).unwrap().items;
        assert_eq!(items.len(), 1);
        // "café zeta": c-a-f-é-space-z-e-t-a -> "zeta" starts at char index 5, not
        // byte index 6, since 'é' is a two-byte char.
        assert_eq!(items[0].highlights, vec![(5, 9)], "{items:?}");
    }

    #[test]
    fn event_highlights_land_in_the_field_that_actually_matched() {
        let db = env();
        let tasks = task_repo(&db);
        // `move_stage` writes one event whose kind is "moved" and whose body is
        // "moved <key> from Backlog to Done" (the board's own wording, which happens
        // to repeat "moved" but also carries "to Done" nowhere in the kind). A
        // highlight computed against the concatenated title instead of the field that
        // actually scored would land inside the "zeta-actor " actor prefix.
        let t = tasks.create(&NewTask { title: "task one".into(), ..Default::default() }, "zeta-actor").unwrap();
        tasks.move_stage(&t.key, "Done", None, "zeta-actor").unwrap();

        // Matched on `kind` ("moved"): highlight must sit right after "zeta-actor ".
        let r = run(&db, &q("moved"));
        let items = &group(&r, SearchKind::Event).unwrap().items;
        assert_eq!(items.len(), 1, "{items:?}");
        let kind_start = "zeta-actor ".chars().count();
        assert_eq!(items[0].highlights, vec![(kind_start, kind_start + "moved".chars().count())], "{items:?}");

        // Matched on `body` only ("...Backlog to Done"): highlight must sit at or
        // after "zeta-actor moved ", not inside the actor/kind prefix.
        let r = run(&db, &q("to done"));
        let items = &group(&r, SearchKind::Event).unwrap().items;
        assert_eq!(items.len(), 1, "{items:?}");
        let body_start = "zeta-actor moved ".chars().count();
        assert!(items[0].highlights[0].0 >= body_start, "highlight must not land before the body field starts: {items:?}");
    }

    #[test]
    fn more_than_500_matches_from_one_source_still_returns_capped_at_500() {
        let db = env();
        // Audit rows aren't produced through a public write API 600 times over, so
        // insert them directly, mirroring how other repos' tests hand-build rows for a
        // scenario the public API offers no shortcut for.
        db.with_conn(|c| {
            for i in 0..600 {
                c.execute(
                    "insert into audit (id, actor, action, entity, entity_id, detail) values (?, 't', 'noted', 'thing', null, ?::json)",
                    duckdb::params![Uuid::new_v4().to_string(), serde_json::json!({"note": format!("capword {i}")}).to_string()],
                )?;
            }
            Ok(())
        })
        .unwrap();

        let query = SearchQuery { q: "capword".into(), project_id: None, kinds: Some(vec![SearchKind::Event]), limit: MAX_LIMIT };
        let r = run(&db, &query);
        assert_eq!(r.total, 500, "the SQL prefilter caps each source at 500 candidates before scoring: {}", r.total);
        assert_eq!(group(&r, SearchKind::Event).unwrap().items.len(), MAX_LIMIT);
    }
}
