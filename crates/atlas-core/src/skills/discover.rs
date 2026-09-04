//! Finds the `SKILL.md` folders an agent can use: the project's own
//! `.claude/skills` and `.codex/skills`, the user's `~/.claude/skills` and
//! `~/.codex/skills`, and the skills inside every installed Claude Code plugin under
//! `~/.claude/plugins/cache`.
//!
//! Read-only. Nothing here creates a directory, follows a symlink out of its root, or
//! reads more than [`frontmatter::MAX_SKILL_BYTES`] of a file. An entry that cannot be
//! read is skipped and reported as a warning rather than failing the whole listing.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::{MemoryScope, SkillSource, SkillSummary};
use crate::{AtlasError, Result};

use super::frontmatter;

/// The file that makes a directory a skill.
pub const SKILL_FILE: &str = "SKILL.md";

/// At most this many other files are listed for one skill, for display only.
pub const MAX_SKILL_FILES: usize = 200;

/// How deep the file listing of one skill folder goes. Enough for the
/// `references/`, `scripts/` and `assets/` layout skills use, shallow enough that a
/// symlinked or accidental tree cannot be walked forever.
const MAX_FILE_DEPTH: usize = 3;

/// A discovered skill: what a listing shows, plus the folder it was read from, which
/// `get` and `write_body` need and no caller outside this module sees.
#[derive(Debug, Clone)]
pub struct Discovered {
    pub summary: SkillSummary,
    pub dir: PathBuf,
}

/// Everything discovery found, plus what it could not read.
#[derive(Debug, Default)]
pub struct Found {
    pub skills: Vec<Discovered>,
    pub warnings: Vec<String>,
}

/// Where a global discovery reads from: `ATLAS_SYNC_HOME` when set on the daemon
/// process, else the daemon user's home. The same override `sync` uses, read at call
/// time so a test can point it at a temp directory; there is no client-side override,
/// since a request must never be able to name the directory the daemon reads.
pub fn skills_home() -> Result<PathBuf> {
    std::env::var_os("ATLAS_SYNC_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()))
        .ok_or_else(|| AtlasError::Other("no home directory to read skills from".into()))
}

/// Every skill under `home`: `.claude/skills`, `.codex/skills`, and the newest
/// installed version of each plugin's `skills` directory. The home is a parameter
/// rather than read here so a test can point it at a temp directory without touching
/// the process environment; [`skills_home`] is what resolves the real one.
pub fn discover_global_in(home: &Path) -> Found {
    let mut found = Found::default();
    scan_root(&home.join(".claude/skills"), SkillSource::ClaudeUser, MemoryScope::Global, None, None, &mut found);
    scan_root(&home.join(".codex/skills"), SkillSource::CodexUser, MemoryScope::Global, None, None, &mut found);
    scan_plugins(&home.join(".claude/plugins/cache"), &mut found);
    found
}

/// Every skill under one project root: `<root>/.claude/skills` and
/// `<root>/.codex/skills`.
pub fn discover_project(root: &Path, project_id: Uuid) -> Found {
    let mut found = Found::default();
    scan_root(&root.join(".claude/skills"), SkillSource::ClaudeProject, MemoryScope::Project, Some(project_id), None, &mut found);
    scan_root(&root.join(".codex/skills"), SkillSource::CodexProject, MemoryScope::Project, Some(project_id), None, &mut found);
    found
}

/// Every `<marketplace>/<plugin>` under the plugin cache, each at the version directory
/// with the newest modification time. Directories whose name ends in `.clone` or starts
/// with `temp_` are a half-finished install, and are skipped.
fn scan_plugins(cache: &Path, found: &mut Found) {
    let Some(marketplaces) = read_dirs(cache, found) else { return };
    for marketplace in marketplaces {
        let Some(plugins) = read_dirs(&marketplace, found) else { continue };
        for plugin in plugins {
            let Some(versions) = read_dirs(&plugin, found) else { continue };
            let newest = versions
                .into_iter()
                .filter(|v| !is_scratch_dir(v))
                .filter_map(|v| modified(&v).map(|t| (t, v)))
                .max_by_key(|(t, _)| *t)
                .map(|(_, v)| v);
            let Some(version) = newest else { continue };
            let label = format!("{}/{}", name_of(&marketplace), name_of(&plugin));
            scan_root(&version.join("skills"), SkillSource::Plugin, MemoryScope::Global, None, Some(label), found);
        }
    }
}

/// Reads every skill directly under `root`. A root that does not exist is not a
/// warning: most projects have no `.codex/skills`, and saying so on every listing would
/// bury the warnings that matter.
fn scan_root(
    root: &Path,
    source: SkillSource,
    scope: MemoryScope,
    project_id: Option<Uuid>,
    plugin: Option<String>,
    found: &mut Found,
) {
    if !root.is_dir() {
        return;
    }
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let Some(entries) = read_dirs(root, found) else { return };
    for dir in entries {
        // A symlink that leaves the root is not this root's skill, and following it
        // would let a link inside the user's home hand out any directory the daemon can
        // read.
        match dir.canonicalize() {
            Ok(real) if real.starts_with(&canonical_root) => {}
            Ok(_) => continue,
            Err(e) => {
                found.warnings.push(format!("{}: {e}", dir.display()));
                continue;
            }
        }
        let file = dir.join(SKILL_FILE);
        if !file.is_file() {
            continue;
        }
        let text = match read_capped(&file) {
            Ok(t) => t,
            Err(e) => {
                found.warnings.push(format!("{}: {e}", file.display()));
                continue;
            }
        };
        let folder = name_of(&dir);
        let relative = match plugin.as_deref() {
            // A plugin skill's id names the plugin, not the version directory it
            // currently sits in, so it survives the plugin being updated.
            Some(label) => format!("{label}/{folder}"),
            None => folder.clone(),
        };
        let fm = frontmatter::parse(&text);
        found.skills.push(Discovered {
            summary: SkillSummary {
                id: format!("{}:{relative}", source.as_str()),
                source,
                name: fm.name.unwrap_or(folder),
                description: fm.description.unwrap_or_else(|| frontmatter::first_paragraph(&text)),
                scope,
                project_id,
                path: Some(dir.display().to_string()),
                plugin: plugin.clone(),
                editable: is_writable(&file),
                updated_at: modified(&file),
                enabled_here: None,
            },
            dir,
        });
    }
}

/// The other files in a skill folder, relative to it, sorted, `SKILL.md` excluded and
/// capped at [`MAX_SKILL_FILES`]. For display only, so an unreadable subdirectory is
/// simply left out.
pub fn list_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((current, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth + 1 < MAX_FILE_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Ok(rel) = path.strip_prefix(dir) else { continue };
            let rel = rel.display().to_string();
            if rel == SKILL_FILE {
                continue;
            }
            out.push(rel);
        }
    }
    out.sort();
    out.truncate(MAX_SKILL_FILES);
    out
}

