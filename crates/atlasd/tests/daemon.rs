//! The daemon process: load, the change stream, shutdown and the route table.

mod common;
use common::*;
use std::process::Command;
use std::time::Duration;

// ---- the route table (ARCH-6) ----

#[path = "../src/routes.rs"]
#[allow(dead_code)]
mod routes;

/// Phase 14: DuckDB (and, with an embedder loaded, ONNX) work runs on blocking
/// threads via `LocalBackend::blocking`, so the HTTP server keeps answering while a
/// large write runs. Fires 20 concurrent `/memories/search` requests alongside a
/// 200-row `/memories` batch and asserts every `/status` probe taken while the batch
/// is in flight answers within 500 ms; before this change the batch's DuckDB writes
/// ran directly on the async runtime's worker threads and could starve `/status`.
#[tokio::test]
async fn status_stays_responsive_under_concurrent_load() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    // Seed a few memories so the concurrent searches below have something to score.
    for i in 0..20 {
        let r = c.post(format!("{base}/memories"))
            .json(&serde_json::json!({"scope":"global","kind":"fact","text": format!("seed memory {i} about bun and duckdb")}))
            .send().await.unwrap();
        assert_eq!(r.status(), 201);
    }

    // A large batch of writes, running in the background while the probes below run.
    let write_base = base.clone();
    let write_client = d.client();
    let writer = tokio::spawn(async move {
        let c = write_client;
        for i in 0..200 {
            let r = c.post(format!("{write_base}/memories"))
                .json(&serde_json::json!({"scope":"global","kind":"fact","text": format!("bulk memory {i} about the atlas daemon and its duckdb file")}))
                .send().await.unwrap();
            assert_eq!(r.status(), 201);
        }
    });

    // 20 concurrent searches, running alongside the batch above.
    let mut searchers = Vec::new();
    for _ in 0..20 {
        let search_base = base.clone();
        let search_client = d.client();
        searchers.push(tokio::spawn(async move {
            let c = search_client;
            let r = c.post(format!("{search_base}/memories/search")).json(&serde_json::json!({"query":"bun duckdb"})).send().await.unwrap();
            assert_eq!(r.status(), 200);
        }));
    }

    // Probe /status while the batch and the searches are in flight, and record the
    // worst latency seen. At least 5 probes run regardless of how fast the batch
    // finishes, so the assertion below is never skipped by a vacuous loop.
    let mut worst = Duration::from_millis(0);
    let mut probes = 0usize;
    loop {
        let started = std::time::Instant::now();
        let r = c.get(format!("{base}/status")).send().await.unwrap();
        let elapsed = started.elapsed();
        assert_eq!(r.status(), 200);
        worst = worst.max(elapsed);
        assert!(elapsed < Duration::from_millis(500), "a /status probe took {elapsed:?} while a 200-row batch and 20 concurrent searches were running");
        probes += 1;
        if writer.is_finished() && probes >= 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    writer.await.unwrap();
    for s in searchers {
        s.await.unwrap();
    }

    eprintln!("worst /status latency under concurrent load ({probes} probes): {worst:?}");
}

// ---- change stream (live sync) ----

/// `GET /api/v1/events` announces a write as it lands: a task created over the JSON
/// API arrives on an open stream as a `task` event carrying the key, before any poll.
#[tokio::test]
async fn the_event_stream_announces_a_task_write() {
    use futures_util::StreamExt;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let stream = c.get(format!("{base}/events")).send().await.unwrap();
    assert_eq!(stream.status(), 200);
    assert!(stream.headers().get("content-type").unwrap().to_str().unwrap().starts_with("text/event-stream"));
    let mut body = stream.bytes_stream();

    let created: serde_json::Value = c
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({"title": "announce me"}))
        .send().await.unwrap().json().await.unwrap();
    let key = created["key"].as_str().unwrap().to_string();

    let mut seen = String::new();
    let heard = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = body.next().await {
            seen.push_str(&String::from_utf8_lossy(&chunk.unwrap()));
            if seen.contains("event: task") && seen.contains(&format!("\"key\":\"{key}\"")) { return true; }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(heard, "no task event for {key} on the stream; saw: {seen}");
    assert!(seen.contains("\"action\":\"created\""), "{seen}");
}

// ---- clean stop (ATL-345) ----

