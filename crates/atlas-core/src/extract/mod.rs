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
    tags: Vec<String>,
    #[serde(default)]
    confidence: Option<f64>,
}

/// Parses the model's response into candidates. Tolerates a Markdown code
/// fence around the array. Items missing `text` are skipped with a warning;
/// an unrecognized `kind` maps to `MemoryKind::Insight` with a warning.
/// A response that is not a JSON array at all is an error.
pub fn parse_candidates(json_text: &str) -> Result<Vec<Candidate>> {
    let trimmed = strip_fence(json_text.trim());
    let raw: Vec<RawCandidate> = serde_json::from_str(trimmed)
        .map_err(|e| AtlasError::Other(format!("model response was not a JSON array: {e}")))?;

    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let Some(text) = item.text else {
            tracing::warn!("extraction candidate missing text, skipping");
            continue;
        };
        let kind = item
            .kind
            .as_deref()
            .and_then(|k| MemoryKind::from_str(k).ok())
            .unwrap_or_else(|| {
                tracing::warn!(kind = item.kind.as_deref().unwrap_or(""), "unrecognized memory kind, defaulting to insight");
                MemoryKind::Insight
            });
        let confidence = item.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        out.push(Candidate { text, kind, tags: item.tags, confidence });
    }
    Ok(out)
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
}
