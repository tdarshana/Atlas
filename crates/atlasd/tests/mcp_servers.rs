//! The agents' MCP servers.

mod common;
use common::*;

/// The agents' MCP servers (Phase 16) end to end over the JSON API: the listing with its
/// per-agent flags, a project listing, a check that answers 200 with `ok: false` for a
/// server that will not start, the enable switch, an add and a remove. Every file these
/// routes read or write is inside the daemon's own `ATLAS_SYNC_HOME` or the test's
/// repository, never the user's home.
#[tokio::test]
async fn mcp_servers_list_check_toggle_add_and_remove() {
    let sync_home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(sync_home.path().join(".cursor")).unwrap();
    std::fs::write(
        sync_home.path().join(".cursor/mcp.json"),
        r#"{
  "mcpServers": {
    "cursor-one": {
      "command": "atlas-no-such-command-exists",
      "args": [],
      "env": { "CURSOR_TOKEN": "SECRET-DO-NOT-LEAK" }
    }
  }
}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(sync_home.path().join(".claude/plugins/cache/acme/tools/aaaa1111")).unwrap();
    std::fs::write(
        sync_home.path().join(".claude/plugins/cache/acme/tools/aaaa1111/.mcp.json"),
        r#"{"mcpServers": {"packaged": {"command": "packaged-server", "args": []}}}"#,
    )
    .unwrap();
    std::fs::write(sync_home.path().join(".claude/settings.json"), r#"{"enabledPlugins": {"tools@acme": true}}"#).unwrap();

    let d = start_with_env(&[("ATLAS_SYNC_HOME", sync_home.path().to_str().unwrap())]).await;
    let base = format!("http://127.0.0.1:{}/api/v1", d.port);
    let c = d.client();

    // The global listing: every agent's user level, the plugin's server and Atlas.
    let list: serde_json::Value = c.get(format!("{base}/mcp/servers")).send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = list["servers"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["atlas", "cursor:user:cursor-one", "plugin:acme/tools:packaged"], "{list}");
    assert_eq!(list["warnings"].as_array().unwrap().len(), 0, "{list}");
    assert!(!list.to_string().contains("SECRET-DO-NOT-LEAK"), "a secret reached the wire: {list}");

    let atlas = &list["servers"][0];
    assert_eq!(atlas["is_atlas"], true, "{atlas}");
    assert_eq!(atlas["transport"], serde_json::json!({"kind": "stdio", "command": "atlas", "args": ["mcp"], "env_keys": []}), "{atlas}");
    assert_eq!(atlas["can_toggle"], false, "{atlas}");
    let cursor = &list["servers"][1];
    assert_eq!(cursor["transport"]["env_keys"], serde_json::json!(["CURSOR_TOKEN"]), "{cursor}");
    assert_eq!(cursor["can_toggle"], true, "{cursor}");
    assert_eq!(list["servers"][2]["plugin"], "acme/tools", "{list}");

    // A check of a command that is not there is an answer, not an error.
    let checked = c.post(format!("{base}/mcp/servers/cursor%3Auser%3Acursor-one/check")).send().await.unwrap();
    assert_eq!(checked.status(), 200);
    let checked: serde_json::Value = checked.json().await.unwrap();
    assert_eq!(checked["ok"], false, "{checked}");
    assert!(checked["error"].as_str().unwrap().contains("atlas-no-such-command-exists"), "{checked}");

    // A plugin server's id carries a slash, so it travels percent-encoded.
    let plugin_check = c.post(format!("{base}/mcp/servers/plugin%3Aacme%2Ftools%3Apackaged/check")).send().await.unwrap();
    assert_eq!(plugin_check.status(), 200, "an encoded slash reaches the route");
    assert_eq!(plugin_check.json::<serde_json::Value>().await.unwrap()["ok"], false);

    // Enable and disable, through Cursor's own switch.
    let off = c
        .put(format!("{base}/mcp/servers/cursor%3Auser%3Acursor-one/enabled"))
        .header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({"enabled": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(off.status(), 200);
    assert_eq!(off.json::<serde_json::Value>().await.unwrap()["enabled"], false);
    let refused = c
        .put(format!("{base}/mcp/servers/atlas/enabled"))
        .json(&serde_json::json!({"enabled": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 400, "Atlas has no switch");

    // The edit above replaced a config full of tokens. Its previous text goes to a backup
    // file under the daemon's own home, and the audit row names that file rather than
    // quoting it, because global search renders a matching audit row's whole detail as the
    // hit's title. Searching for terms that config is full of must not hand back a secret.
    for term in ["cursor", "atlas-no-such-command-exists", "mcp_config_edit", "CURSOR_TOKEN"] {
        let found = c.get(format!("{base}/search?q={term}")).send().await.unwrap();
        assert_eq!(found.status(), 200, "the search itself has to work for this to mean anything");
        let hits = found.text().await.unwrap();
        assert!(!hits.contains("SECRET-DO-NOT-LEAK"), "'{term}' returned a secret from the audit trail: {hits}");
        assert!(!hits.contains("mcp_config_edit"), "'{term}' returned a config edit at all: {hits}");
    }

    // Add: a new server in the repository's own `.mcp.json`, which Atlas may create.
    let repo = repo_free_tempdir();
    fixture_repo(repo.path());
    let project: serde_json::Value =
        c.post(format!("{base}/projects/connect")).json(&serde_json::json!({"root": repo.path()})).send().await.unwrap().json().await.unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    let added = c
        .post(format!("{base}/mcp/servers"))
        .header("X-Atlas-Actor", "desktop")
        .json(&serde_json::json!({
            "source": "claude",
            "scope": "project",
            "project_id": project_id,
            "name": "repo-one",
            "transport": {"kind": "stdio", "command": "repo-server", "args": ["--serve"], "env": {"REPO_TOKEN": "SECRET-DO-NOT-LEAK"}}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(added.status(), 201);
    let added: serde_json::Value = added.json().await.unwrap();
    assert_eq!(added["id"], "claude:project:repo-one", "{added}");
    assert_eq!(added["transport"]["env_keys"], serde_json::json!(["REPO_TOKEN"]), "{added}");
    assert!(!added.to_string().contains("SECRET-DO-NOT-LEAK"), "{added}");
    let written = std::fs::read_to_string(repo.path().join(".mcp.json")).unwrap();
    assert!(written.contains("REPO_TOKEN") && written.contains("repo-server"), "{written}");

    // A project listing shows it, switched off until it is approved.
    let listed: serde_json::Value =
        c.get(format!("{base}/mcp/servers?project_id={project_id}")).send().await.unwrap().json().await.unwrap();
    let row = listed["servers"].as_array().unwrap().iter().find(|s| s["id"] == "claude:project:repo-one").unwrap();
    assert_eq!(row["enabled"], false, "a repository server waits for approval: {listed}");
    assert_eq!(row["project_id"], project_id, "{row}");

    // Adding the same name again is a conflict, not a replacement.
    let again = c
        .post(format!("{base}/mcp/servers"))
        .json(&serde_json::json!({
            "source": "claude", "scope": "project", "project_id": project_id, "name": "repo-one",
            "transport": {"kind": "http", "url": "https://example.test/mcp"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 409);

    // Remove.
    let removed = c
        .delete(format!("{base}/mcp/servers/claude%3Aproject%3Arepo-one?project_id={project_id}"))
        .header("X-Atlas-Actor", "desktop")
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 204);
    assert!(!std::fs::read_to_string(repo.path().join(".mcp.json")).unwrap().contains("repo-server"));
    let gone = c
        .delete(format!("{base}/mcp/servers/claude%3Aproject%3Arepo-one?project_id={project_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 404);

    // A config that does not parse is reported by position. Both parsers would otherwise
    // print the offending source line, which is where a token on a half-edited line would
    // leave the daemon: once in the listing's warnings, once in an error body.
    std::fs::create_dir_all(sync_home.path().join(".codex")).unwrap();
    std::fs::write(
        sync_home.path().join(".codex/config.toml"),
        "[mcp_servers.one]\ncommand = \"x\"\ntoken = \"SECRET-DO-NOT-LEAK\" and then some\n",
    )
    .unwrap();
    let broken: serde_json::Value = c.get(format!("{base}/mcp/servers")).send().await.unwrap().json().await.unwrap();
    let warnings = broken["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{broken}");
    let warning = warnings[0].as_str().unwrap();
    assert!(!warning.contains("SECRET-DO-NOT-LEAK"), "the warning quoted the broken line: {warning}");
    assert!(warning.contains("line 3") && warning.contains("column"), "{warning}");

    let refused = c
        .post(format!("{base}/mcp/servers"))
        .json(&serde_json::json!({
            "source": "codex", "scope": "user", "project_id": null, "name": "nope",
            "transport": {"kind": "stdio", "command": "x", "args": []}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 400);
    let body = refused.text().await.unwrap();
    assert!(!body.contains("SECRET-DO-NOT-LEAK"), "the error body quoted the broken line: {body}");
    assert!(body.contains("line 3"), "{body}");
}
