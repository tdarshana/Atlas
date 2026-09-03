//! The unified project log: everything that happened in one project, from four
//! tables that never shared a shape.
//!
//! Task events, the audit rows of the project's memories, the project's own audit
//! rows (and its sync targets'), and the extraction jobs run against it are read
//! separately, mapped onto one `LogEntry`, merged and sorted newest first. The
//! filters then run in Rust rather than SQL, because a filter over the merged list
//! is the only one that means the same thing for all four sources.

use crate::db::Db;
use crate::models::{LogEntry, LogFilter, LogRef};
use crate::Result;
use duckdb::params;
use uuid::Uuid;

/// Entries a filtered read answers with when the caller names no limit.
pub const DEFAULT_LIMIT: usize = 100;
/// The most a filtered read will answer with, whatever the caller asks for.
pub const MAX_LIMIT: usize = 500;

fn ts(us: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_micros(us).unwrap_or_default()
}

fn task_ref(id: Option<Uuid>, key: Option<String>) -> Option<LogRef> {
    Some(LogRef { kind: "task".into(), id, key })
}

/// Every entry for `project_id`, newest first, before any filter.
///
/// `root_path` is what an extraction job is matched by: the job payload records the
/// root it was queued from, not a project id.
fn collect(db: &Db, project_id: Uuid, root_path: &str) -> Result<Vec<LogEntry>> {
    let pid = project_id.to_string();
    let mut out: Vec<LogEntry> = Vec::new();

    db.with_conn(|c| {
        // Task events: the actor is the source, the event kind is the kind, and the
        // body is the detail. Joined to `tasks` so the entry can carry the task key.
        let mut st = c.prepare(
            "select epoch_us(e.created_at), e.actor, e.kind, e.body, e.task_id::text, t.key \
             from task_events e join tasks t on e.task_id = t.id where t.project_id = ?",
        )?;
        let mut rows = st.query(params![pid])?;
        while let Some(r) = rows.next()? {
            let task_id = Uuid::parse_str(&r.get::<_, String>(4)?).ok();
            let key: String = r.get(5)?;
            let body: String = r.get(3)?;
            let kind: String = r.get(2)?;
            out.push(LogEntry {
                time: ts(r.get(0)?),
                source: r.get(1)?,
                // A move or a claim carries no body, so the entry would read blank;
                // "moved" plus the task key is still the sentence the user wants.
                detail: if body.trim().is_empty() { format!("{kind} {key}") } else { body },
                kind,
                reference: task_ref(task_id, Some(key)),
            });
        }
        Ok(())
    })?;

    db.with_conn(|c| {
        // Memory audit rows, restricted to this project's memories. `forget_reason` is
        // deliberately left out: `forget` writes it alongside `supersede`, and one act
        // should be one line.
        let mut st = c.prepare(
            "select epoch_us(a.\"at\"), a.actor, a.action, m.text, m.id::text \
             from audit a join memories m on a.entity_id = m.id \
             where a.entity = 'memory' and m.project_id = ? and a.action in ('insert', 'supersede', 'set_status')",
        )?;
        let mut rows = st.query(params![pid])?;
        while let Some(r) = rows.next()? {
            let action: String = r.get(2)?;
            let kind = match action.as_str() {
                "insert" => "remembered",
                "supersede" => "forgotten",
                _ => "reviewed",
            };
            out.push(LogEntry {
                time: ts(r.get(0)?),
                source: r.get(1)?,
                kind: kind.into(),
                detail: r.get(3)?,
                reference: Some(LogRef { kind: "memory".into(), id: Uuid::parse_str(&r.get::<_, String>(4)?).ok(), key: None }),
            });
        }
        Ok(())
    })?;

    db.with_conn(|c| {
        // The project's own audit rows, plus any written against a sync target of it.
        let mut st = c.prepare(
            "select epoch_us(a.\"at\"), a.actor, a.action, a.entity, a.detail::text \
             from audit a where a.entity in ('project', 'sync') and a.entity_id = ?",
        )?;
        let mut rows = st.query(params![pid])?;
        while let Some(r) = rows.next()? {
            let action: String = r.get(2)?;
            let entity: String = r.get(3)?;
            let detail: Option<String> = r.get(4)?;
            let kind = if entity == "sync" {
                "synced"
            } else if action == "insert" {
                "connected"
            } else if action == "update" && detail.as_deref().is_some_and(|d| d.contains("\"profile\"")) {
                "refreshed"
            } else {
                "updated"
            };
            out.push(LogEntry {
                time: ts(r.get(0)?),
                source: r.get(1)?,
                kind: kind.into(),
                detail: detail.filter(|d| d != "null").unwrap_or(action),
                reference: Some(LogRef { kind: entity, id: Some(project_id), key: None }),
            });
        }
        Ok(())
    })?;

    db.with_conn(|c| {
        // Extraction jobs. The transcript itself is never selected: an ingest payload
        // can be a million characters, and none of them belong in a log line.
        let mut st = c.prepare(
            "select epoch_us(created_at), id::text, status, error, result::text, \
             json_extract_string(payload, '$.source_tool'), \
             json_extract_string(payload, '$.project_root'), \
             json_extract_string(payload, '$.project_id') \
             from jobs where kind = 'ingest' and status in ('done', 'failed')",
        )?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            let job_root: Option<String> = r.get(6)?;
            let job_project: Option<String> = r.get(7)?;
            if job_root.as_deref() != Some(root_path) && job_project.as_deref() != Some(pid.as_str()) {
                continue;
            }
            let status: String = r.get(2)?;
            let error: Option<String> = r.get(3)?;
            let result: Option<String> = r.get(4)?;
            out.push(LogEntry {
                time: ts(r.get(0)?),
                source: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "extractor".into()),
                kind: if status == "done" { "ingested".into() } else { "failed".into() },
                detail: error.or(result).unwrap_or(status),
                reference: Some(LogRef { kind: "job".into(), id: Uuid::parse_str(&r.get::<_, String>(1)?).ok(), key: None }),
            });
        }
        Ok(())
    })?;

    out.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(out)
}