/// A terminate signal stops the daemon within seconds even while a change stream is
/// open, the stop checkpoints the write-ahead log into the database file, and the
/// file reopens with everything that was written. This is the path `atlas daemon stop`
/// and the desktop's own stop take; without it an unclean stop left a log DuckDB could
/// not replay.
#[tokio::test]
async fn a_terminate_signal_stops_the_daemon_and_checkpoints_the_log() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    // A change stream that never hangs up on its own.
    let stream = c.get(format!("{base}/events")).send().await.unwrap();
    assert_eq!(stream.status(), 200);
    let created: serde_json::Value = c
        .post(format!("{base}/tasks"))
        .json(&serde_json::json!({"title": "survives the stop"}))
        .send().await.unwrap().json().await.unwrap();
    let key = created["key"].as_str().unwrap().to_string();

    let pid = d.child.id();
    let status = Command::new("kill").arg(pid.to_string()).status().unwrap();
    assert!(status.success());
    let mut d = d;
    let mut exited = None;
    for _ in 0..100 {
        if let Some(s) = d.child.try_wait().unwrap() { exited = Some(s); break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let exited = exited.expect("the daemon exited within ten seconds of SIGTERM");
    assert!(exited.success(), "clean exit: {exited:?}");
    drop(stream);

    let home = d.home.path().to_path_buf();
    let db_path = home.join("atlas.duckdb");
    let wal = std::fs::metadata(home.join("atlas.duckdb.wal")).map(|m| m.len()).unwrap_or(0);
    assert_eq!(wal, 0, "the stop folded the log into the file");
    assert!(!home.join("daemon.json").exists(), "daemon.json is taken down");
    let db = atlas_core::db::Db::open(&db_path).unwrap();
    let found: i64 = db
        .with_conn(|c| Ok(c.query_row("select count(*) from tasks where key = ?", [&key], |r| r.get(0))?))
        .unwrap();
    assert_eq!(found, 1, "{key} survived the stop and reopen");
}

/// Every entry of `routes::ROUTES` is served by the running daemon, and the router mounts
/// nothing the table lacks. A route dropped from the router answers the axum fallback
/// (a 404 with an empty body); one dropped from the table is caught by the count below.
/// A handler's own 404 (`{"error": ..}` for the nil uuid) and 400 (no body sent) both
/// mean the route is there, so they pass.
#[tokio::test]
async fn every_route_in_the_table_is_served() {
    let d = start().await;
    let c = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let nil = uuid::Uuid::nil().to_string();
    let mut failures = Vec::new();
    for r in routes::ROUTES {
        let mut path = String::new();
        for (i, seg) in r.path.split('/').enumerate() {
            if i > 0 { path.push('/'); }
            let filled = match seg {
                "{id}" | "{*id}" => nil.as_str(),
                "{id_or_key}" => "ATL-1",
                "{kind}" => "superpowers",
                s if s.starts_with('{') => "x",
                s => s,
            };
            path.push_str(filled);
        }
        let method = reqwest::Method::from_bytes(r.method.as_bytes()).unwrap();
        let resp = c.request(method, format!("http://127.0.0.1:{}{path}", d.port)).send().await.unwrap();
        let status = resp.status().as_u16();
        let served = match status {
            405 => false,
            404 => {
                let body = resp.text().await.unwrap_or_default();
                serde_json::from_str::<serde_json::Value>(&body).ok().is_some_and(|v| v.get("error").is_some())
            }
            _ => true,
        };
        if !served {
            failures.push(format!("{} {} ({}) answered {status}", r.method, r.path, r.name));
        }
    }
    assert!(failures.is_empty(), "routes in the table the daemon does not serve:\n{}", failures.join("\n"));

    // The other direction: every `.route(` line of `http.rs` mounts a path the table has,
    // with as many methods as the table lists for it.
    let http = include_str!("../src/http.rs");
    let mut missing = Vec::new();
    // A line marked `// alias` mounts an old path onto a listed route's handlers and is
    // deliberately absent from the table, so the generated client has one name per route.
    for line in http.lines().map(str::trim_start).filter(|l| l.starts_with(".route(\"/api/v1") && !l.contains("// alias")) {
        let path = line.split('"').nth(1).unwrap();
        let mounted = ["get(", "post(", "put(", "patch(", "delete("].iter().filter(|m| line.contains(*m)).count();
        let listed = routes::ROUTES.iter().filter(|r| r.path == path).count();
        if listed != mounted {
            missing.push(format!("{path}: router mounts {mounted} method(s), the table lists {listed}"));
        }
    }
    assert!(missing.is_empty(), "routes the table does not match:\n{}", missing.join("\n"));
}
