mod prompt;
mod summary;
pub use prompt::{project_summary_prompt, EXTRACTION_SYSTEM_PROMPT};
pub use summary::summarize_project;

use crate::backend::LocalBackend;
use crate::db::Db;
use crate::jobs::Job;
use crate::llm::LlmClient;
use crate::models::{MemoryKind, MemoryScope, MemoryStatus, NewMemory};
use crate::projects::{detect_root, ProjectRepo};
use crate::service::MemoryService;
use crate::settings::SettingsRepo;
use crate::{AtlasError, Result};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

/// What `POST /ingest` answers with when extraction is off or half configured.
/// The exact text is part of the API: clients match on it.
pub const DISABLED: &str = "extraction is disabled";

/// The `source_agent` every extracted memory carries, so a reviewer can tell a
/// machine-proposed memory from one an agent wrote deliberately.
pub const EXTRACTOR: &str = "extractor";

/// A candidate this close to an existing memory is the same memory said twice.
const DUPLICATE_COSINE: f64 = 0.92;

/// The extraction settings, read unmasked, once the enable gate is open.
pub struct ExtractionConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub auto_accept_min_confidence: f64,
}

/// Written by hand rather than derived: a derived `Debug` would put the api key
/// into any log line or panic message that formats the config.
impl std::fmt::Debug for ExtractionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractionConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("auto_accept_min_confidence", &self.auto_accept_min_confidence)
            .finish_non_exhaustive()
    }
}

/// The extraction settings for a scope, or the feature reported as disabled.
///
/// A project may override the global `extraction.*` settings field by field: an
/// absent field on the override falls back to the global value, so a project can
/// point one endpoint elsewhere without restating the model or the threshold.
/// Extraction is off by default and stays off unless the resolved `enabled` is true
/// and both `base_url` and `model` resolve to something; the api key may be empty,
/// since a local endpoint does not ask for one.
///
/// The global values come from `SettingsRepo::get_raw` and the project's from
/// `ProjectRepo::extraction_raw`, not from the masking read paths: a `"***"` is not
/// a key the worker could use. The key is never logged and never put in an error.
pub fn resolve_extraction(db: &Db, project_id: Option<Uuid>) -> Result<ExtractionConfig> {
    let over = match project_id {
        Some(id) => ProjectRepo::new(db).extraction_raw(id)?,
        None => None,
    };
    let settings = SettingsRepo::new(db);
    let disabled = || AtlasError::Conflict(DISABLED.to_string());
    let global_text = |key: &str| -> Result<Option<String>> {
        Ok(settings.get_raw(key)?.and_then(|v| v.as_str().map(str::to_string)).filter(|s| !s.trim().is_empty()))
    };
    let enabled = match over.as_ref().and_then(|o| o.enabled) {
        Some(v) => v,
        None => settings.get_raw("extraction.enabled")?.and_then(|v| v.as_bool()) == Some(true),
    };
    if !enabled {
        return Err(disabled());
    }
    let text = |value: Option<&String>, key: &str| -> Result<String> {
        match value.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(v) => Ok(v.to_string()),
            None => global_text(key)?.ok_or_else(disabled),
        }
    };
    let api_key = match over.as_ref().and_then(|o| o.api_key.as_deref()).filter(|k| !k.is_empty()) {
        Some(k) => k.to_string(),
        None => settings.get_raw("extraction.api_key")?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
    };
    Ok(ExtractionConfig {
        base_url: text(over.as_ref().and_then(|o| o.base_url.as_ref()), "extraction.base_url")?,
        model: text(over.as_ref().and_then(|o| o.model.as_ref()), "extraction.model")?,
        api_key,
        // Defaulting to 1.0 means nothing is auto-accepted until the user lowers it.
        auto_accept_min_confidence: over
            .as_ref()
            .and_then(|o| o.auto_accept_min_confidence)
            .or_else(|| settings.get_raw("extraction.auto_accept_min_confidence").ok().flatten().and_then(|v| v.as_f64()))
            .unwrap_or(1.0),
    })
}

/// The global extraction settings, with no project override in play.
pub fn extraction_config(db: &Db) -> Result<ExtractionConfig> {
    resolve_extraction(db, None)
}

