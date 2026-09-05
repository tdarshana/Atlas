mod common;

use std::sync::Arc;

use common::TestDaemon;

use atlas_cli::remote::RemoteBackend;
use atlas_cli::tui::data::perform;
use atlas_cli::tui::state::{Action, Effect};
use atlas_core::backend::MemoryBackend;
use atlas_core::models::*;
use uuid::Uuid;

/// `perform` maps every `Effect` onto one `RemoteBackend` call, so this exercises it
/// against a real daemon: a memory saved through the backend has to come back out of
/// `Recall`, `Status` has to answer, `Forget` on an id that does not exist has to
/// surface as `Action::Error`, and `ConnectCwd` has to register `cwd` as a project.
#[tokio::test]
async fn perform_maps_effects_onto_the_backend() {
    let daemon = TestDaemon::new();
    let repo = tempfile::tempdir().unwrap();
    common::fixture_repo(repo.path());
    let out = daemon.cmd().args(["daemon", "start"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let backend = Arc::new(RemoteBackend::new(daemon.port));
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
}
