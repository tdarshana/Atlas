//! Stage list rules: the default board, validation, parsing from JSON, and the
//! [`TaskRepo`] methods that decide which list applies and rewrite one.
//!
//! A stage list is the board's columns. It lives in settings under `board.stages`
//! and a project may override it. Every rule that decides whether a list is usable
//! lives here so the repository, the daemon and the GUI all refuse the same lists.

use super::{conv_err, TaskRepo, STAGES_SETTING};
use crate::memories::MemoryRepo;
use crate::models::{Stage, StageList};
use crate::settings::SettingsRepo;
use crate::{AtlasError, Result};
use duckdb::types::Type;
use duckdb::{params, params_from_iter, Connection};
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

/// Fewer than two columns is not a board; more than twelve does not fit a screen.
pub const MIN_STAGES: usize = 2;
pub const MAX_STAGES: usize = 12;

/// Backlog, In Progress, Testing, Done. Used when nothing is configured.
pub fn default_stages() -> Vec<Stage> {
    [("Backlog", false), ("In Progress", false), ("Testing", false), ("Done", true)]
        .into_iter()
        .map(|(name, done)| Stage { name: name.to_string(), done })
        .collect()
}

/// 2 to 12 stages, every name non-empty and unique once trimmed, at least one
/// `done` stage. Names are compared case-insensitively: two columns that differ
/// only in case would be indistinguishable on the board and in a move request.
pub fn validate_stages(stages: &[Stage]) -> Result<()> {
    if stages.len() < MIN_STAGES || stages.len() > MAX_STAGES {
        return Err(AtlasError::Invalid(format!(
            "a board needs {MIN_STAGES} to {MAX_STAGES} stages, got {}",
            stages.len()
        )));
    }
    let mut seen: Vec<String> = Vec::with_capacity(stages.len());
    for s in stages {
        let name = s.name.trim();
        if name.is_empty() {
            return Err(AtlasError::Invalid("a stage name cannot be empty".into()));
        }
        let folded = name.to_lowercase();
        if seen.contains(&folded) {
            return Err(AtlasError::Invalid(format!("duplicate stage name '{name}'")));
        }
        seen.push(folded);
    }
    if !stages.iter().any(|s| s.done) {
        return Err(AtlasError::Invalid("a board needs at least one done stage".into()));
    }
    Ok(())
}

/// Reads a stage list out of JSON (`[{"name": "Backlog", "done": false}, ...]`)
/// and validates it. Names are stored trimmed.
pub fn parse_stages(v: Value) -> Result<Vec<Stage>> {
    let stages: Vec<Stage> = serde_json::from_value(v)
        .map_err(|e| AtlasError::Invalid(format!("stages must be a list of {{name, done}}: {e}")))?;
    let stages: Vec<Stage> = stages.into_iter().map(|s| Stage { name: s.name.trim().to_string(), done: s.done }).collect();
    validate_stages(&stages)?;
    Ok(stages)
}

/// The stage in `stages` whose name matches `name` case-insensitively once
/// trimmed, so `atlas task move ATL-1 "in progress"` finds `In Progress`.
pub fn find_stage<'a>(stages: &'a [Stage], name: &str) -> Option<&'a Stage> {
    let want = name.trim().to_lowercase();
    stages.iter().find(|s| s.name.trim().to_lowercase() == want)
}

/// `Invalid` naming every valid stage, for a move to a stage that is not on the board.
pub fn unknown_stage(name: &str, stages: &[Stage]) -> AtlasError {
    let valid = stages.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
    AtlasError::Invalid(format!("unknown stage '{name}'; valid stages are: {valid}"))
}

/// The stage lists in play for a set of tasks, one per owning project.
pub(super) struct StageIndex {
    pub(super) by_project: HashMap<Option<Uuid>, Vec<Stage>>,
}

impl StageIndex {
    pub(super) fn is_done(&self, project_id: Option<Uuid>, stage: &str) -> bool {
        self.by_project
            .get(&project_id)
            .and_then(|s| find_stage(s, stage))
            .map(|s| s.done)
            // A task sitting in a stage that is no longer on the board is open, not done.
            .unwrap_or(false)
    }
}

/// The stage policy side of [`TaskRepo`]: which list applies where, and how the
/// global and per-project lists are rewritten. The SQL that moves tasks stays in
/// `board/mod.rs`; this block only decides and validates.
impl TaskRepo {
    /// The list a project's tasks are moved through: the project's own
    /// `board_stages` when it has one, else the `board.stages` setting, else the
    /// built-in default.
    pub fn effective_stages(&self, project_id: Option<Uuid>) -> Result<StageList> {
        self.db.with_conn(|c| self.stages_for(c, project_id))
    }

    fn global_stages(&self, c: &Connection) -> Result<Vec<Stage>> {
        let mut st = c.prepare("select value::text from settings where key = ?")?;
        let mut rows = st.query(params![STAGES_SETTING])?;
        match rows.next()? {
            Some(r) => {
                let text: String = r.get(0)?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                if v.is_null() {
                    Ok(default_stages())
                } else {
                    parse_stages(v)
                }
            }
            None => Ok(default_stages()),
        }
    }