/// A memory extracted from a transcript by the model, before it is turned
/// into a `NewMemory` and persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub text: String,
    pub kind: MemoryKind,
    pub tags: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Deserialize)]
struct RawCandidate {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    tags: Option<serde_json::Value>,
    /// Deserialized as a bare `Value` rather than an `f64` so a model that answers
    /// `"confidence": "high"` loses only its confidence, not the whole candidate.
    #[serde(default)]
    confidence: Option<serde_json::Value>,
}

/// Parses the model's response into candidates. Tolerates a Markdown code
/// fence around the array. The array is deserialized one item at a time so a
/// single malformed item (not an object, or a `tags` that isn't an array)
/// cannot sink the whole batch: such items are skipped with a warning, as
/// are items whose `text` is missing or blank. An unrecognized `kind` maps to
/// `MemoryKind::Insight` with a warning, and a missing `confidence` reads as
/// 0.0. A response that is not a JSON array at all is an error.
pub fn parse_candidates(json_text: &str) -> Result<Vec<Candidate>> {
    let trimmed = strip_fence(json_text.trim());
    let items: Vec<serde_json::Value> = serde_json::from_str(trimmed)
        .map_err(|e| AtlasError::Other(format!("model response was not a JSON array: {e}")))?;

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let raw: RawCandidate = match serde_json::from_value(item) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("extraction candidate item was not an object, skipping: {e}");
                continue;
            }
        };
        let Some(text) = raw.text else {
            tracing::warn!("extraction candidate missing text, skipping");
            continue;
        };
        // A blank `text` is a missing one that happens to be spelled `""`: storing it
        // would put a memory nobody can read into the review queue.
        if text.trim().is_empty() {
            tracing::warn!("extraction candidate text was blank, skipping");
            continue;
        }
        let kind = raw
            .kind
            .as_deref()
            .and_then(|k| MemoryKind::from_str(k).ok())
            .unwrap_or_else(|| {
                tracing::warn!(kind = raw.kind.as_deref().unwrap_or(""), "unrecognized memory kind, defaulting to insight");
                MemoryKind::Insight
            });
        // A model that omits `confidence`, or sends something that is not a number,
        // has said nothing about how sure it is, and 0.0 is the honest reading of
        // that. Defaulting to 1.0 would have cleared even the strictest auto-accept
        // threshold, so the most common malformed answer would have skipped review.
        let confidence = parse_confidence(raw.confidence);
        out.push(Candidate { text, kind, tags: parse_tags(raw.tags), confidence });
    }
    Ok(out)
}

/// A candidate's confidence, clamped into 0..1. A model that omits the field, or
/// sends something that is not a number, has said nothing about how sure it is,
/// and 0.0 is the honest reading of that: defaulting to 1.0 would clear even the
/// strictest auto-accept threshold, so the most common malformed answer would
/// have skipped review entirely.
fn parse_confidence(value: Option<Value>) -> f64 {
    match value {
        None | Some(Value::Null) => 0.0,
        Some(v) => match v.as_f64() {
            Some(n) => n.clamp(0.0, 1.0),
            None => {
                tracing::warn!(?v, "confidence was not a number, reading it as 0");
                0.0
            }
        },
    }
}

/// Whether a candidate may be stored `active` outright. Everything waits for
/// review unless the user has lowered the bar far enough that this candidate
/// clears it, and a candidate carrying no confidence never clears the default.
fn status_for(confidence: f64, threshold: f64) -> MemoryStatus {
    if confidence >= threshold { MemoryStatus::Active } else { MemoryStatus::Pending }
}

/// `tags` is expected to be an array of strings, but the model can send
/// `null`, omit the field, or send the wrong shape. Any of those become an
/// empty list; non-string entries inside an array are dropped with a
/// warning rather than failing the item.
fn parse_tags(value: Option<serde_json::Value>) -> Vec<String> {
    match value {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s),
                other => {
                    tracing::warn!(?other, "non-string tag entry dropped");
                    None
                }
            })
            .collect(),
        Some(other) => {
            tracing::warn!(?other, "tags field was not an array, ignoring");
            Vec::new()
        }
    }
}