/// The project's log, narrowed by `f` and capped. `after` pages: pass the `time` of
/// the last entry you saw to get the ones strictly older than it.
pub fn project_log(db: &Db, project_id: Uuid, f: &LogFilter) -> Result<Vec<LogEntry>> {
    let project = super::ProjectRepo::new(db).get(project_id)?;
    let all = collect(db, project_id, &project.root_path)?;
    let q = f.q.as_deref().map(str::to_lowercase).filter(|q| !q.is_empty());
    let limit = f.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    Ok(all
        .into_iter()
        .filter(|e| f.source.as_deref().filter(|s| !s.is_empty()).is_none_or(|s| s == e.source))
        .filter(|e| f.kind.as_deref().filter(|k| !k.is_empty()).is_none_or(|k| k == e.kind))
        .filter(|e| q.as_deref().is_none_or(|q| e.detail.to_lowercase().contains(q)))
        .filter(|e| f.after.is_none_or(|after| e.time < after))
        .take(limit)
        .collect())
}

/// The whole log as JSON lines, one entry per line, newest first and uncapped: an
/// export the user asked for is the one read that should not be paged.
pub fn project_log_export(db: &Db, project_id: Uuid) -> Result<String> {
    let project = super::ProjectRepo::new(db).get(project_id)?;
    let mut out = String::new();
    for entry in collect(db, project_id, &project.root_path)? {
        out.push_str(&serde_json::to_string(&entry)?);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MemoryKind, MemoryScope, MemoryStatus, NewMemory, NewTask};
    use crate::projects::{Detected, ProjectRepo};
    use std::sync::Arc;

    /// A project with one of everything the log reads: a task and its events, a
    /// remembered and then forgotten memory, the project's own audit rows, and a
    /// finished ingest job.
    fn fixture() -> (Arc<crate::db::Db>, Uuid, String) {
        let db = Arc::new(crate::db::Db::open_in_memory().unwrap());
        let root = "/tmp/logged";
        let projects = ProjectRepo::new(&db);
        // The upsert writes an `insert` audit row of its own: the "connected" entry.
        let project = projects.upsert(&Detected { root: root.into(), remote: None }, None, "cli").unwrap();

        let board = crate::board::TaskRepo::new(db.clone(), Default::default());
        let task = board
            .create(&NewTask { project_id: Some(project.id), title: "log me".into(), ..Default::default() }, "claude-code")
            .unwrap();
        board.comment(&task.key, "progress note", "codex").unwrap();

        let memories = crate::memories::MemoryRepo::new(&db);
        let memory = memories
            .insert(
                &NewMemory {
                    scope: MemoryScope::Project,
                    project_id: Some(project.id),
                    kind: MemoryKind::Fact,
                    text: "the deploy target is fly.io".into(),
                    tags: vec![],
                    source_agent: None,
                    source_tool: None,
                    confidence: 1.0,
                    status: MemoryStatus::Active,
                },
                "codex",
            )
            .unwrap();
        memories.supersede(memory.id, None, "desktop").unwrap();

        let jobs = crate::jobs::JobRepo::new(db.clone());
        let job = jobs.enqueue("ingest", serde_json::json!({"text": "a transcript", "source_tool": "claude-code", "project_root": root})).unwrap();
        jobs.mark_done(job, serde_json::json!({"inserted": 2})).unwrap();

        (db, project.id, task.key)
    }

    #[test]
    fn the_log_merges_every_source_newest_first_with_its_refs() {
        let (db, project_id, task_key) = fixture();
        let entries = project_log(&db, project_id, &LogFilter::default()).unwrap();

        let kinds: Vec<&str> = entries.iter().map(|e| e.kind.as_str()).collect();
        for want in ["created", "commented", "remembered", "forgotten", "connected", "ingested"] {
            assert!(kinds.contains(&want), "missing {want} in {kinds:?}");
        }
        for pair in entries.windows(2) {
            assert!(pair[0].time >= pair[1].time, "entries must be newest first: {entries:?}");
        }

        let commented = entries.iter().find(|e| e.kind == "commented").unwrap();
        assert_eq!(commented.source, "codex");
        assert_eq!(commented.detail, "progress note");
        let r = commented.reference.as_ref().unwrap();
        assert_eq!(r.kind, "task");
        assert_eq!(r.key.as_deref(), Some(task_key.as_str()));

        let remembered = entries.iter().find(|e| e.kind == "remembered").unwrap();
        assert_eq!(remembered.detail, "the deploy target is fly.io");
        assert_eq!(remembered.reference.as_ref().unwrap().kind, "memory");

        let ingested = entries.iter().find(|e| e.kind == "ingested").unwrap();
        assert_eq!(ingested.source, "claude-code");
        assert!(ingested.detail.contains("inserted"), "{}", ingested.detail);
        assert_eq!(ingested.reference.as_ref().unwrap().kind, "job");

        let connected = entries.iter().find(|e| e.kind == "connected").unwrap();
        assert_eq!(connected.reference.as_ref().unwrap().id, Some(project_id));
    }

    #[test]
    fn the_filters_narrow_the_merged_list() {
        let (db, project_id, _) = fixture();
        let all = project_log(&db, project_id, &LogFilter::default()).unwrap();

        let by_source = project_log(&db, project_id, &LogFilter { source: Some("codex".into()), ..Default::default() }).unwrap();
        assert!(!by_source.is_empty());
        assert!(by_source.iter().all(|e| e.source == "codex"), "{by_source:?}");

        let by_kind = project_log(&db, project_id, &LogFilter { kind: Some("remembered".into()), ..Default::default() }).unwrap();
        assert_eq!(by_kind.len(), 1);

        // Case-insensitive substring over the detail.
        let by_q = project_log(&db, project_id, &LogFilter { q: Some("FLY.IO".into()), ..Default::default() }).unwrap();
        assert!(by_q.iter().all(|e| e.detail.to_lowercase().contains("fly.io")), "{by_q:?}");
        assert!(!by_q.is_empty());

        let one = project_log(&db, project_id, &LogFilter { limit: Some(1), ..Default::default() }).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].time, all[0].time);

        // `after` pages: strictly older than the time it is given.
        let paged = project_log(&db, project_id, &LogFilter { after: Some(one[0].time), ..Default::default() }).unwrap();
        assert!(paged.iter().all(|e| e.time < one[0].time), "{paged:?}");
        assert!(paged.len() < all.len());

        // An empty string is a filter left off, not a filter that matches nothing.
        let blank = project_log(&db, project_id, &LogFilter { source: Some(String::new()), q: Some(String::new()), ..Default::default() }).unwrap();
        assert_eq!(blank.len(), all.len());
    }

    #[test]
    fn the_export_is_one_json_object_per_line() {
        let (db, project_id, _) = fixture();
        let all = project_log(&db, project_id, &LogFilter { limit: Some(MAX_LIMIT), ..Default::default() }).unwrap();
        let text = project_log_export(&db, project_id).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), all.len());
        for line in &lines {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v["time"].is_string() && v["source"].is_string() && v["kind"].is_string(), "{line}");
            assert!(v.get("ref").is_some(), "{line}");
        }
    }

    #[test]
    fn an_unknown_project_is_not_found() {
        let db = crate::db::Db::open_in_memory().unwrap();
        assert!(matches!(project_log(&db, Uuid::new_v4(), &LogFilter::default()), Err(crate::AtlasError::NotFound(_))));
    }
}
