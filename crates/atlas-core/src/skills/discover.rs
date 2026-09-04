//! Finds the `SKILL.md` folders an agent can use: the project's own
//! `.claude/skills` and `.codex/skills`, the user's `~/.claude/skills` and
//! `~/.codex/skills`, and the skills inside every installed Claude Code plugin under
//! `~/.claude/plugins/cache`.
//!
//! Read-only, and fenced to one root at a time. Nothing here creates a directory, and
//! every path it reads or writes, the skill folder, its `SKILL.md` and any file listed
//! beside it, is canonicalised and required to stay under the canonicalised root, so a
//! symlink can never carry a read out of the root it was found in. No file is read past
//! [`frontmatter::MAX_SKILL_BYTES`]. An entry that cannot be read is skipped and
//! reported as a warning rather than failing the whole listing.

use std::collections::BTreeSet;
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

/// How many directory levels below a skill folder the file listing goes: a file three
/// levels down (`references/a/b/notes.md`) is listed, one deeper is not. Enough for the
/// `references/`, `scripts/` and `assets/` layout skills use, shallow enough that an
/// accidental tree cannot be walked forever.
const MAX_FILE_DEPTH: usize = 3;

/// How many directory entries one skill's file listing will look at before it stops.
/// The listing is cosmetic, so a folder holding a build output directory costs a bounded
/// walk rather than an unbounded one.
const MAX_FILE_ENTRIES_SCANNED: usize = 10_000;

/// A discovered skill: what a listing shows, plus the folder it was read from and the
/// canonical root that folder has to stay under. Neither path leaves this crate.
#[derive(Debug, Clone)]
pub struct Discovered {
    pub summary: SkillSummary,
    pub dir: PathBuf,
    /// The canonicalised source root `dir` was found under. Every later read or write
    /// re-checks against it, so a symlink planted between the listing and the read
    /// cannot widen what Atlas will open.
    pub root: PathBuf,
}

/// Everything discovery found, plus what it could not read.
#[derive(Debug, Default)]
pub struct Found {
    pub skills: Vec<Discovered>,
    pub warnings: Vec<String>,
}

/// Every skill under `home`: `.claude/skills`, `.codex/skills`, and the newest
/// installed version of each plugin's `skills` directory. The home is always a
/// parameter; `AtlasPaths::skills_home` is what resolves the real one, once, from
/// `ATLAS_SYNC_HOME` or the user's own home.
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

/// Reads every skill directly under `root`. A root that is not there is not a warning:
/// most projects have no `.codex/skills`, and saying so on every listing would bury the
/// warnings that matter. A root that exists but cannot be listed is a warning.
fn scan_root(
    root: &Path,
    source: SkillSource,
    scope: MemoryScope,
    project_id: Option<Uuid>,
    plugin: Option<String>,
    found: &mut Found,
) {
    let Some(entries) = read_dirs(root, found) else { return };
    let canonical_root = match root.canonicalize() {
        Ok(r) => r,
        Err(e) => {
            found.warnings.push(format!("{}: {e}", root.display()));
            return;
        }
    };
    for dir in entries {
        // A symlink that leaves the root is not this root's skill, and following it
        // would let a link inside the user's home hand out any directory the daemon can
        // read.
        match inside_root(&dir, &canonical_root) {
            Ok(Some(_)) => {}
            // Said rather than skipped in silence: a user who symlinked a skill folder
            // in from elsewhere should be told why it is missing, not left guessing.
            Ok(None) => {
                found.warnings.push(format!("{}: resolves outside {}", dir.display(), root.display()));
                continue;
            }
            Err(e) => {
                found.warnings.push(format!("{}: {e}", dir.display()));
                continue;
            }
        }
        // The same rule for the file itself: a real folder whose `SKILL.md` is a symlink
        // to a private file outside the root would otherwise have that file's first
        // paragraph published as a description and its whole text served by `skill_get`.
        let file = dir.join(SKILL_FILE);
        let file = match inside_root(&file, &canonical_root) {
            // Canonicalising resolves the link, so an absent `SKILL.md` lands in `Err`
            // (not found) rather than being reported as an escape. That is the common
            // case, a directory that is not a skill, so it is silent.
            Ok(Some(real)) => real,
            Ok(None) => {
                found.warnings.push(format!("{}: SKILL.md resolves outside {}", dir.display(), root.display()));
                continue;
            }
            Err(_) => continue,
        };
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
            root: canonical_root.clone(),
        });
    }
}