fn strip_fence(s: &str) -> &str {
    let s = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")).unwrap_or(s);
    s.strip_suffix("```").unwrap_or(s).trim()
}

/// Runs extraction end to end: asks the model to read `transcript` and
/// returns the parsed candidates.
pub async fn extract_candidates(transcript: &str, client: &LlmClient) -> Result<Vec<Candidate>> {
    let response = client.chat(EXTRACTION_SYSTEM_PROMPT, transcript).await?;
    parse_candidates(&response)
}

/// `text` reduced to the form two memories have to share to count as the same
/// one when no embedding is available: lowercased, runs of whitespace collapsed
/// to one space, trailing punctuation dropped.
pub fn normalize_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    collapsed.trim_end_matches(|c: char| c.is_ascii_punctuation()).to_string()
}

/// Drops candidates the store already holds, and candidates the batch repeats.
///
/// `project_id` is the scope the candidates will be stored under, and it bounds
/// what they are compared against: a candidate for project P is a duplicate only
/// of P's own memories or of a global one, and a global candidate only of another
/// global memory. The daemon serves every project at once, so without this a
/// sentence one project already recorded would silently swallow another project's
/// version of the same fact.
///
/// When the embedder is ready the test against stored memories is cosine
/// similarity; when it is not (no model loaded, or the text will not embed) it
/// falls back to comparing the normalized text. The text comparison also runs on
/// its own account, because an exact repeat is a duplicate whatever the vectors
/// say, and because it is the only thing that can catch a candidate matching a
/// `pending` memory: pending rows are deliberately kept out of the vector map,
/// but re-proposing one would double up the review queue.
pub fn dedupe(candidates: Vec<Candidate>, memories: &MemoryService, project_id: Option<Uuid>) -> Result<Vec<Candidate>> {
    // `list` with a project widens to that project plus every global memory; with no
    // project, the global scope narrows it to global memories alone.
    let scope = project_id.is_none().then_some(MemoryScope::Global);
    let mut seen: Vec<String> = Vec::new();
    for status in [MemoryStatus::Active, MemoryStatus::Pending] {
        seen.extend(memories.list(status, scope, project_id)?.into_iter().map(|m| normalize_text(&m.text)));
    }
    let mut kept = Vec::with_capacity(candidates.len());
    for c in candidates {
        let normalized = normalize_text(&c.text);
        let duplicate = seen.contains(&normalized)
            || memories.nearest_active(&c.text, project_id)?.is_some_and(|(_, score)| score >= DUPLICATE_COSINE);
        if duplicate {
            continue;
        }
        seen.push(normalized);
        kept.push(c);
    }
    Ok(kept)
}

/// The project an ingest is scoped to. An unknown root is not an error: the
/// memories simply land global, which is what a transcript from a directory
/// Atlas has never connected deserves.
fn project_for(db: &Db, root: Option<PathBuf>) -> Result<Option<Uuid>> {
    let Some(root) = root else { return Ok(None) };
    // Resolve the way `connect_project` does, so a path inside a repository finds
    // the row stored under the repository root; an unresolvable path is looked up
    // as given and simply misses.
    let resolved = detect_root(&root).map(|d| d.root).unwrap_or(root);
    Ok(ProjectRepo::new(db).by_root(&resolved)?.map(|p| p.id))
}

/// Builds the client extraction talks to, from an already-read `ExtractionConfig`.
/// Both `run_ingest` and `Backend::test_extraction_for` go through this rather than
/// each constructing an `LlmClient` of their own, so the two ways of reaching the
/// model never drift apart.
pub fn build_client(cfg: &ExtractionConfig) -> Result<LlmClient> {
    LlmClient::new(&cfg.base_url, &cfg.api_key, &cfg.model)
}

