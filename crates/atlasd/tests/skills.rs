//! Skills: listing, editing and per-project gating.

mod common;
use common::*;

/// Skills (Phase 15) end to end over the JSON API: the global listing, a project
/// listing that widens to the project's own `SKILL.md` folders, an in-place edit that
/// lands on disk, and a disabled list that shows in `enabled_here` and in
/// `project_context`.
#[tokio::test]
async fn skills_list_edit_and_gate_per_project() {
    // The daemon reads its global skills from `ATLAS_SYNC_HOME`, never the real home.
    let sync_home = tempfile::tempdir().unwrap();
    let user_skill = sync_home.path().join(".claude/skills/greeter");
    std::fs::create_dir_all(&user_skill).unwrap();
    // A block scalar description, the shape real plugin skills use: it has to arrive
    // folded, not as a bare ">-".
    std::fs::write(user_skill.join("SKILL.md"), "---\nname: greeter\ndescription: >-\n  Greets a person\n  by name.\n---\n\nSay hello.\n").unwrap();
    let packaged = sync_home.path().join(".claude/plugins/cache/acme/tools/aaaa1111/skills/packaged");
    std::fs::create_dir_all(&packaged).unwrap();
    std::fs::write(packaged.join("SKILL.md"), "---\nname: packaged\ndescription: From a plugin.\n---\n\nPackaged body.\n").unwrap();

    let d = start_with_env(&[("ATLAS_SYNC_HOME", sync_home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    let repo = repo_free_tempdir();
    fixture_repo(repo.path());
    let deployer = repo.path().join(".claude/skills/deployer");
    std::fs::create_dir_all(&deployer).unwrap();
    std::fs::write(deployer.join("SKILL.md"), "---\nname: deployer\ndescription: Ships it.\n---\n\nRun the deploy.\n").unwrap();
    std::fs::write(deployer.join("checklist.md"), "one\n").unwrap();

    // Without a project, only the global skills are listed.
    let global: serde_json::Value = c.get(format!("{base}/skills")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = global["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["claude-user:greeter", "plugin:acme/tools/packaged"], "sorted by name: {global}");
    assert!(global["skills"][0]["enabled_here"].is_null(), "no project was named: {global}");
    assert_eq!(global["warnings"].as_array().unwrap().len(), 0, "{global}");
    assert_eq!(global["skills"][0]["description"], "Greets a person by name.", "a block scalar description arrives folded: {global}");
    let plugin_row = &global["skills"][1];
    assert_eq!(plugin_row["plugin"], "acme/tools", "{plugin_row}");
    assert_eq!(plugin_row["source"], "plugin", "{plugin_row}");

    // A plugin skill's id carries slashes and a colon. Both reach the route: raw, the
    // way `RemoteBackend` builds it, and percent-encoded, the way a browser client's
    // `encodeURIComponent` does.
    for addressed in ["plugin:acme/tools/packaged", "plugin%3Aacme%2Ftools%2Fpackaged"] {
        let one = c.get(format!("{base}/skills/{addressed}")).send().await.unwrap();
        assert_eq!(one.status(), 200, "{addressed} did not resolve");
        assert!(one.json::<serde_json::Value>().await.unwrap()["body"].as_str().unwrap().contains("Packaged body."), "{addressed}");
    }

    let created = c.post(format!("{base}/skills")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"name": "house-style", "description": "How we write.", "body": "# house style\n"})).send().await.unwrap();
    assert_eq!(created.status(), 201);
    let native: serde_json::Value = created.json().await.unwrap();
    let native_id = native["id"].as_str().unwrap().to_string();
    assert_eq!(native["source"], "native", "{native}");
    assert_eq!(native["scope"], "global", "{native}");
    assert_eq!(native["editable"], true, "{native}");

    let project: serde_json::Value = c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    // A project widens the listing to its own roots and answers `enabled_here`.
    let listed: serde_json::Value = c.get(format!("{base}/skills?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = listed["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"claude-project:deployer") && ids.contains(&"claude-user:greeter") && ids.contains(&native_id.as_str()), "{ids:?}");
    assert!(listed["skills"].as_array().unwrap().iter().all(|s| s["enabled_here"] == true), "{listed}");

    // One skill by id, with its text and the files beside it.
    let one: serde_json::Value = c.get(format!("{base}/skills/claude-project:deployer?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    assert!(one["body"].as_str().unwrap().contains("Run the deploy."), "{one}");
    assert_eq!(one["files"], serde_json::json!(["checklist.md"]), "{one}");
    assert_eq!(one["plugin"], serde_json::Value::Null, "{one}");
    // The stored root is canonical, so only the tail is compared: on macOS the temp
    // directory reaches the daemon as `/private/var/...` and the test as `/var/...`.
    assert!(one["path"].as_str().unwrap().ends_with(".claude/skills/deployer"), "{one}");

    // An edit in place rewrites the file on disk and comes back with the new text.
    let edited = "---\nname: deployer\ndescription: Ships it, carefully.\n---\n\nRun the deploy twice.\n";
    let put = c.put(format!("{base}/skills/claude-project:deployer?project_id={project_id}")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"body": edited})).send().await.unwrap();
    assert_eq!(put.status(), 200);
    let put_body: serde_json::Value = put.json().await.unwrap();
    assert_eq!(put_body["description"], "Ships it, carefully.", "{put_body}");
    assert_eq!(std::fs::read_to_string(deployer.join("SKILL.md")).unwrap(), edited);

    // The same route edits a native skill's stored body.
    let put = c.put(format!("{base}/skills/{native_id}")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"body": "# house style, revised\n"})).send().await.unwrap();
    assert_eq!(put.status(), 200);
    assert_eq!(put.json::<serde_json::Value>().await.unwrap()["body"], "# house style, revised\n");

    // An unknown id is a 404, not a path read.
    let missing = c.get(format!("{base}/skills/claude-user:nope")).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    // The disabled list refuses an id no skill holds, then takes a real one.
    let bad = c.put(format!("{base}/projects/{project_id}/skills")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["claude-user:nope"]})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    assert!(bad.json::<serde_json::Value>().await.unwrap()["error"].as_str().unwrap().contains("nope"));

    let gated = c.put(format!("{base}/projects/{project_id}/skills")).header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"disabled": ["claude-project:deployer"]})).send().await.unwrap();
    assert_eq!(gated.status(), 200);
    assert_eq!(gated.json::<serde_json::Value>().await.unwrap()["skills_disabled"], serde_json::json!(["claude-project:deployer"]));

    let listed: serde_json::Value = c.get(format!("{base}/skills?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    let deployer_row = listed["skills"].as_array().unwrap().iter().find(|s| s["id"] == "claude-project:deployer").unwrap();
    assert_eq!(deployer_row["enabled_here"], false, "a disabled skill still lists, switched off: {listed}");

    // `project_context` carries only the skills that still apply.
    let ctx: serde_json::Value = c.post(format!("{base}/projects/context")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let context_ids: Vec<&str> = ctx["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(!context_ids.contains(&"claude-project:deployer"), "{context_ids:?}");
    assert!(context_ids.contains(&"claude-user:greeter") && context_ids.contains(&native_id.as_str()), "{context_ids:?}");

    // A native skill can be deleted; a discovered one never is.
    let refused = c.delete(format!("{base}/skills/claude-user:greeter")).header("X-Atlas-Actor", "desktop").send().await.unwrap();
    assert_eq!(refused.status(), 400);
    let removed = c.delete(format!("{base}/skills/{native_id}")).header("X-Atlas-Actor", "desktop").send().await.unwrap();
    assert_eq!(removed.status(), 204);
    assert!(std::fs::read_to_string(user_skill.join("SKILL.md")).is_ok(), "no file was removed");
}