/// A skill's `SKILL.md`, capped at [`frontmatter::MAX_SKILL_BYTES`].
pub fn read_body(dir: &Path) -> Result<String> {
    read_capped(&dir.join(SKILL_FILE))
}

/// Writes a skill's `SKILL.md` through a temp file in the same folder and a rename, so
/// a failed write leaves the old file intact rather than a truncated one. The temp file
/// is removed on a failure, so no `.atlas-*` litter is left behind.
pub fn write_body(dir: &Path, body: &str) -> Result<()> {
    let target = dir.join(SKILL_FILE);
    let temp = dir.join(format!(".atlas-{}.tmp", Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp, body) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}

/// Reads at most [`frontmatter::MAX_SKILL_BYTES`] of a file as UTF-8, lossily: a skill
/// with a stray byte still lists rather than disappearing.
fn read_capped(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(frontmatter::MAX_SKILL_BYTES as u64).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The subdirectories of `path`, or `None` (with a warning) when it cannot be listed.
/// A path that simply does not exist is neither.
fn read_dirs(path: &Path, found: &mut Found) -> Option<Vec<PathBuf>> {
    if !path.exists() {
        return None;
    }
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut out: Vec<PathBuf> = Vec::new();
            for entry in entries {
                match entry {
                    Ok(e) if e.path().is_dir() => out.push(e.path()),
                    Ok(_) => {}
                    Err(e) => found.warnings.push(format!("{}: {e}", path.display())),
                }
            }
            out.sort();
            Some(out)
        }
        Err(e) => {
            found.warnings.push(format!("{}: {e}", path.display()));
            None
        }
    }
}

fn is_scratch_dir(path: &Path) -> bool {
    let name = name_of(path);
    name.ends_with(".clone") || name.starts_with("temp_")
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn modified(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).map(DateTime::<Utc>::from)
}

/// Whether the daemon user may write this file. Read from the file's own permissions
/// rather than attempted, so listing never touches a file it is only describing.
fn is_writable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| !m.permissions().readonly()).unwrap_or(false)
}
