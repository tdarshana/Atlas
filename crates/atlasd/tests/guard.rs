//! The token and loopback guard (SEC-5, the browser checks).

mod common;
use common::*;

/// SEC-5: the daemon and its clients share a secret. Every `/api/v1` route wants it in
/// `X-Atlas-Token`; a missing or wrong one is 401 and never reaches a handler. `status`
/// answers with the token's SHA-256 so a client can tell atlasd from an impostor on the
/// port, and the two routes a browser cannot set headers on (`events`, the plugin
/// channel) take `?token=` instead. `/mcp` is outside `/api/v1` and is not gated here.
#[tokio::test]
async fn every_api_route_requires_the_daemon_token() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let bare = reqwest::Client::new();

    let missing = bare.get(format!("{base}/status")).send().await.unwrap();
    assert_eq!(missing.status(), 401);
    assert_eq!(missing.json::<serde_json::Value>().await.unwrap(), serde_json::json!({"error": "unauthorized"}));
    let wrong = bare.get(format!("{base}/status")).header(TOKEN_HEADER, "not-the-token").send().await.unwrap();
    assert_eq!(wrong.status(), 401);
    // A write route is gated the same way, before any handler runs.
    let write = bare.post(format!("{base}/memories")).json(&serde_json::json!({"scope":"global","kind":"fact","text":"x"})).send().await.unwrap();
    assert_eq!(write.status(), 401);

    let ok = bare.get(format!("{base}/status")).header(TOKEN_HEADER, &d.token).send().await.unwrap();
    assert_eq!(ok.status(), 200);
    let st: serde_json::Value = ok.json().await.unwrap();
    assert_eq!(st["token_sha256"], sha256_hex(&d.token), "status proves the daemon holds the token");

    let stream = bare.get(format!("{base}/events?token={}", d.token)).send().await.unwrap();
    assert_eq!(stream.status(), 200, "events takes the token as a query parameter");
    assert_eq!(bare.get(format!("{base}/events?token=wrong")).send().await.unwrap().status(), 401);
    assert_eq!(bare.get(format!("{base}/events")).send().await.unwrap().status(), 401);
    // The query form is for those two routes only.
    assert_eq!(bare.get(format!("{base}/status?token={}", d.token)).send().await.unwrap().status(), 401);

    // The token is not a CORS-safelisted header, so a preflight carries none; it must
    // still be answered or the webview could never send the real request.
    let preflight = bare.request(reqwest::Method::OPTIONS, format!("{base}/memories"))
        .header("Origin", "tauri://localhost").header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "x-atlas-token,content-type").send().await.unwrap();
    assert_eq!(preflight.status(), 200);
    let allowed = preflight.headers().get("access-control-allow-headers").unwrap().to_str().unwrap().to_ascii_lowercase();
    assert!(allowed.contains("x-atlas-token"), "allow-headers: {allowed}");

    // The file that carries the token is private to the user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.home.path().join("daemon.json")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "daemon.json mode {mode:o}");
    }
}

/// The daemon has no authentication, so a page open in the user's browser must not be able
/// to reach it, on the JSON API or on /mcp.
#[tokio::test]
async fn rejects_browser_origins_and_non_loopback_hosts() {
    let d = start().await;
    let status = format!("http://127.0.0.1:{}/api/v1/status", d.port);
    let c = d.client();

    let cross = c.get(&status).header("Origin", "https://evil.example").send().await.unwrap();
    assert_eq!(cross.status(), 403);
    let body: serde_json::Value = cross.json().await.unwrap();
    assert_eq!(body["error"], "forbidden origin");

    let rebound = c.get(&status).header("Host", "evil.example").send().await.unwrap();
    assert_eq!(rebound.status(), 403);

    let mcp = c.post(format!("http://127.0.0.1:{}/mcp", d.port)).header("Origin", "https://evil.example")
        .header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}))
        .send().await.unwrap();
    assert_eq!(mcp.status(), 403, "the guard must cover /mcp too");

    for origin in ["http://127.0.0.1", &format!("http://localhost:{}", d.port)] {
        assert!(c.get(&status).header("Origin", origin).send().await.unwrap().status().is_success(), "{origin} should be allowed");
    }
}