    pub(super) fn stages_for(&self, c: &Connection, project_id: Option<Uuid>) -> Result<StageList> {
        if let Some(pid) = project_id {
            let mut st = c.prepare("select board_stages::text from projects where id = ?")?;
            let mut rows = st.query(params![pid.to_string()])?;
            // A missing project row falls back to the global list rather than failing
            // the read: `ProjectRepo::delete` leaves tasks behind, and a board that
            // cannot be listed is worse than one judged against the global stages.
            if let Some(row) = rows.next()? {
                if let Some(text) = row.get::<_, Option<String>>(0)? {
                    let v: serde_json::Value = serde_json::from_str(&text)?;
                    if !v.is_null() {
                        return Ok(StageList { stages: parse_stages(v)?, overridden: true });
                    }
                }
            }
        }
        Ok(StageList { stages: self.global_stages(c)?, overridden: false })
    }

    /// The stages a task would land in after `renames` is applied, or an error
    /// naming how many tasks sit in a stage the new list drops.
    fn check_stage_removals(&self, c: &Connection, scope_sql: &str, args: &[String], target: &[Stage], renames: &HashMap<String, String>) -> Result<()> {
        let mut st = c.prepare(&format!("select stage, count(*) from tasks where {scope_sql} group by stage"))?;
        let mut rows = st.query(params_from_iter(args.iter()))?;
        while let Some(r) = rows.next()? {
            let stage: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            let after = rename_of(renames, &stage).unwrap_or_else(|| stage.clone());
            if find_stage(target, &after).is_none() {
                return Err(AtlasError::Invalid(format!(
                    "stage '{stage}' still holds {n} task{}; move or rename them first",
                    if n == 1 { "" } else { "s" }
                )));
            }
        }
        Ok(())
    }

    /// Applies `renames` to the tasks in scope in a single pass, recording a `moved`
    /// event for each task that changes column.
    ///
    /// Each task's new stage is computed from the stage it was already in, never from
    /// one this call just wrote. `{Testing: Done, Done: Archive}` therefore leaves a
    /// task that was in `Testing` in `Done`, and a swap `{A: B, B: A}` exchanges the two
    /// columns instead of collapsing them. Applying the map entry by entry would make
    /// the result depend on `HashMap` iteration order, which is not defined.
    fn apply_renames(&self, c: &Connection, scope_sql: &str, args: &[String], renames: &HashMap<String, String>, actor: &str) -> Result<()> {
        if renames.is_empty() {
            return Ok(());
        }
        // Read every task in scope first, so no write can feed the next lookup.
        let mut rows: Vec<(String, String, String)> = Vec::new();
        {
            let mut st = c.prepare(&format!("select id::text, key, stage from tasks where {scope_sql} order by project_id nulls first, seq"))?;
            let mut r = st.query(params_from_iter(args.iter()))?;
            while let Some(row) = r.next()? {
                rows.push((row.get(0)?, row.get(1)?, row.get(2)?));
            }
        }
        for (id, key, from) in rows {
            let Some(to) = rename_of(renames, &from) else { continue };
            if to.eq_ignore_ascii_case(from.trim()) {
                continue;
            }
            c.execute("update tasks set stage = ?, updated_at = now() where id = ?", params![to, id])?;
            let uid = Uuid::parse_str(&id).map_err(|e| conv_err(0, Type::Text, e))?;
            self.event(
                c,
                uid,
                actor,
                "moved",
                &format!("moved {key} from {from} to {to}"),
                Some(json!({"from": from, "to": to, "reason": "stage renamed"})),
            )?;
        }
        Ok(())
    }

    /// Refuses a rename map that cannot be applied in one pass: two sources that differ
    /// only in case would be picked between by `HashMap` order, and a target that is not
    /// on the new board would move tasks into a column nothing can show.
    fn check_renames(renames: &HashMap<String, String>, target: &[Stage]) -> Result<()> {
        let mut seen: Vec<String> = Vec::with_capacity(renames.len());
        for (from, to) in renames {
            let folded = from.trim().to_lowercase();
            if seen.contains(&folded) {
                return Err(AtlasError::Invalid(format!("stage '{}' is renamed twice", from.trim())));
            }
            seen.push(folded);
            if find_stage(target, to).is_none() {
                return Err(unknown_stage(to, target));
            }
        }
        Ok(())
    }