/// Runs one `ingest` job: read the transcript out of the payload, ask the model
/// for candidates, drop the duplicates, and store what is left.
///
/// The HTTP call to the model happens before anything touches DuckDB, so the
/// write gate is never held across an await. Each insert then goes through
/// `MemoryService::remember`, which takes the gate itself.
pub async fn run_ingest(job: &Job, backend: &LocalBackend) -> Result<Value> {
    let text = job.payload["text"].as_str().ok_or_else(|| AtlasError::Invalid("ingest job has no text".into()))?;
    let source_tool = job.payload["source_tool"].as_str().unwrap_or("ingest").to_string();
    let root = job.payload["project_root"].as_str().map(PathBuf::from);

    // The project is resolved before the settings are read, because the project may
    // override them: an ingest for a project with its own endpoint must not go to the
    // global one.
    let project_id = project_for(&backend.db, root)?;
    let cfg = resolve_extraction(&backend.db, project_id)?;
    let client = build_client(&cfg)?;
    let candidates = extract_candidates(text, &client).await?;
    let found = candidates.len();

    let keep = dedupe(candidates, &backend.memories, project_id)?;
    let skipped_duplicates = found - keep.len();

    // An insert that fails partway leaves the earlier memories stored, so the count
    // is tracked as we go and the audit row is written either way: the trail has to
    // name what actually landed, not what was attempted.
    let mut inserted = 0usize;
    let mut failure = None;
    for c in keep {
        let stored = backend.memories.remember(
            NewMemory {
                scope: if project_id.is_some() { MemoryScope::Project } else { MemoryScope::Global },
                project_id,
                kind: c.kind,
                text: c.text,
                tags: c.tags,
                source_agent: Some(EXTRACTOR.to_string()),
                source_tool: Some(source_tool.clone()),
                confidence: c.confidence,
                status: status_for(c.confidence, cfg.auto_accept_min_confidence),
            },
            EXTRACTOR,
        );
        match stored {
            Ok(_) => inserted += 1,
            Err(e) => { failure = Some(e); break; }
        }
    }

    backend.memories.audit(
        EXTRACTOR,
        "extract",
        "job",
        Some(job.id),
        serde_json::json!({
            "job_id": job.id,
            "candidates": found,
            "inserted": inserted,
            "skipped_duplicates": skipped_duplicates,
            "model": cfg.model,
        }),
    )?;
    match failure {
        Some(e) => Err(e),
        None => Ok(serde_json::json!({"inserted": inserted, "skipped_duplicates": skipped_duplicates})),
    }
}

