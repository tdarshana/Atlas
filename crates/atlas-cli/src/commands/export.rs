use crate::remote::RemoteBackend;
use atlas_core::backend::{MemoryBackend, ProjectBackend, LibraryBackend};
use atlas_core::export::claude_agent_md;
use atlas_core::library::validate_name;
use atlas_core::models::{Doc, DocKind, MemoryStatus};
use std::path::{Path, PathBuf};

/// Every status, so an export is a full snapshot rather than only what is live.
pub(super) const STATUSES: [MemoryStatus; 4] = [MemoryStatus::Active, MemoryStatus::Pending, MemoryStatus::Rejected, MemoryStatus::Superseded];

/// The directory each doc kind is written to, and read back from by `import`.
pub fn doc_dir(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Practice => "practices",
        DocKind::Workflow => "workflows",
    }
}

pub async fn run(dir: PathBuf, force: bool, backend: &RemoteBackend) -> anyhow::Result<()> {
    std::fs::create_dir_all(&dir)?;

    let mut memories = String::new();
    let mut memory_count = 0usize;
    for status in STATUSES {
        for memory in backend.list_memories(status, None, atlas_core::models::MemoryScopeFilter::All, atlas_core::models::MemoryPage::default()).await? {
            memories.push_str(&serde_json::to_string(&memory)?);
            memories.push('\n');
            memory_count += 1;
        }
    }
    std::fs::write(dir.join("memories.jsonl"), memories)?;

    let projects = backend.list_projects().await?;
    let mut lines = String::new();
    for project in &projects {
        lines.push_str(&serde_json::to_string(project)?);
        lines.push('\n');
    }
    std::fs::write(dir.join("projects.jsonl"), lines)?;

    let agents = backend.list_agents().await?;
    write_all(&dir.join("agents"), force, agents.iter().map(|a| (a.name.as_str(), claude_agent_md(a))))?;

    let practices = backend.list_docs(DocKind::Practice, None).await?;
    let workflows = backend.list_docs(DocKind::Workflow, None).await?;
    for (kind, docs) in [(DocKind::Practice, &practices), (DocKind::Workflow, &workflows)] {
        write_all(&dir.join(doc_dir(kind)), force, docs.iter().map(|d| (d.name.as_str(), doc_md(d))))?;
    }

    println!(
        "exported {memory_count} memories, {} projects, {} agents, {} practices, {} workflows to {}",
        projects.len(),
        agents.len(),
        practices.len(),
        workflows.len(),
        dir.display()
    );
    Ok(())
}

/// Writes one `<name>.md` per item into a directory it first empties, so that an
/// export into a directory used before does not leave a file for something since
/// deleted, which `import` would then bring back. Only these three subdirectories
/// are removed, never the target directory itself, which may be the user's own.
///
/// The directory is only emptied when everything in it looks like something a
/// previous export wrote; anything else and the export stops rather than delete a
/// directory the user meant to keep. `force` says to clear it regardless.
///
/// Names are validated on the way in (lowercase, digits, `-` and `_` only), so
/// they are safe as file names.
fn write_all<'a>(dir: &Path, force: bool, items: impl Iterator<Item = (&'a str, String)>) -> anyhow::Result<()> {
    if dir.is_dir() {
        if !force {
            check_prunable(dir)?;
        }
        std::fs::remove_dir_all(dir).map_err(|e| anyhow::anyhow!("failed to clear {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir)?;
    for (name, content) in items {
        std::fs::write(dir.join(format!("{name}.md")), content)?;
    }
    Ok(())
}

/// Refuses to prune a directory holding anything an export would not have
/// written: a subdirectory, a file of another type, or an `.md` file whose stem
/// is not a name Atlas would have given it.
fn check_prunable(dir: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).map_err(|e| anyhow::anyhow!("failed to read {}: {e}", dir.display()))? {
        let path = entry.map_err(|e| anyhow::anyhow!("failed to read {}: {e}", dir.display()))?.path();
        let exported = path.is_file()
            && path.extension().is_some_and(|e| e == "md")
            && path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| validate_name(s).is_ok());
        if !exported {
            anyhow::bail!(
                "{} holds {}, which `atlas export` did not write; export empties this directory, so move it aside or pass --force",
                dir.display(),
                path.file_name().unwrap_or(path.as_os_str()).to_string_lossy()
            );
        }
    }
    Ok(())
}

/// Renders a practice or workflow as frontmatter plus body, the form `import`
/// reads back.
pub(super) fn doc_md(doc: &Doc) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", doc.name));
    if !doc.tags.is_empty() {
        out.push_str(&format!("tags: {}\n", doc.tags.join(", ")));
    }
    if let Some(project_id) = doc.project_id {
        out.push_str(&format!("project_id: {project_id}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(&doc.body);
    // Always one closing newline, matching the agent exporter, so `import` can
    // undo the framing without having to guess how the body ended.
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pruning is what makes an export a snapshot, but it must not take a
    /// directory the user filled with something else.
    #[test]
    fn pruning_refuses_a_directory_holding_anything_else() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("practices");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("commits.md"), "x").unwrap();
        check_prunable(&dir).expect("an exported file is prunable");

        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        let err = check_prunable(&dir).unwrap_err().to_string();
        assert!(err.contains("notes.txt") && err.contains("--force"), "{err}");

        std::fs::remove_file(dir.join("notes.txt")).unwrap();
        std::fs::write(dir.join("Not A Name.md"), "mine").unwrap();
        assert!(check_prunable(&dir).is_err(), "an .md file Atlas would not have named is not prunable");

        std::fs::remove_file(dir.join("Not A Name.md")).unwrap();
        std::fs::create_dir(dir.join("nested")).unwrap();
        assert!(check_prunable(&dir).is_err(), "a subdirectory is not prunable");
    }
}
