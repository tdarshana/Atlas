mod prompt;
pub use prompt::{project_summary_prompt, EXTRACTION_SYSTEM_PROMPT};

use crate::llm::LlmClient;
use crate::models::MemoryKind;
use crate::{AtlasError, Result};
use serde::Deserialize;
use std::str::FromStr;

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
}
