//! Settings and the extraction configuration.

mod common;
use common::*;

/// `extraction.api_key` is masked on every GET and PUT response, a masked round trip
/// leaves the real key untouched, and an unknown key is rejected with 400.
#[tokio::test]
async fn settings_api_masks_the_key_and_validates_keys() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let put: serde_json::Value = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.api_key": "sk-test", "extraction.model": "deepseek-chat",
    })).send().await.unwrap().json().await.unwrap();
    assert_eq!(put["extraction.api_key"], "***", "{put}");
    assert_eq!(put["extraction.enabled"], true);
    assert_eq!(put["extraction.model"], "deepseek-chat");

    let get: serde_json::Value = c.get(format!("{base}/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(get["extraction.api_key"], "***", "{get}");
    assert_eq!(get["extraction.enabled"], true);

    // A masked round trip (the shape a GET->PUT client sends back) must not clobber the
    // real key: it stays masked and non-empty after another PUT that doesn't touch it.
    let put2: serde_json::Value = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.model": "x", "extraction.api_key": "***",
    })).send().await.unwrap().json().await.unwrap();
    assert_eq!(put2["extraction.api_key"], "***", "{put2}");
    assert_ne!(put2["extraction.api_key"], "", "{put2}");
    assert_eq!(put2["extraction.model"], "x");

    let bad = c.put(format!("{base}/settings")).json(&serde_json::json!({"bogus": 1})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let body: serde_json::Value = bad.json().await.unwrap();
    assert!(body["error"].as_str().is_some(), "{body}");
}

/// `POST /extraction/test` gates on the same settings `POST /ingest` does (409 while
/// disabled or unconfigured), sends the connectivity check to the configured model
/// when it is, and reports a model-endpoint error as 400 rather than 500.
#[tokio::test]
async fn extraction_test_endpoint_checks_connectivity_and_reports_model_errors() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let disabled = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(disabled.status(), 409);
    let body: serde_json::Value = disabled.json().await.unwrap();
    assert_eq!(body["error"], "extraction is disabled", "{body}");

    let stub = stub_llm_with_reply("OK").await;
    let put = c.put(format!("{base}/settings")).json(&serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": stub, "extraction.model": "stub", "extraction.api_key": "k",
    })).send().await.unwrap();
    assert_eq!(put.status(), 200);

    let ok = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(ok.status(), 200);
    let body: serde_json::Value = ok.json().await.unwrap();
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["reply"], "OK", "{body}");

    // Point at an endpoint that will refuse the connection: a model-endpoint error
    // is a 400 with ok:false, not a 500.
    let put2 = c.put(format!("{base}/settings")).json(&serde_json::json!({"extraction.base_url": "http://127.0.0.1:1"})).send().await.unwrap();
    assert_eq!(put2.status(), 200);
    let failed = c.post(format!("{base}/extraction/test")).send().await.unwrap();
    assert_eq!(failed.status(), 400);
    let body: serde_json::Value = failed.json().await.unwrap();
    assert_eq!(body["ok"], false, "{body}");
    assert!(body["error"].as_str().is_some(), "{body}");
}

/// The daemon has no authentication of its own, so any process that can reach the
/// loopback port could otherwise repoint `extraction.base_url` and then have the
/// daemon send the stored key to a host of its choosing. Moving the endpoint without
/// supplying a new key drops the stored one, and the PUT says so.
#[tokio::test]
async fn changing_the_base_url_clears_the_stored_key() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let put = |body: serde_json::Value| c.put(format!("{base}/settings")).json(&body).send();

    let configured: serde_json::Value = put(serde_json::json!({
        "extraction.enabled": true, "extraction.base_url": "https://api.deepseek.com", "extraction.model": "m", "extraction.api_key": "sk-real",
    })).await.unwrap().json().await.unwrap();
    assert_eq!(configured["extraction.api_key"], "***", "{configured}");
    assert!(configured["extraction.api_key_cleared"].is_null(), "a key entered with its endpoint is kept: {configured}");

    // The same endpoint again, trailing slash and all, is not a move.
    let same: serde_json::Value = put(serde_json::json!({"extraction.base_url": "https://api.deepseek.com/"})).await.unwrap().json().await.unwrap();
    assert!(same["extraction.api_key_cleared"].is_null(), "{same}");
    assert_eq!(same["extraction.api_key"], "***", "{same}");

    // Somewhere new, with no key of its own: the old key does not follow it there.
    let moved: serde_json::Value = put(serde_json::json!({"extraction.base_url": "http://attacker.example/v1"})).await.unwrap().json().await.unwrap();
    assert_eq!(moved["extraction.api_key_cleared"], true, "{moved}");
    assert_eq!(moved["extraction.api_key"], "", "the masked key is gone because there is no key: {moved}");
}
