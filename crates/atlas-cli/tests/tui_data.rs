mod common;

use std::process::Command;
use std::sync::Arc;

use atlas_cli::remote::RemoteBackend;
use atlas_cli::tui::data::perform;
use atlas_cli::tui::state::{Action, Effect};
use atlas_core::backend::Backend;
use atlas_core::models::*;
use uuid::Uuid;

fn atlas() -> Command { Command::new(env!("CARGO_BIN_EXE_atlas")) }

/// `perform` maps every `Effect` onto one `RemoteBackend` call, so this exercises it
/// against a real daemon: a memory saved through the backend has to come back out of
/// `Recall`, `Status` has to answer, `Forget` on an id that does not exist has to
/// surface as `Action::Error`, and `ConnectCwd` has to register `cwd` as a project.
#[tokio::test]
async fn perform_maps_effects_onto_the_backend() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let atlasd_dir = std::path::Path::new(env!("CARGO_BIN_EXE_atlas")).parent().unwrap().to_path_buf();
    let env = |c: &mut Command| {
        c.env("ATLAS_HOME", home.path())
            .env("ATLAS_PORT", port.to_string())
            .env("ATLAS_NO_EMBED", "1")
            .env("PATH", format!("{}:{}", atlasd_dir.display(), std::env::var("PATH").unwrap_or_default()));
    };
    let mut c = atlas();
    env(&mut c);
    let out = c.args(["daemon", "start"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let backend = Arc::new(RemoteBackend::new(port));
    backend
        .remember(
            NewMemory {
                scope: MemoryScope::Global,
                project_id: None,
                kind: MemoryKind::Fact,
                text: "the deploy target is fly.io".into(),
                tags: vec![],
                source_agent: None,
                source_tool: Some("test".into()),
                confidence: 1.0,
                status: MemoryStatus::Active,
            },
            "test",
        )
        .await
        .unwrap();

    let action = perform(Effect::Recall(String::new()), backend.clone(), repo.path().to_path_buf()).await;
    match action {
        Action::MemoriesLoaded(hits) => assert_eq!(hits.len(), 1, "expected one memory: {hits:?}"),
        other => panic!("expected MemoriesLoaded, got {other:?}"),
    }

    let action = perform(Effect::Status, backend.clone(), repo.path().to_path_buf()).await;
    assert!(matches!(action, Action::StatusLoaded(_)), "expected StatusLoaded, got {action:?}");

    let action = perform(Effect::Forget(Uuid::new_v4()), backend.clone(), repo.path().to_path_buf()).await;
    assert!(matches!(action, Action::Error(_)), "expected Error, got {action:?}");

    let action = perform(Effect::ConnectCwd, backend.clone(), repo.path().to_path_buf()).await;
    match action {
        Action::ProjectsLoaded(projects) => assert_eq!(projects.len(), 1, "expected one project: {projects:?}"),
        other => panic!("expected ProjectsLoaded, got {other:?}"),
    }

    let mut c = atlas();
    env(&mut c);
    assert!(c.args(["daemon", "stop"]).status().unwrap().success());
}
