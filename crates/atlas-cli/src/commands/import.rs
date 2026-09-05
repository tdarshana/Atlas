use crate::remote::RemoteBackend;
use atlas_core::backend::Backend;
use atlas_core::models::{DocKind, Memory, NewAgent, NewDoc, NewMemory, NewWorkflow, Trigger};
use atlas_core::workflow::migrate_docs::single_action_graph;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub async fn run(dir: PathBuf, backend: &RemoteBackend) -> anyhow::Result<()> {
    let mut agents = 0usize;
    for path in md_files(&dir.join("agents"))? {
        backend.save_agent(read_agent(&path)?, "import").await?;
        agents += 1;
    }

    let mut docs = 0usize;
    for path in md_files(&dir.join(super::export::doc_dir(DocKind::Practice)))? {
        backend.save_doc(DocKind::Practice, read_doc(&path)?, "import").await?;
        docs += 1;
    }
    // A pre-Phase-9 export directory may still hold Markdown workflow documents:
    // `DocKind::Workflow` no longer has anywhere to save one (the daemon's own
    // migration, run once at startup, only ever sees documents already in the store,
    // not ones an import is about to add). Each becomes a manual single-action
    // workflow through the same graph shape `workflow::migrate_docs` builds, rather
    // than failing.
    for path in md_files(&dir.join(super::export::doc_dir(DocKind::Workflow)))? {
        let doc = read_doc(&path)?;
        backend
            .create_workflow(
                NewWorkflow {
                    name: doc.name.clone(),
                    project_id: doc.project_id,
                    description: format!("Migrated from the workflow document '{}'.", doc.name),
                    trigger: Trigger::manual(),
                    graph: single_action_graph(&doc.body),
                    enabled: true,
                },
                "import",
            )
            .await?;
        docs += 1;
    }

    // Memories have no name to upsert on, so the guard against importing the same
    // export twice is the text itself. Every status is seeded, not just the active
    // one, because export writes every status and a pending or superseded memory
    // would otherwise come back again on each import. Each insert joins the set,
    // which also dedupes repeats within the file.
    let mut seen = HashSet::new();
    for status in super::export::STATUSES {
        seen.extend(backend.list_memories(status, None, atlas_core::models::MemoryScopeFilter::All, atlas_core::models::MemoryPage::default()).await?.iter().map(|m| normalize(&m.text)));
    }
    let (mut imported, mut skipped) = (0usize, 0usize);
    let path = dir.join("memories.jsonl");
    if path.exists() {
        for (i, line) in std::fs::read_to_string(&path)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let memory: Memory = serde_json::from_str(line).map_err(|e| anyhow::anyhow!("{}:{}: {e}", path.display(), i + 1))?;
            if !seen.insert(normalize(&memory.text)) {
                skipped += 1;
                continue;
            }
            backend
                .remember(
                    NewMemory {
                        scope: memory.scope,
                        project_id: memory.project_id,
                        kind: memory.kind,
                        text: memory.text,
                        tags: memory.tags,
                        source_agent: memory.source_agent,
                        source_tool: memory.source_tool,
                        confidence: memory.confidence,
                        status: memory.status,
                    },
                    "import",
                )
                .await?;
            imported += 1;
        }
    }

    println!("imported {agents} agents, {docs} practices and workflows, {imported} memories ({skipped} already present)");
    Ok(())
}

fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

/// The `.md` files in `dir`, sorted, or nothing when the directory is absent so
/// a partial export still imports.
fn md_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        // An unreadable entry is a file this import would have brought in, so it is
        // reported rather than dropped: a partial import that says nothing is worse
        // than one that stops.
        let path = entry.map_err(|e| anyhow::anyhow!("failed to read {}: {e}", dir.display()))?.path();
        if path.extension().is_some_and(|e| e == "md") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Reads an agent file in the Claude export format, including the `tags` line
/// the exporter adds beyond what Claude Code itself defines.
fn read_agent(path: &Path) -> anyhow::Result<NewAgent> {
    let (front, body) = split_frontmatter(&std::fs::read_to_string(path)?).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(NewAgent {
        name: name_of(&front, path),
        description: front.get("description").cloned().unwrap_or_default(),
        instructions: body,
        model_hint: front.get("model").cloned(),
        tools: comma_list(front.get("tools")),
        tags: comma_list(front.get("tags")),
    })
}

