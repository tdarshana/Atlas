//! A minimal OpenAI-compatible chat client used by the opt-in extraction
//! step. The API key is never logged and never included in error text.

use crate::{AtlasError, Result};
use std::time::Duration;

/// Everything a chat call needs to know about which model to talk to and how. This
/// is the seam for per-case model assignment: whoever resolves a profile (see
/// `extract::resolve_model`) decides the model, and the client only carries it.
#[derive(Clone)]
pub struct ModelProfile {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// Written by hand rather than derived: a derived `Debug` would put the api key
/// into any log line or panic message that formats the profile.
impl std::fmt::Debug for ModelProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelProfile").field("base_url", &self.base_url).field("model", &self.model).finish_non_exhaustive()
    }
}

pub struct LlmClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    client: reqwest::Client,
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| AtlasError::Other(e.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            client,
        })
    }

    pub fn from_profile(profile: &ModelProfile) -> Result<Self> {
        Self::new(&profile.base_url, &profile.api_key, &profile.model)
    }

    /// Text with every occurrence of the api key replaced by `***`. An empty key
    /// (a local endpoint that wants none) redacts nothing.
    fn redact(&self, text: &str) -> String {
        if self.api_key.is_empty() { return text.to_string(); }
        text.replace(&self.api_key, "***")
    }

    pub async fn chat(&self, system: &str, user: &str) -> Result<String> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0,
        });
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AtlasError::Other(format!("model endpoint request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            // The excerpt keeps the upstream reason, which is the only clue a user has
            // when a gateway rejects the call. It is redacted first: this error is
            // logged, persisted into `jobs.error` and served back by `GET /jobs/{id}`,
            // and an endpoint that quotes the failing request would otherwise put the
            // key in all three. Redacting before the 200-char cut also stops a key
            // straddling the boundary from surviving in half.
            let excerpt: String = self.redact(&text).chars().take(200).collect();
            // An endpoint that answers with an empty body leaves nothing to quote, and a
            // message ending in a bare `: ` reads as truncated rather than as "no reason
            // was given".
            let reason = if excerpt.trim().is_empty() { String::new() } else { format!(": {excerpt}") };
            return Err(AtlasError::Other(format!(
                "model endpoint returned {status}{reason}"
            )));
        }

        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AtlasError::Other(format!("model endpoint returned invalid JSON: {e}")))?;
        value["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| AtlasError::Other("model endpoint response missing choices[0].message.content".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Json, http::StatusCode, routing::post, Router};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    async fn spawn(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn chat_sends_expected_request_and_parses_response() {
        let captured: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let captured2 = captured.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let captured = captured2.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    *captured.lock().unwrap() = Some(json!({"body": body, "auth": auth}));
                    Json(json!({"choices": [{"message": {"content": "hello from model"}}]}))
                }
            }),
        );
        let base = spawn(app).await;

        let client = LlmClient::new(&base, "secret-key-123", "gpt-test").unwrap();
        let out = client.chat("sys prompt", "user prompt").await.unwrap();
        assert_eq!(out, "hello from model");

        let captured = captured.lock().unwrap().clone().unwrap();
        assert_eq!(captured["body"]["model"], "gpt-test");
        assert_eq!(captured["body"]["temperature"], 0);
        let messages = captured["body"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "sys prompt");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "user prompt");
        assert_eq!(captured["auth"], "Bearer secret-key-123");
    }

    #[tokio::test]
    async fn chat_error_does_not_leak_api_key() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "the model endpoint is down") }),
        );
        let base = spawn(app).await;

        let client = LlmClient::new(&base, "super-secret-999", "gpt-test").unwrap();
        let err = client.chat("s", "u").await.unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("super-secret-999"), "error text leaked the api key: {msg}");
        assert!(msg.contains("500"), "expected status in error text: {msg}");
    }

    /// A gateway that quotes the offending credential back at us must not get the key
    /// into the error, which is logged and stored in `jobs.error`.
    #[tokio::test]
    async fn chat_error_redacts_a_key_the_endpoint_echoes_back() {
        let app = Router::new().route(
            "/chat/completions",
            post(|headers: axum::http::HeaderMap| async move {
                let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                (StatusCode::UNAUTHORIZED, format!("{{\"error\":\"invalid api key: {auth}\"}}"))
            }),
        );
        let base = spawn(app).await;

        let client = LlmClient::new(&base, "sk-live-abc123", "gpt-test").unwrap();
        let msg = client.chat("s", "u").await.unwrap_err().to_string();
        assert!(!msg.contains("sk-live-abc123"), "the echoed key survived into the error: {msg}");
        assert!(msg.contains("***"), "the excerpt should keep the upstream reason, redacted: {msg}");
        assert!(msg.contains("401"), "expected status in error text: {msg}");
    }

    /// An empty key is not a substring to redact; an empty-needle `replace` would
    /// otherwise splice `***` between every character of the body.
    #[tokio::test]
    async fn an_empty_key_leaves_the_excerpt_alone() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async { (StatusCode::BAD_REQUEST, "model 'x' not found") }),
        );
        let base = spawn(app).await;
        let msg = LlmClient::new(&base, "", "x").unwrap().chat("s", "u").await.unwrap_err().to_string();
        assert!(msg.contains("model 'x' not found"), "{msg}");
        assert!(!msg.contains("***"), "{msg}");
    }
}