/// SEC-2 (ATL-296). A page on another loopback port can submit a plain HTML form at a
/// bodiless side-effect route: the browser sends it with no preflight, and CORS only keeps
/// the page from reading the answer, not the request from running. Such an origin may use
/// safe methods only (the request above still passes); any other method from it is
/// refused before a handler runs. The app's own origins and clients that send no
/// `Origin` reach the handler as before.
#[tokio::test]
async fn a_form_post_from_a_loopback_page_outside_the_app_is_refused() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let refresh = format!("{base}/projects/{}/refresh", uuid::Uuid::new_v4());
    let c = d.client();

    let form = c.post(&refresh).header("Origin", "http://localhost:3000")
        .header("Content-Type", "application/x-www-form-urlencoded").body("a=1").send().await.unwrap();
    assert_eq!(form.status(), 403, "a simple form post from another loopback page must not run");
    let body: serde_json::Value = form.json().await.unwrap();
    assert_eq!(body["error"], "forbidden origin");

    let get = c.get(format!("{base}/status")).header("Origin", "http://localhost:3000").send().await.unwrap();
    assert_eq!(get.status(), 200, "a safe method from that page still passes the guard");

    // The webview and a non-browser client get past the guard to the handler, which
    // answers 404 for a project that does not exist.
    let webview = c.post(&refresh).header("Origin", "tauri://localhost").send().await.unwrap();
    assert_eq!(webview.status(), 404, "the app's own origin reaches the handler");
    let cli = c.post(&refresh).send().await.unwrap();
    assert_eq!(cli.status(), 404, "a client with no Origin reaches the handler");
}

/// The Tauri desktop app's webview sends one of these fixed origins depending on
/// platform (WKWebView/wry: `tauri://localhost`; WebView2: `http://tauri.localhost`),
/// plus its dev server origin `http://localhost:1420`. None of them may be rejected as
/// a browser origin.
#[tokio::test]
async fn accepts_tauri_webview_origins() {
    let d = start().await;
    let status = format!("http://127.0.0.1:{}/api/v1/status", d.port);
    let c = d.client();
    for origin in ["tauri://localhost", "http://tauri.localhost", "http://localhost:1420"] {
        let r = c.get(&status).header("Origin", origin).send().await.unwrap();
        assert_eq!(r.status(), 200, "{origin} should be allowed");
    }
}

/// The daemon never authenticates, so a browser or the Tauri webview only gets to read the
/// response if `Access-Control-Allow-Origin` echoes an allowed origin. The CORS allow list is
/// narrower than the request guard: a page on another loopback port has its request run (200)
/// but gets no allow-origin header, so the browser will not hand it the body; a non-loopback
/// origin never gets past the guard at all.
#[tokio::test]
async fn cors_headers_cover_allowed_and_reject_other_origins() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let ok = c.get(format!("{base}/status")).header("Origin", "http://localhost:1420").send().await.unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.headers().get("access-control-allow-origin").unwrap(), "http://localhost:1420");
    let vary = ok.headers().get("vary").unwrap_or_else(|| panic!("no vary header")).to_str().unwrap().to_ascii_lowercase();
    assert!(vary.contains("origin"), "{vary}");

    let preflight = c.request(reqwest::Method::OPTIONS, format!("{base}/memories/search"))
        .header("Origin", "tauri://localhost")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type, x-atlas-actor")
        .send().await.unwrap();
    assert!(preflight.status().is_success(), "{}", preflight.status());
    assert_eq!(preflight.headers().get("access-control-allow-origin").unwrap(), "tauri://localhost");
    let allow_headers = preflight.headers().get("access-control-allow-headers").unwrap_or_else(|| panic!("no access-control-allow-headers")).to_str().unwrap().to_ascii_lowercase();
    assert!(allow_headers.contains("content-type"), "{allow_headers}");
    assert!(allow_headers.contains("x-atlas-actor"), "the board actor header must pass preflight: {allow_headers}");

    // A local page on another port is allowed through the guard, but must not be able to
    // read what came back: the request runs, the allow-origin header is absent.
    let other_port = c.get(format!("{base}/status")).header("Origin", "http://localhost:3000").send().await.unwrap();
    assert_eq!(other_port.status(), 200);
    assert!(other_port.headers().get("access-control-allow-origin").is_none(), "{:?}", other_port.headers());

    let evil = c.get(format!("{base}/status")).header("Origin", "https://evil.example").send().await.unwrap();
    assert_eq!(evil.status(), 403);
    assert!(evil.headers().get("access-control-allow-origin").is_none(), "{:?}", evil.headers());

    let bare = c.get(format!("{base}/status")).send().await.unwrap();
    assert_eq!(bare.status(), 200);
    assert!(bare.headers().get("access-control-allow-origin").is_none(), "{:?}", bare.headers());
}