/// `path` canonicalised when it stays under `root`, `Ok(None)` when it resolves outside
/// it, and `Err` when it cannot be resolved at all (most often because it is not there).
fn inside_root(path: &Path, root: &Path) -> std::io::Result<Option<PathBuf>> {
    let real = path.canonicalize()?;
    Ok(real.starts_with(root).then_some(real))
}

/// The other files in a skill folder, relative to it, sorted, `SKILL.md` excluded and
/// capped at [`MAX_SKILL_FILES`]. For display only, so an unreadable subdirectory is
/// simply left out, as is anything resolving outside `root`. The set is trimmed as the
/// walk goes rather than after it, so a folder holding a large tree costs a bounded
/// amount of memory.
pub fn list_files(dir: &Path, root: &Path) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut seen = 0usize;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((current, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_FILE_ENTRIES_SCANNED {
                return out.into_iter().collect();
            }
            let path = entry.path();
            // `read_dir`'s file type does not follow symlinks, so this is resolved
            // rather than trusted: a link is only listed, or descended into, when it
            // stays under the root.
            let Ok(Some(real)) = inside_root(&path, root) else { continue };
            if real.is_dir() {
                if depth < MAX_FILE_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Ok(rel) = path.strip_prefix(dir) else { continue };
            let rel = rel.display().to_string();
            if rel == SKILL_FILE {
                continue;
            }
            out.insert(rel);
            if out.len() > MAX_SKILL_FILES {
                out.pop_last();
            }
        }
    }
    out.into_iter().collect()
}

/// A skill's `SKILL.md`, capped at [`frontmatter::MAX_SKILL_BYTES`] and re-checked
/// against the root it was found under.
pub fn read_body(dir: &Path, root: &Path) -> Result<String> {
    read_capped(&checked_skill_file(dir, root)?)
}

/// Writes a skill's `SKILL.md` through a temp file in the same folder and a rename, so a
/// failed write leaves the old text intact rather than a truncated one. The temp file is
/// removed on a failure, so no `.atlas-*` litter is left behind, and it is given the
/// target's own permissions first, so a `0600` skill does not come back world-readable.
/// The target is re-checked against its root: a `SKILL.md` that resolves outside the
/// root it was listed under is refused rather than replaced.
pub fn write_body(dir: &Path, root: &Path, body: &str) -> Result<()> {
    let target = checked_skill_file(dir, root)?;
    let mode = std::fs::metadata(&target).map(|m| m.permissions()).ok();
    let temp = dir.join(format!(".atlas-{}.tmp", Uuid::new_v4()));
    let write = (|| -> std::io::Result<()> {
        std::fs::write(&temp, body)?;
        if let Some(mode) = mode {
            std::fs::set_permissions(&temp, mode)?;
        }
        std::fs::rename(&temp, &target)
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}

/// The skill folder's `SKILL.md`, refused when either the folder or the file resolves
/// outside `root`.
fn checked_skill_file(dir: &Path, root: &Path) -> Result<PathBuf> {
    let escaped = |path: &Path| AtlasError::Invalid(format!("{} resolves outside {}", path.display(), root.display()));
    let file = dir.join(SKILL_FILE);
    match inside_root(dir, root) {
        Ok(Some(_)) => {}
        Ok(None) => return Err(escaped(dir)),
        Err(e) => return Err(e.into()),
    }
    match inside_root(&file, root) {
        Ok(Some(real)) => Ok(real),
        Ok(None) => Err(escaped(&file)),
        Err(e) => Err(e.into()),
    }
}

/// Reads at most [`frontmatter::MAX_SKILL_BYTES`] of a file as UTF-8, lossily: a skill
/// with a stray byte still lists rather than disappearing.
fn read_capped(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(frontmatter::MAX_SKILL_BYTES as u64).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The subdirectories of `path`, or `None` when it holds none to offer. A path that is
/// simply not there is silent, since most roots do not exist; anything else, a directory
/// the daemon may not traverse in particular, is a warning, so an unreadable root does
/// not look identical to an absent one.
fn read_dirs(path: &Path, found: &mut Found) -> Option<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            found.warnings.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
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

/// Whether the daemon user may write this file. On Unix this is the kernel's own answer
/// (`access(2)` with `W_OK`), not the readonly bit, which is true only when no write bit
/// is set for anyone and so calls a `0644` file owned by someone else writable. Nothing
/// is opened or created: `editable` is computed on every listing, and a listing must not
/// touch the files it describes.
#[cfg(unix)]
fn is_writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // Safety: `c_path` is a valid, NUL-terminated C string that outlives the call, and
    // `access` only reads it.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// Elsewhere, the readonly bit is the best answer the standard library offers.
#[cfg(not(unix))]
fn is_writable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| !m.permissions().readonly()).unwrap_or(false)
}