    /// Rewrites the global stage list. `renames` maps an old stage name to a new
    /// one and carries the tasks along; dropping a stage that still holds tasks is
    /// refused with the count.
    pub fn set_global_stages(&self, stages: Vec<Stage>, renames: &HashMap<String, String>, actor: &str) -> Result<Vec<Stage>> {
        let _gate = self.gate();
        validate_stages(&stages)?;
        let stages = self.db.with_conn(|c| {
            // Tasks judged against the global list: global ones, plus every project
            // without its own override.
            let scope = "(project_id is null or project_id in (select id from projects where board_stages is null))";
            Self::check_renames(renames, &stages)?;
            self.check_stage_removals(c, scope, &[], &stages, renames)?;
            self.apply_renames(c, scope, &[], renames, actor)?;
            Ok(stages)
        })?;
        // Not `set_many`: that refuses `board.stages` outright, so no client can write
        // the list without the checks and the gate this method has just taken.
        SettingsRepo::new(&self.db).set_board_stages(&serde_json::to_value(&stages)?, actor)?;
        Ok(stages)
    }

    /// Sets or clears a project's stage override. `None` restores the global list.
    pub fn set_project_stages(&self, project_id: Uuid, stages: Option<Vec<Stage>>, renames: &HashMap<String, String>, actor: &str) -> Result<StageList> {
        let _gate = self.gate();
        if let Some(s) = &stages {
            validate_stages(s)?;
        }
        let out = self.db.with_conn(|c| {
            let n: i64 = c.query_row("select count(*) from projects where id = ?", params![project_id.to_string()], |r| r.get(0))?;
            if n == 0 {
                return Err(AtlasError::NotFound(format!("project {project_id}")));
            }
            let target = match &stages {
                Some(s) => s.clone(),
                None => self.global_stages(c)?,
            };
            let scope = "project_id = ?";
            let args = vec![project_id.to_string()];
            Self::check_renames(renames, &target)?;
            self.check_stage_removals(c, scope, &args, &target, renames)?;
            self.apply_renames(c, scope, &args, renames, actor)?;
            match &stages {
                Some(s) => c.execute(
                    "update projects set board_stages = ?::json where id = ?",
                    params![serde_json::to_string(s)?, project_id.to_string()],
                )?,
                None => c.execute("update projects set board_stages = null where id = ?", params![project_id.to_string()])?,
            };
            self.stages_for(c, Some(project_id))
        })?;
        MemoryRepo::new(&self.db).audit(actor, "set_board_stages", "project", Some(project_id), json!({"stages": stages, "renames": renames}))?;
        Ok(out)
    }
}

/// The new name for `stage` in a rename map, matched case-insensitively on
/// trimmed names so a map built from a GUI form still lines up with stored rows.
fn rename_of(renames: &HashMap<String, String>, stage: &str) -> Option<String> {
    renames
        .iter()
        .find(|(from, _)| from.trim().eq_ignore_ascii_case(stage.trim()))
        .map(|(_, to)| to.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(name: &str, done: bool) -> Stage {
        Stage { name: name.into(), done }
    }

    #[test]
    fn the_default_list_validates() {
        let d = default_stages();
        assert_eq!(d.len(), 4);
        assert_eq!(d[0].name, "Backlog");
        assert_eq!(d[1].name, "In Progress");
        assert_eq!(d[2].name, "Testing");
        assert!(d[3].done && d[3].name == "Done");
        validate_stages(&d).unwrap();
    }

    #[test]
    fn a_list_with_one_stage_is_refused() {
        let err = validate_stages(&[stage("Only", true)]).unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
        assert!(err.to_string().contains("2 to 12"), "{err}");
    }

    #[test]
    fn too_many_stages_are_refused() {
        let mut many: Vec<Stage> = (0..13).map(|i| stage(&format!("S{i}"), false)).collect();
        many[0].done = true;
        assert!(validate_stages(&many).is_err());
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let err = validate_stages(&[stage("Backlog", false), stage("backlog", false), stage("Done", true)]).unwrap_err();
        assert!(err.to_string().contains("duplicate"), "{err}");
    }

    #[test]
    fn an_empty_name_is_refused() {
        let err = validate_stages(&[stage("  ", false), stage("Done", true)]).unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
    }

    #[test]
    fn a_list_with_no_done_stage_is_refused() {
        let err = validate_stages(&[stage("Backlog", false), stage("Doing", false)]).unwrap_err();
        assert!(err.to_string().contains("done stage"), "{err}");
    }

    #[test]
    fn parse_stages_trims_names_and_validates() {
        let v = serde_json::json!([{"name": " Backlog ", "done": false}, {"name": "Done", "done": true}]);
        let parsed = parse_stages(v).unwrap();
        assert_eq!(parsed[0].name, "Backlog");
        assert!(parse_stages(serde_json::json!("Backlog,Done")).is_err());
        assert!(parse_stages(serde_json::json!([{"name": "Only", "done": true}])).is_err());
    }

    #[test]
    fn find_stage_is_case_insensitive_and_unknown_stage_lists_the_valid_ones() {
        let d = default_stages();
        assert_eq!(find_stage(&d, "in progress").unwrap().name, "In Progress");
        assert!(find_stage(&d, "Nope").is_none());
        let err = unknown_stage("Nope", &d).to_string();
        assert!(err.contains("Backlog") && err.contains("Done"), "{err}");
    }
}
