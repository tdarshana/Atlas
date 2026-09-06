//! Agents (the persona rows): library, roster, task attribution and the persona header.

mod common;
use common::*;
use std::time::Duration;

// ---- personas (Phase 17) ----

/// Every persona route round-trips: the library (create with a derived slug, list, get
/// by slug, update, delete), the bundle with a warning per missing reference, the
/// project roster (whole-list replace, one default, in `position` order), the persona
/// on a task (create, patch, filter, clear on `""`), and the change stream carrying
/// `persona` events for library writes and `project` events for roster writes.
#[tokio::test]
async fn personas_routes_roster_task_persona_and_events() {
    use futures_util::StreamExt;
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    let stream = c.get(format!("{base}/events")).send().await.unwrap();
    let mut body = stream.bytes_stream();

    // Create: 201, slug derived, defaults filled.
    let r = c.post(format!("{base}/personas")).header("X-Atlas-Actor", "user")
        .json(&serde_json::json!({"name": "Mobile Developer", "role": "Builds the app", "skills": ["plugin:gone/gone/gone"], "models": {"implement": "sonnet"}, "access": {"memory_write": "review"}}))
        .send().await.unwrap();
    assert_eq!(r.status(), 201);
    let mobile: serde_json::Value = r.json().await.unwrap();
    assert_eq!(mobile["slug"], "mobile-developer");
    assert_eq!(mobile["access"]["task_move"], "allow");
    assert_eq!(mobile["models"]["implement"], "sonnet");
    let mobile_id = mobile["id"].as_str().unwrap().to_string();
    // A second name that only differs in case is a conflict; a bad rule is 400.
    let r = c.post(format!("{base}/personas")).json(&serde_json::json!({"name": "mobile developer"})).send().await.unwrap();
    assert_eq!(r.status(), 409);
    let r = c.post(format!("{base}/personas")).json(&serde_json::json!({"name": "Ops", "access": {"task_move": "review"}})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let reviewer: serde_json::Value = c.post(format!("{base}/personas")).json(&serde_json::json!({"name": "Security Reviewer"})).send().await.unwrap().json().await.unwrap();
    let reviewer_id = reviewer["id"].as_str().unwrap().to_string();

    // List, get by slug and by id, 404 for a stranger.
    let list: serde_json::Value = c.get(format!("{base}/personas")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2);
    let by_slug: serde_json::Value = c.get(format!("{base}/personas/mobile-developer")).send().await.unwrap().json().await.unwrap();
    assert_eq!(by_slug["id"], mobile_id);
    assert_eq!(c.get(format!("{base}/personas/nobody")).send().await.unwrap().status(), 404);

    // Update: fields change, the slug follows the name.
    let r = c.put(format!("{base}/personas/{mobile_id}")).header("X-Atlas-Actor", "user")
        .json(&serde_json::json!({"name": "Android Developer", "tools": ["task_list"]})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let updated: serde_json::Value = r.json().await.unwrap();
    assert_eq!((updated["slug"].as_str(), updated["role"].as_str()), (Some("android-developer"), Some("Builds the app")));
    assert_eq!(updated["tools"], serde_json::json!(["task_list"]));

    // The bundle resolves what it can and warns about the rest.
    let bundle: serde_json::Value = c.get(format!("{base}/personas/android-developer/bundle")).send().await.unwrap().json().await.unwrap();
    assert_eq!(bundle["persona"]["id"], mobile_id);
    assert!(bundle["skills"].as_array().unwrap().is_empty());
    let warnings = bundle["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w == "skill plugin:gone/gone/gone not found"), "{warnings:?}");

    // The roster: two defaults are refused, then a valid list reads back in position order.
    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let pid = p["id"].as_str().unwrap().to_string();
    let empty: serde_json::Value = c.get(format!("{base}/projects/{pid}/personas")).send().await.unwrap().json().await.unwrap();
    assert_eq!(empty, serde_json::json!([]));
    let two_defaults = serde_json::json!([
        {"persona_id": mobile_id, "is_default": true, "position": 0},
        {"persona_id": reviewer_id, "is_default": true, "position": 1},
    ]);
    let r = c.put(format!("{base}/projects/{pid}/personas")).header("X-Atlas-Actor", "user").json(&two_defaults).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let entries = serde_json::json!([
        {"persona_id": mobile_id, "is_default": false, "position": 2},
        {"persona_id": reviewer_id, "is_default": true, "position": 1},
    ]);
    let r = c.put(format!("{base}/projects/{pid}/personas")).header("X-Atlas-Actor", "user").json(&entries).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let roster: serde_json::Value = c.get(format!("{base}/projects/{pid}/personas")).send().await.unwrap().json().await.unwrap();
    let rows = roster.as_array().unwrap();
    assert_eq!(rows.iter().map(|r| r["slug"].as_str().unwrap()).collect::<Vec<_>>(), ["security-reviewer", "android-developer"]);
    assert_eq!((rows[0]["is_default"].as_bool(), rows[1]["is_default"].as_bool()), (Some(true), Some(false)));
    assert_eq!(rows[0]["project_id"], pid);

    // A task carries its persona: by slug on create, cleared with "" on patch, and the
    // list narrows by `?persona=`.
    let r = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "user")
        .json(&serde_json::json!({"project_id": pid, "title": "ship", "persona": "android-developer"})).send().await.unwrap();
    assert_eq!(r.status(), 201);
    let task: serde_json::Value = r.json().await.unwrap();
    assert_eq!((task["persona_id"].as_str(), task["persona_name"].as_str(), task["persona_slug"].as_str()), (Some(mobile_id.as_str()), Some("Android Developer"), Some("android-developer")));
    let key = task["key"].as_str().unwrap().to_string();
    let r = c.post(format!("{base}/tasks")).json(&serde_json::json!({"project_id": pid, "title": "x", "persona": "nobody"})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let plain: serde_json::Value = c.post(format!("{base}/tasks")).json(&serde_json::json!({"project_id": pid, "title": "plain"})).send().await.unwrap().json().await.unwrap();
    assert!(plain["persona_id"].is_null());
    let narrowed: serde_json::Value = c.get(format!("{base}/tasks?project_id={pid}&persona=android-developer")).send().await.unwrap().json().await.unwrap();
    assert_eq!(narrowed.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect::<Vec<_>>(), [key.as_str()]);
    let patched: serde_json::Value = c.patch(format!("{base}/tasks/{key}")).json(&serde_json::json!({"persona": ""})).send().await.unwrap().json().await.unwrap();
    assert!(patched["persona_id"].is_null() && patched["persona_slug"].is_null(), "{patched}");
    let patched: serde_json::Value = c.patch(format!("{base}/tasks/{key}")).json(&serde_json::json!({"persona": reviewer_id})).send().await.unwrap().json().await.unwrap();
    assert_eq!(patched["persona_slug"], "security-reviewer");

    // Delete: 204, the roster row and the task's persona go with it.
    let r = c.delete(format!("{base}/personas/{reviewer_id}")).header("X-Atlas-Actor", "user").send().await.unwrap();
    assert_eq!(r.status(), 204);
    assert_eq!(c.get(format!("{base}/personas/{reviewer_id}")).send().await.unwrap().status(), 404);
    let roster: serde_json::Value = c.get(format!("{base}/projects/{pid}/personas")).send().await.unwrap().json().await.unwrap();
    assert_eq!(roster.as_array().unwrap().len(), 1);
    let detail: serde_json::Value = c.get(format!("{base}/tasks/{key}")).send().await.unwrap().json().await.unwrap();
    assert!(detail["task"]["persona_id"].is_null(), "{detail}");

    // The stream heard every write: the library as `persona`, the roster as `project`.
    let mut seen = String::new();
    let heard = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = body.next().await {
            seen.push_str(&String::from_utf8_lossy(&chunk.unwrap()));
            if seen.contains("\"entity\":\"persona\",\"action\":\"delete\"") { return true; }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(heard, "no persona delete event on the stream; saw: {seen}");
    for needle in ["event: persona", "\"action\":\"create\"", "\"action\":\"update\"", "\"action\":\"assign\"", "\"action\":\"set_default\"", "\"entity\":\"project\",\"action\":\"roster\""] {
        assert!(seen.contains(needle), "{needle} missing from: {seen}");
    }
}

/// `X-Atlas-Persona` binds a write to a persona: a `review` persona lands an agent's
/// memory pending, a `deny` persona refuses a task move, an unknown slug is 400, and
/// an allowed move's event detail names the slug while the actor label does not.
#[tokio::test]
async fn a_persona_header_gates_writes_and_lands_in_the_detail() {
    let d = start().await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();
    for (name, access) in [("Careful", serde_json::json!({"memory_write": "review"})), ("Locked", serde_json::json!({"task_move": "deny"})), ("Open", serde_json::json!({}))] {
        let r = c.post(format!("{base}/personas")).json(&serde_json::json!({"name": name, "access": access})).send().await.unwrap();
        assert_eq!(r.status(), 201, "{name}");
    }

    // An unknown slug is refused before anything is written; a name is not a slug.
    let r = c.post(format!("{base}/memories?actor=codex")).header("X-Atlas-Persona", "nobody")
        .json(&serde_json::json!({"scope": "global", "kind": "fact", "text": "never lands"})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let e: serde_json::Value = r.json().await.unwrap();
    assert_eq!(e["error"], "invalid input: no persona nobody", "{e}");
    let r = c.post(format!("{base}/memories?actor=codex")).header("X-Atlas-Persona", "Careful")
        .json(&serde_json::json!({"scope": "global", "kind": "fact", "text": "never lands"})).send().await.unwrap();
    assert_eq!(r.status(), 400, "the header takes a slug, not a name");

    // A review persona lands an agent's memory pending; the user's own hands are never gated.
    let r = c.post(format!("{base}/memories?actor=codex")).header("X-Atlas-Persona", "careful")
        .json(&serde_json::json!({"scope": "global", "kind": "fact", "text": "reviewed first"})).send().await.unwrap();
    assert_eq!(r.status(), 201);
    let m: serde_json::Value = r.json().await.unwrap();
    assert_eq!(m["status"], "pending", "{m}");
    let r = c.post(format!("{base}/memories?actor=cli")).header("X-Atlas-Persona", "careful")
        .json(&serde_json::json!({"scope": "global", "kind": "fact", "text": "the user is exempt"})).send().await.unwrap();
    let m: serde_json::Value = r.json().await.unwrap();
    assert_eq!(m["status"], "active", "{m}");

    let repo = tempfile::tempdir().unwrap();
    fixture_repo(repo.path());
    let p: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let task: serde_json::Value = c.post(format!("{base}/tasks")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"project_id": p["id"], "title": "guarded"})).send().await.unwrap().json().await.unwrap();
    let key = task["key"].as_str().unwrap().to_string();

    // A deny persona refuses the move and names itself; the actor label stays plain.
    let refused = c.post(format!("{base}/tasks/{key}/move")).header("X-Atlas-Actor", "codex").header("X-Atlas-Persona", "locked")
        .json(&serde_json::json!({"stage": "In Progress"})).send().await.unwrap();
    assert_eq!(refused.status(), 409);
    let refused: serde_json::Value = refused.json().await.unwrap();
    assert_eq!(refused["error"], "persona 'locked' may not move tasks", "{refused}");
    let refused = c.post(format!("{base}/tasks/{key}/claim")).header("X-Atlas-Actor", "codex").header("X-Atlas-Persona", "locked")
        .json(&serde_json::json!({"force": false})).send().await.unwrap();
    assert_eq!(refused.status(), 409, "a claim is a move");

    let allowed = c.post(format!("{base}/tasks/{key}/move")).header("X-Atlas-Actor", "codex").header("X-Atlas-Persona", "open")
        .json(&serde_json::json!({"stage": "In Progress"})).send().await.unwrap();
    assert_eq!(allowed.status(), 200);
    let detail: serde_json::Value = c.get(format!("{base}/tasks/{key}")).send().await.unwrap().json().await.unwrap();
    let moved = detail["events"].as_array().unwrap().iter().find(|e| e["kind"] == "moved").expect("a moved event");
    assert_eq!(moved["actor"], "codex", "the label never carries the persona: {moved}");
    assert_eq!(moved["detail"]["persona"], "open", "{moved}");
    let plain = c.post(format!("{base}/tasks/{key}/move")).header("X-Atlas-Actor", "codex")
        .json(&serde_json::json!({"stage": "Testing"})).send().await.unwrap();
    assert_eq!(plain.status(), 200);
    let detail: serde_json::Value = c.get(format!("{base}/tasks/{key}")).send().await.unwrap().json().await.unwrap();
    let last = detail["events"].as_array().unwrap().iter().rfind(|e| e["kind"] == "moved").unwrap();
    assert!(last["detail"].get("persona").is_none(), "no persona, no field: {last}");
}