fn read_doc(path: &Path) -> anyhow::Result<NewDoc> {
    let (front, body) = split_frontmatter(&std::fs::read_to_string(path)?).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let project_id = match front.get("project_id") {
        Some(id) => Some(id.parse().map_err(|e| anyhow::anyhow!("{}: bad project_id: {e}", path.display()))?),
        None => None,
    };
    Ok(NewDoc { name: name_of(&front, path), body, tags: comma_list(front.get("tags")), project_id })
}

/// The frontmatter `name`, falling back to the file stem for a hand-written file.
fn name_of(front: &HashMap<String, String>, path: &Path) -> String {
    front
        .get("name")
        .cloned()
        .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
}

fn comma_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// Splits a leading `---` frontmatter block from the body. Only `key: value`
/// lines are read; the generated-by comment the exporters write is ignored.
///
/// Every exporter writes one blank line after the closing `---` and ends the
/// file with a newline, so exactly one of each is removed. Stripping more would
/// make an export, import and export cycle lose a body's own blank lines;
/// stripping fewer would make it gain one on every pass.
fn split_frontmatter(text: &str) -> anyhow::Result<(HashMap<String, String>, String)> {
    let rest = text.strip_prefix("---\n").ok_or_else(|| anyhow::anyhow!("file does not start with --- frontmatter"))?;
    let (head, body) = rest.split_once("\n---\n").ok_or_else(|| anyhow::anyhow!("frontmatter is not terminated by ---"))?;
    let mut front = HashMap::new();
    for line in head.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            front.insert(key.trim().to_string(), unquote(value.trim()));
        }
    }
    let body = body.strip_prefix('\n').unwrap_or(body);
    Ok((front, body.strip_suffix('\n').unwrap_or(body).to_string()))
}

/// Undoes the quoting the Claude exporter applies to a YAML scalar.
fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::export::claude_agent_md;
    use atlas_core::models::{Agent, Doc};

    #[test]
    fn agent_frontmatter_round_trips() {
        let agent = Agent {
            id: uuid::Uuid::nil(),
            name: "reviewer".into(),
            description: "note: careful".into(),
            instructions: "Review the diff.\n".into(),
            model_hint: Some("sonnet".into()),
            tools: vec!["read".into(), "mcp: grep".into()],
            tags: vec!["qa".into(), "slow".into()],
            version: 1,
            created_at: Default::default(),
            updated_at: Default::default(),
        };
        let (front, body) = split_frontmatter(&claude_agent_md(&agent)).unwrap();
        assert_eq!(front.get("name").unwrap(), "reviewer");
        assert_eq!(front.get("description").unwrap(), "note: careful", "a quoted scalar is unquoted, colon and all");
        assert_eq!(front.get("model").unwrap(), "sonnet");
        assert_eq!(comma_list(front.get("tools")), vec!["read", "mcp: grep"], "a colon in a tool must not split the line");
        assert_eq!(comma_list(front.get("tags")), vec!["qa", "slow"], "tags must survive the round trip");
        assert_eq!(body, "Review the diff.\n");
    }

    /// The body has to survive the trip byte for byte, or a repeated export and
    /// import would drift the stored text a newline at a time.
    #[test]
    fn doc_body_round_trips_unchanged() {
        for body in ["one line", "one line\n", "two\n\nparagraphs\n", "\nleading blank\n"] {
            let doc = Doc {
                id: uuid::Uuid::nil(),
                kind: DocKind::Practice,
                name: "review".into(),
                body: body.into(),
                tags: vec!["ci".into()],
                project_id: None,
                created_at: Default::default(),
                updated_at: Default::default(),
            };
            let (front, parsed) = split_frontmatter(&super::super::export::doc_md(&doc)).unwrap();
            assert_eq!(parsed, body, "body {body:?} did not survive the round trip");
            assert_eq!(front.get("name").unwrap(), "review");
            assert_eq!(comma_list(front.get("tags")), vec!["ci"]);
        }
    }

    #[test]
    fn frontmatter_must_be_terminated() {
        assert!(split_frontmatter("---\nname: x\nbody\n").is_err());
        assert!(split_frontmatter("no frontmatter here\n").is_err());
    }
}