/// Runs one `project_summary` job: load the project named by `payload.project_id`,
/// ask the model to condense its profile into a couple of sentences, and store the
/// result into `profile.summary`. Enqueued by `LocalBackend::refresh_project` when
/// extraction is enabled.
pub async fn run_project_summary(job: &Job, backend: &LocalBackend) -> Result<Value> {
    let project_id = job.payload["project_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AtlasError::Invalid("project_summary job has no project_id".into()))?;

    // The project's own override applies to its summary, the same as to an ingest.
    let cfg = resolve_extraction(&backend.db, Some(project_id))?;
    let client = build_client(&cfg)?;

    let repo = ProjectRepo::new(&backend.db);
    let profile = repo
        .get(project_id)?
        .profile
        .ok_or_else(|| AtlasError::Invalid(format!("project {project_id} has no profile to summarize")))?;

    // The model call happens here, before the gate is taken: `write_gate` is a
    // blocking mutex and must never be held across an await.
    let summary = summarize_project(&profile, &client).await?;
    let chars = summary.chars().count();

    // The profile read above is now potentially stale: a `refresh_project` running
    // alongside this job may have rescanned the repository and written new languages,
    // file counts and README excerpt. Re-read it under the gate and change only
    // `summary`, so writing the summary cannot revert that scan.
    {
        let _gate = backend.memories.write_gate();
        let mut profile = repo
            .get(project_id)?
            .profile
            .ok_or_else(|| AtlasError::Invalid(format!("project {project_id} has no profile to summarize")))?;
        profile.summary = Some(summary);
        repo.set_profile(project_id, &profile, EXTRACTOR)?;
    }
    Ok(serde_json::json!({"chars": chars}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The project override wins field by field; every field it leaves out falls back
    /// to the global setting, and a project may switch extraction on where the global
    /// setting has it off.
    #[test]
    fn resolve_extraction_prefers_the_project_field_by_field() {
        use crate::models::ProjectExtraction;
        use crate::projects::{Detected, ProjectRepo};
        let db = Db::open_in_memory().unwrap();
        let settings = SettingsRepo::new(&db);
        settings
            .set_many(
                &serde_json::Map::from_iter([
                    ("extraction.enabled".to_string(), serde_json::Value::from(true)),
                    ("extraction.base_url".to_string(), "https://global.example/v1".into()),
                    ("extraction.model".to_string(), "global-model".into()),
                    ("extraction.api_key".to_string(), "sk-global".into()),
                    ("extraction.auto_accept_min_confidence".to_string(), serde_json::Value::from(0.5)),
                ]),
                "t",
            )
            .unwrap();
        let projects = ProjectRepo::new(&db);
        let p = projects.upsert(&Detected { root: "/tmp/over".into(), remote: None }, None, "t").unwrap();

        // No override: the global values, unchanged.
        let global = resolve_extraction(&db, Some(p.id)).unwrap();
        assert_eq!(global.base_url, "https://global.example/v1");
        assert_eq!(global.model, "global-model");
        assert_eq!(global.api_key, "sk-global");
        assert_eq!(global.auto_accept_min_confidence, 0.5);

        // A partial override replaces only the fields it names.
        projects
            .set_project_extraction(
                p.id,
                Some(ProjectExtraction { model: Some("project-model".into()), api_key: Some("sk-project".into()), ..Default::default() }),
                "t",
            )
            .unwrap();
        let merged = resolve_extraction(&db, Some(p.id)).unwrap();
        assert_eq!(merged.model, "project-model");
        assert_eq!(merged.api_key, "sk-project");
        assert_eq!(merged.base_url, "https://global.example/v1", "an absent field falls back");
        assert_eq!(merged.auto_accept_min_confidence, 0.5);
        // The global scope never sees the project's values.
        assert_eq!(resolve_extraction(&db, None).unwrap().model, "global-model");

        // The global switch off, the project's own switch on.
        settings.set_many(&serde_json::Map::from_iter([("extraction.enabled".to_string(), serde_json::Value::from(false))]), "t").unwrap();
        assert!(matches!(resolve_extraction(&db, None), Err(AtlasError::Conflict(_))));
        assert!(matches!(resolve_extraction(&db, Some(p.id)), Err(AtlasError::Conflict(_))));
        projects
            .set_project_extraction(
                p.id,
                Some(ProjectExtraction {
                    enabled: Some(true),
                    base_url: Some("http://localhost:1234/v1".into()),
                    model: Some("project-model".into()),
                    api_key: Some("sk-project".into()),
                    auto_accept_min_confidence: Some(0.9),
                }),
                "t",
            )
            .unwrap();
        let on = resolve_extraction(&db, Some(p.id)).unwrap();
        assert_eq!(on.base_url, "http://localhost:1234/v1");
        assert_eq!(on.auto_accept_min_confidence, 0.9);
        assert!(matches!(resolve_extraction(&db, None), Err(AtlasError::Conflict(_))), "the global scope stays off");
    }

    #[test]
    fn parses_bare_array() {
        let json = r#"[{"text": "uses bun", "kind": "fact", "tags": ["tooling"], "confidence": 0.9}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "uses bun");
        assert_eq!(out[0].kind, MemoryKind::Fact);
        assert_eq!(out[0].tags, vec!["tooling".to_string()]);
        assert_eq!(out[0].confidence, 0.9);
    }

    #[test]
    fn parses_fenced_array() {
        let json = "```json\n[{\"text\": \"uses bun\", \"kind\": \"fact\"}]\n```";
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "uses bun");
    }

    #[test]
    fn parses_plain_fenced_array_without_json_tag() {
        let json = "```\n[{\"text\": \"uses bun\", \"kind\": \"fact\"}]\n```";
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn skips_item_without_text() {
        let json = r#"[{"kind": "fact"}, {"text": "kept", "kind": "fact"}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "kept");
    }

    #[test]
    fn unknown_kind_maps_to_insight() {
        let json = r#"[{"text": "something", "kind": "opinion"}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MemoryKind::Insight);
    }

    #[test]
    fn confidence_is_clamped_into_0_1() {
        let json = r#"[{"text": "a", "kind": "fact", "confidence": 1.5}, {"text": "b", "kind": "fact", "confidence": -0.3}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out[0].confidence, 1.0);
        assert_eq!(out[1].confidence, 0.0);
    }

    /// Dropping `confidence` is the most common thing a model gets wrong about this
    /// prompt, so it must be the safe case rather than the maximally confident one.
    #[test]
    fn a_missing_or_unusable_confidence_reads_as_zero() {
        let json = r#"[{"text": "a", "kind": "fact"}, {"text": "b", "kind": "fact", "confidence": null}, {"text": "c", "kind": "fact", "confidence": "high"}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 3, "a confidence of the wrong type loses the confidence, not the candidate: {out:?}");
        for c in &out {
            assert_eq!(c.confidence, 0.0, "{c:?}");
        }
    }

    /// The auto-accept boundary, which is the whole of the review gate's safety story.
    /// `>=` is deliberate: a threshold is the lowest confidence that may skip review.
    #[test]
    fn auto_accept_is_inclusive_at_the_threshold_and_never_reached_without_a_confidence() {
        assert_eq!(status_for(0.8, 0.8), MemoryStatus::Active, "confidence at the threshold is accepted");
        assert_eq!(status_for(0.79, 0.8), MemoryStatus::Pending, "just below the threshold waits for review");
        assert_eq!(status_for(1.0, 1.0), MemoryStatus::Active, "the default threshold accepts a fully confident candidate");

        // A candidate the model gave no confidence for carries 0.0, so it waits at the
        // default threshold and at a lowered one alike.
        let missing = parse_candidates(r#"[{"text": "a", "kind": "fact"}]"#).unwrap();
        for threshold in [1.0, 0.5] {
            assert_eq!(status_for(missing[0].confidence, threshold), MemoryStatus::Pending, "threshold {threshold}");
        }
    }

    /// A model that emits `{"text": ""}` must not put a blank row into the review queue.
    #[test]
    fn a_blank_text_is_skipped_like_a_missing_one() {
        let json = r#"[{"text": "", "kind": "fact"}, {"text": "   \n\t ", "kind": "fact"}, {"text": "kept", "kind": "fact"}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].text, "kept");
    }

    #[test]
    fn non_json_response_is_an_error() {
        let err = parse_candidates("not json at all").unwrap_err();
        assert!(matches!(err, AtlasError::Other(_)));
    }

    #[test]
    fn null_tags_yields_empty_vec_without_failing_the_batch() {
        let json = r#"[{"text": "a", "kind": "fact", "tags": null}, {"text": "b", "kind": "fact", "tags": ["real"]}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].tags.is_empty());
        assert_eq!(out[1].tags, vec!["real".to_string()]);
    }

    #[test]
    fn malformed_item_is_skipped_without_failing_the_batch() {
        let json = r#"[{"text": "a", "kind": "fact"}, "oops", {"text": "b", "kind": "fact"}]"#;
        let out = parse_candidates(json).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "a");
        assert_eq!(out[1].text, "b");
    }

    // ---- dedupe and the enable gate ----

    use crate::search::NoopEmbedder;
    use std::sync::Arc;

    fn candidate(text: &str) -> Candidate {
        Candidate { text: text.into(), kind: MemoryKind::Fact, tags: vec![], confidence: 0.9 }
    }

    /// A service with no embedding model, which is what forces `dedupe` down the
    /// normalized-text path.
    fn service(db: Arc<Db>) -> MemoryService {
        MemoryService::new(db, Arc::new(NoopEmbedder)).unwrap()
    }

    fn stored(text: &str, status: MemoryStatus) -> NewMemory {
        NewMemory {
            scope: MemoryScope::Global, project_id: None, kind: MemoryKind::Fact, text: text.into(),
            tags: vec![], source_agent: None, source_tool: None, confidence: 1.0, status,
        }
    }

    #[test]
    fn normalize_text_lowercases_collapses_and_trims_punctuation() {
        assert_eq!(normalize_text("  The   Project\nuses BUN!!  "), "the project uses bun");
        assert_eq!(normalize_text("deploy target is fly.io."), "deploy target is fly.io");
    }

    /// With no embedder, a candidate that normalizes to an existing memory's text
    /// is a duplicate even though the wording differs in case and spacing.
    #[test]
    fn dedupe_falls_back_to_normalized_text_without_an_embedder() {
        let s = service(Arc::new(Db::open_in_memory().unwrap()));
        s.remember(stored("The project uses bun.", MemoryStatus::Active), "t").unwrap();
        let out = dedupe(vec![candidate("the   project uses BUN"), candidate("deploy target is fly.io")], &s, None).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "deploy target is fly.io");
    }

    /// A batch that says the same thing twice stores it once.
    #[test]
    fn dedupe_drops_repeats_within_the_batch() {
        let s = service(Arc::new(Db::open_in_memory().unwrap()));
        let out = dedupe(vec![candidate("the project uses bun"), candidate("The project uses bun!")], &s, None).unwrap();
        assert_eq!(out.len(), 1);
    }

    /// A pending memory is a proposal already awaiting review, so re-extracting it
    /// must not queue it a second time.
    #[test]
    fn dedupe_also_matches_pending_memories() {
        let s = service(Arc::new(Db::open_in_memory().unwrap()));
        s.remember(stored("the project uses bun", MemoryStatus::Pending), "t").unwrap();
        assert!(dedupe(vec![candidate("the project uses bun")], &s, None).unwrap().is_empty());
    }

    /// The daemon serves every project at once, so a sentence project A already
    /// holds must not swallow project B's version of it. Global memories still
    /// suppress a candidate anywhere, and a project's memories never suppress a
    /// global candidate.
    #[test]
    fn dedupe_is_scoped_to_the_candidate_project() {
        let s = service(Arc::new(Db::open_in_memory().unwrap()));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut in_a = stored("we deploy on fly.io", MemoryStatus::Active);
        in_a.scope = MemoryScope::Project;
        in_a.project_id = Some(a);
        s.remember(in_a, "t").unwrap();

        let fly = || vec![candidate("we deploy on fly.io")];
        assert_eq!(dedupe(fly(), &s, Some(b)).unwrap().len(), 1, "another project's wording must not suppress this one");
        assert!(dedupe(fly(), &s, Some(a)).unwrap().is_empty(), "the project's own memory does suppress it");
        assert_eq!(dedupe(fly(), &s, None).unwrap().len(), 1, "a project memory must not suppress a global candidate");

        s.remember(stored("the runtime is bun", MemoryStatus::Active), "t").unwrap();
        let bun = || vec![candidate("the runtime is bun")];
        assert!(dedupe(bun(), &s, Some(b)).unwrap().is_empty(), "a global memory suppresses a candidate in any project");
        assert!(dedupe(bun(), &s, None).unwrap().is_empty());
    }

    #[test]
    fn extraction_config_is_disabled_until_enabled_and_fully_configured() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let set = |values: serde_json::Map<String, Value>| repo.set_many(&values, "t").unwrap();
        let err = |db: &Db| extraction_config(db).unwrap_err().to_string();

        assert_eq!(err(&db), DISABLED, "off by default");
        set(serde_json::Map::from_iter([("extraction.enabled".into(), Value::from(true))]));
        assert_eq!(err(&db), DISABLED, "enabled but no base_url or model");
        set(serde_json::Map::from_iter([("extraction.base_url".into(), Value::from("http://localhost:1234/v1"))]));
        assert_eq!(err(&db), DISABLED, "still no model");

        set(serde_json::Map::from_iter([("extraction.model".into(), Value::from("local"))]));
        let cfg = extraction_config(&db).unwrap();
        assert_eq!(cfg.base_url, "http://localhost:1234/v1");
        assert_eq!(cfg.model, "local");
        assert_eq!(cfg.api_key, "", "a local endpoint needs no key");
        assert_eq!(cfg.auto_accept_min_confidence, 1.0, "nothing is auto-accepted by default");
    }

    /// The gate is a 409, not a 400: the request was fine, the daemon is not set up.
    #[test]
    fn the_disabled_error_is_a_conflict() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(extraction_config(&db).unwrap_err(), AtlasError::Conflict(_)));
    }
}
