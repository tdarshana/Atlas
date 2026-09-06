use atlas_core::export::*;
use atlas_core::models::*;

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
fn managed_block_matches_golden() {
    check(
        "managed_block.md",
        &render_block(&BlockContext { mcp_command: "atlas mcp".into(), practices: vec![practice()], project_name: Some("fixture".into()) }),
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
    assert!(is_generated(&format!("---\n# {GENERATED_HEADER}\nname: x\n---\n")));
    assert!(!is_generated("---\nname: x\n---\n"));
}
