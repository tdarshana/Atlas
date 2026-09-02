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

/// Reads the extraction settings, or reports the feature as disabled. Extraction
/// is off by default and stays off unless `extraction.enabled` is true and both
/// `extraction.base_url` and `extraction.model` are set; the api key may be empty,
/// since a local endpoint does not ask for one.
///
/// The values come from `SettingsRepo::get_raw`, not `get_all`: `get_all` masks
/// `extraction.api_key` to `"***"`, which is not a key the worker could use. The
/// key is never logged and never put in an error.
pub fn extraction_config(db: &Db) -> Result<ExtractionConfig> {
    let settings = SettingsRepo::new(db);
    let disabled = || AtlasError::Conflict(DISABLED.to_string());
    if settings.get_raw("extraction.enabled")?.and_then(|v| v.as_bool()) != Some(true) {
        return Err(disabled());
    }
    let text = |key: &str| -> Result<String> {
        settings.get_raw(key)?
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(disabled)
    };
    Ok(ExtractionConfig {
        base_url: text("extraction.base_url")?,
        model: text("extraction.model")?,
        api_key: settings.get_raw("extraction.api_key")?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        // Defaulting to 1.0 means nothing is auto-accepted until the user lowers it.
        auto_accept_min_confidence: settings
            .get_raw("extraction.auto_accept_min_confidence")?
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0),
    })
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
    #[serde(default)]
    confidence: Option<f64>,
}

/// Parses the model's response into candidates. Tolerates a Markdown code
/// fence around the array. The array is deserialized one item at a time so a
/// single malformed item (not an object, or a `tags` that isn't an array)
/// cannot sink the whole batch: such items are skipped with a warning, as
/// are items missing `text`. An unrecognized `kind` maps to
/// `MemoryKind::Insight` with a warning. A response that is not a JSON array
/// at all is an error.
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
        let kind = raw
            .kind
            .as_deref()
            .and_then(|k| MemoryKind::from_str(k).ok())
            .unwrap_or_else(|| {
                tracing::warn!(kind = raw.kind.as_deref().unwrap_or(""), "unrecognized memory kind, defaulting to insight");
                MemoryKind::Insight
            });
        let confidence = raw.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        out.push(Candidate { text, kind, tags: parse_tags(raw.tags), confidence });
    }
    Ok(out)
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
/// Both `run_ingest` and `Backend::test_extraction` go through this rather than
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
    let cfg = extraction_config(&backend.db)?;
    let text = job.payload["text"].as_str().ok_or_else(|| AtlasError::Invalid("ingest job has no text".into()))?;
    let source_tool = job.payload["source_tool"].as_str().unwrap_or("ingest").to_string();
    let root = job.payload["project_root"].as_str().map(PathBuf::from);

    let client = build_client(&cfg)?;
    let candidates = extract_candidates(text, &client).await?;
    let found = candidates.len();

    let project_id = project_for(&backend.db, root)?;
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
                // Everything waits for review unless the user has lowered the bar
                // far enough that this candidate clears it.
                status: if c.confidence >= cfg.auto_accept_min_confidence { MemoryStatus::Active } else { MemoryStatus::Pending },
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

    let cfg = extraction_config(&backend.db)?;
    let client = build_client(&cfg)?;

    let repo = ProjectRepo::new(&backend.db);
    let project = repo.get(project_id)?;
    let mut profile = project
        .profile
        .ok_or_else(|| AtlasError::Invalid(format!("project {project_id} has no profile to summarize")))?;

    let summary = summarize_project(&profile, &client).await?;
    let chars = summary.chars().count();
    profile.summary = Some(summary);
    repo.set_profile(project_id, &profile, EXTRACTOR)?;
    Ok(serde_json::json!({"chars": chars}))
}

#[cfg(test)]
mod tests {
    use super::*;

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
