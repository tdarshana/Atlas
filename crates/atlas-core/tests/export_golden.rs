use atlas_core::export::*;
use atlas_core::models::*;

fn agent() -> Agent {
    Agent {
        id: uuid::Uuid::nil(),
        name: "reviewer".into(),
        description: "Reviews pull requests for correctness and risk".into(),
        instructions: "You are a strict reviewer.\n\nReport findings with file:line.".into(),
        model_hint: Some("opus".into()),
        tools: vec!["Read".into(), "Grep".into()],
        tags: vec!["qa".into()],
        version: 3,
        created_at: Default::default(),
        updated_at: Default::default(),
    }
}

fn practice() -> Doc {
    Doc {
        id: uuid::Uuid::nil(),
        kind: DocKind::Practice,
        name: "commits".into(),
        body: "Imperative mood, one change per commit.".into(),
        tags: vec![],
        project_id: None,
        created_at: Default::default(),
        updated_at: Default::default(),
    }
}

fn check(name: &str, actual: &str) {
    let path = format!("{}/tests/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run with UPDATE_GOLDEN=1 once"));
    assert_eq!(actual, expected, "golden mismatch for {name}");
}

#[test]
fn claude_md_matches_golden() {
    check("reviewer.claude.md", &claude_agent_md(&agent()));
}

#[test]
fn codex_toml_matches_golden() {
    check("reviewer.codex.toml", &codex_agent_toml(&agent()));
}

#[test]
fn managed_block_matches_golden() {
    check(
        "managed_block.md",
        &render_block(&BlockContext { mcp_command: "atlas mcp".into(), agents: vec![agent()], practices: vec![practice()], project_name: Some("fixture".into()) }),
    );
}

#[test]
fn splice_replaces_only_between_markers() {
    let existing = "# My notes\n\nkeep me\n\n<!-- atlas:start -->\nold\n<!-- atlas:end -->\n\nalso keep\n";
    let out = splice_block(existing, "<!-- atlas:start -->\nnew\n<!-- atlas:end -->");
    assert_eq!(out, "# My notes\n\nkeep me\n\n<!-- atlas:start -->\nnew\n<!-- atlas:end -->\n\nalso keep\n");
    assert_eq!(splice_block("# Notes\n", "<!-- atlas:start -->\nx\n<!-- atlas:end -->"), "# Notes\n\n<!-- atlas:start -->\nx\n<!-- atlas:end -->\n");
}

#[test]
fn generated_header_detection() {
    assert!(is_generated(&claude_agent_md(&agent())));
    assert!(is_generated(&codex_agent_toml(&agent())));
    assert!(!is_generated("---\nname: x\n---\n"));
}
