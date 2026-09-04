//! Markdown rendering of the board: one `## <stage>` section per stage, in board
//! order, with `- KEY title (assignee)` lines. Shared by the
//! `atlas://projects/{name}/board` MCP resource and, later, the `TASKS.md` sync mirror.

use crate::models::{Stage, Task};

/// A done stage holding more than this many tasks collapses to a single count line
/// instead of listing each one, so a long-lived board's finished history does not
/// dwarf the columns still in play.
const DONE_COLLAPSE_THRESHOLD: usize = 10;

/// Renders `tasks` grouped by `stage`, in the order stages are given. A stage with
/// no tasks still gets its heading, with nothing under it.
pub fn render_board_markdown(stages: &[Stage], tasks: &[Task]) -> String {
    let mut out = String::new();
    for stage in stages {
        out.push_str(&format!("## {}\n", stage.name));
        let in_stage: Vec<&Task> = tasks.iter().filter(|t| t.stage == stage.name).collect();
        if stage.done && in_stage.len() > DONE_COLLAPSE_THRESHOLD {
            out.push_str(&format!("{} done\n", in_stage.len()));
        } else {
            for t in in_stage {
                let assignee = t.assignee.as_deref().unwrap_or("unassigned");
                out.push_str(&format!("- {} {} ({assignee})\n", t.key, t.title));
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn task(key: &str, stage: &str, assignee: Option<&str>) -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            key: key.into(),
            project_id: None,
            seq: 1,
            title: format!("{key} title"),
            description: String::new(),
            stage: stage.into(),
            kind: crate::models::TaskKind::Task,
            priority: crate::models::TaskPriority::Medium,
            assignee: assignee.map(String::from),
            labels: vec![],
            parent_id: None,
            created_by: "t".into(),
            created_at: now,
            updated_at: now,
            closed_at: None,
            source_ref: None,
            blocked_by: vec![],
            open_blockers: 0,
            ready: false,
            blocked_reason: None,
            subtasks_total: 0,
            subtasks_done: 0,
        }
    }

    #[test]
    fn renders_a_section_per_stage_with_key_title_and_assignee() {
        let stages = vec![Stage { name: "Backlog".into(), done: false }, Stage { name: "Done".into(), done: true }];
        let tasks = vec![task("ATL-1", "Backlog", Some("codex")), task("ATL-2", "Backlog", None)];
        let md = render_board_markdown(&stages, &tasks);
        assert!(md.contains("## Backlog\n"));
        assert!(md.contains("- ATL-1 ATL-1 title (codex)\n"));
        assert!(md.contains("- ATL-2 ATL-2 title (unassigned)\n"));
        assert!(md.contains("## Done\n"));
    }

    #[test]
    fn a_done_stage_over_the_threshold_collapses_to_a_count() {
        let stages = vec![Stage { name: "Done".into(), done: true }];
        let tasks: Vec<Task> = (0..11).map(|i| task(&format!("ATL-{i}"), "Done", None)).collect();
        let md = render_board_markdown(&stages, &tasks);
        assert!(md.contains("11 done\n"), "{md}");
        assert!(!md.contains("ATL-0"), "{md}");
    }

    #[test]
    fn a_done_stage_at_the_threshold_still_lists_tasks() {
        let stages = vec![Stage { name: "Done".into(), done: true }];
        let tasks: Vec<Task> = (0..10).map(|i| task(&format!("ATL-{i}"), "Done", None)).collect();
        let md = render_board_markdown(&stages, &tasks);
        assert!(md.contains("- ATL-0"), "{md}");
        assert!(!md.contains("done\n"), "{md}");
    }
}
