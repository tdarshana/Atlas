//! A minimal OpenAI-compatible chat client used by the opt-in extraction
//! step. The API key is never logged and never included in error text.

use crate::{AtlasError, Result};
use std::time::Duration;

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
            let excerpt: String = text.chars().take(200).collect();
            return Err(AtlasError::Other(format!(
                "model endpoint returned {status}: {excerpt}"
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
}
