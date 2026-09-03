//! Stage list rules: the default board, validation, and parsing from JSON.
//!
//! A stage list is the board's columns. It lives in settings under `board.stages`
//! and a project may override it. Every rule that decides whether a list is usable
//! lives here so the repository, the daemon and the GUI all refuse the same lists.

use crate::models::Stage;
use crate::{AtlasError, Result};
use serde_json::Value;

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
