//! Skills: the `SKILL.md` folders coding agents already read, plus Atlas's own stored
//! skills, served as one list.
//!
//! Discovery ([`discover`]) walks the project's and the user's skill roots and the
//! installed plugin caches; [`repo`] holds the native ones in DuckDB. This module joins
//! the two, gates them against a project's disabled list, and is the only way in: a
//! skill is always addressed by id, and an id is only ever resolved against a listing
//! taken right now, so no caller-supplied string ever becomes a path.

pub mod discover;
pub mod frontmatter;
pub mod repo;

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::db::Db;
use crate::models::{Project, Skill, SkillList, SkillSummary, SkillUpdate};
use crate::projects::ProjectRepo;
use crate::{AtlasError, Result};

use discover::Discovered;
use repo::SkillRepo;

use Located::{Folder, Native};

/// Where a skill's text actually lives.
enum Located {
    /// A row in `skills`.
    Native(Uuid),
    /// A `SKILL.md` inside this folder.
    Folder(PathBuf),
}

struct Entry {
    summary: SkillSummary,
    located: Located,
}

/// Every skill that applies, native and discovered, with `enabled_here` filled in when
/// a project was given. Sorted by name, then by source, so two skills of the same name
/// from different places keep a stable order.
pub fn list_skills(db: &Db, project: Option<&Project>) -> Result<SkillList> {
    list_skills_in(db, project, &discover::skills_home()?)
}

/// [`list_skills`] against an explicit home directory. The public entry point resolves
/// the real one; tests pass a temp directory so they never read the user's own.
pub fn list_skills_in(db: &Db, project: Option<&Project>, home: &Path) -> Result<SkillList> {
    let (entries, warnings) = collect(db, project, home)?;
    Ok(SkillList { skills: entries.into_iter().map(|e| e.summary).collect(), warnings })
}

/// One skill with its text and the other files beside it.
pub fn get_skill(db: &Db, project: Option<&Project>, id: &str) -> Result<Skill> {
    get_skill_in(db, project, id, &discover::skills_home()?)
}

/// [`get_skill`] against an explicit home directory.
pub fn get_skill_in(db: &Db, project: Option<&Project>, id: &str, home: &Path) -> Result<Skill> {
    let entry = find(db, project, id, home)?;
    match entry.located {
        Native(uuid) => {
            let mut skill = SkillRepo::new(db).get(uuid)?;
            skill.summary = entry.summary;
            Ok(skill)
        }
        Folder(dir) => Ok(Skill {
            body: discover::read_body(&dir)?,
            files: discover::list_files(&dir),
            summary: entry.summary,
        }),
    }
}

/// Replaces a skill's text in place: the stored body for a native skill, the whole
/// `SKILL.md` for a discovered one. A skill Atlas cannot write is refused rather than
/// half-written, and a file write goes through a temp file and a rename so a failure
/// leaves the old text intact.
pub fn write_skill_body(db: &Db, project: Option<&Project>, id: &str, body: String, actor: &str) -> Result<Skill> {
    write_skill_body_in(db, project, id, body, actor, &discover::skills_home()?)
}

/// [`write_skill_body`] against an explicit home directory.
pub fn write_skill_body_in(db: &Db, project: Option<&Project>, id: &str, body: String, actor: &str, home: &Path) -> Result<Skill> {
    let entry = find(db, project, id, home)?;
    if !entry.summary.editable {
        return Err(AtlasError::Invalid(format!("skill '{id}' is not editable")));
    }
    match &entry.located {
        Native(uuid) => {
            SkillRepo::new(db).update(*uuid, &SkillUpdate { body: Some(body), ..Default::default() }, actor)?;
        }
        Folder(dir) => {
            discover::write_body(dir, &body)?;
            crate::memories::MemoryRepo::new(db).audit(
                actor,
                "skill_edit",
                "skill",
                None,
                serde_json::json!({"id": id, "path": dir.join(discover::SKILL_FILE).display().to_string()}),
            )?;
        }
    }
    get_skill_in(db, project, id, home)
}

/// Replaces the project's disabled-skill list. Every id has to name a skill that
/// applies here right now, so a typo is refused instead of being stored as a rule that
/// silently gates nothing.
pub fn set_project_skills_disabled(db: &Db, project_id: Uuid, ids: Vec<String>, actor: &str) -> Result<Project> {
    set_project_skills_disabled_in(db, project_id, ids, actor, &discover::skills_home()?)
}

/// [`set_project_skills_disabled`] against an explicit home directory.
pub fn set_project_skills_disabled_in(db: &Db, project_id: Uuid, ids: Vec<String>, actor: &str, home: &Path) -> Result<Project> {
    let projects = ProjectRepo::new(db);
    let project = projects.get(project_id)?;
    let known = list_skills_in(db, Some(&project), home)?;
    for id in &ids {
        if !known.skills.iter().any(|s| &s.id == id) {
            return Err(AtlasError::Invalid(format!("unknown skill id '{id}'")));
        }
    }
    projects.set_skills_disabled(project_id, &ids, actor)
}

/// The one entry whose id matches, resolved against a listing taken right now.
fn find(db: &Db, project: Option<&Project>, id: &str, home: &Path) -> Result<Entry> {
    let (entries, _) = collect(db, project, home)?;
    entries.into_iter().find(|e| e.summary.id == id).ok_or_else(|| AtlasError::NotFound(format!("skill {id}")))
}

/// Native rows plus discovered folders, in listing order. A project narrows nothing:
/// its own skills join the global ones, since both apply while working in it.
fn collect(db: &Db, project: Option<&Project>, home: &Path) -> Result<(Vec<Entry>, Vec<String>)> {
    let mut entries: Vec<Entry> = Vec::new();
    for stored in SkillRepo::new(db).list(project.map(|p| p.id))? {
        let uuid: Uuid = stored.summary.id.parse().map_err(|e| AtlasError::Other(format!("stored skill id {}: {e}", stored.summary.id)))?;
        entries.push(Entry { located: Native(uuid), summary: stored.summary });
    }

    let mut found = discover::discover_global_in(home);
    if let Some(p) = project {
        let project_found = discover::discover_project(Path::new(&p.root_path), p.id);
        found.skills.extend(project_found.skills);
        found.warnings.extend(project_found.warnings);
    }
    entries.extend(found.skills.into_iter().map(|d: Discovered| Entry { summary: d.summary, located: Folder(d.dir) }));

    if let Some(p) = project {
        for entry in &mut entries {
            entry.summary.enabled_here = Some(!p.skills_disabled.contains(&entry.summary.id));
        }
    }
    entries.sort_by(|a, b| a.summary.name.cmp(&b.summary.name).then_with(|| a.summary.source.as_str().cmp(b.summary.source.as_str())));
    Ok((entries, found.warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MemoryScope, NewSkill, SkillSource};

    /// Writes a `SKILL.md` folder under `root`.
    fn skill_at(root: &Path, name: &str, text: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
        dir
    }

    /// A home with one skill in each of its two user roots, and a plugin installed in
    /// two versions plus a half-finished clone.
    fn populated_home(home: &Path) {
        skill_at(&home.join(".claude/skills"), "user-claude", "---\nname: user-claude\ndescription: A user skill.\n---\n\nBody.\n");
        skill_at(&home.join(".codex/skills"), "user-codex", "---\nname: user-codex\ndescription: A codex skill.\n---\n");

        let plugin = home.join(".claude/plugins/cache/acme/tools");
        let old = plugin.join("aaaa1111/skills");
        skill_at(&old, "packaged", "---\nname: packaged\ndescription: The old version.\n---\n");
        // The newest version by modification time wins.
        let new = plugin.join("bbbb2222/skills");
        skill_at(&new, "packaged", "---\nname: packaged\ndescription: The new version.\n---\n");
        let now = std::time::SystemTime::now();
        set_dir_mtime(&plugin.join("aaaa1111"), now - std::time::Duration::from_secs(600));
        set_dir_mtime(&plugin.join("bbbb2222"), now);
        // Neither of these is an installed version.
        skill_at(&plugin.join("cccc3333.clone/skills"), "cloned", "---\nname: cloned\n---\n");
        skill_at(&plugin.join("temp_dddd/skills"), "scratch", "---\nname: scratch\n---\n");
    }

    /// Sets a directory's modification time, which is what picks the installed plugin
    /// version discovery reads.
    fn set_dir_mtime(dir: &Path, when: std::time::SystemTime) {
        std::fs::File::open(dir).unwrap().set_modified(when).unwrap();
    }

    fn project_at(root: &Path, db: &Db) -> Project {
        skill_at(&root.join(".claude/skills"), "proj-claude", "---\nname: proj-claude\ndescription: A project skill.\n---\n");
        skill_at(&root.join(".codex/skills"), "proj-codex", "---\nname: proj-codex\ndescription: Another.\n---\n");
        let detected = crate::projects::Detected { root: root.to_path_buf(), remote: None };
        ProjectRepo::new(db).upsert(&detected, None, "t").unwrap()
    }

    #[test]
    fn discovers_every_root_and_prefers_the_newest_plugin_version() {
        let home = tempfile::tempdir().unwrap();
        populated_home(home.path());
        let db = Db::open_in_memory().unwrap();
        let list = list_skills_in(&db, None, home.path()).unwrap();

        let ids: Vec<&str> = list.skills.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["plugin:acme/tools/packaged", "claude-user:user-claude", "codex-user:user-codex"], "sorted by name: {ids:?}");
        let packaged = &list.skills[0];
        assert_eq!(packaged.description, "The new version.", "the newest version directory wins");
        assert_eq!(packaged.plugin.as_deref(), Some("acme/tools"));
        assert_eq!(packaged.source, SkillSource::Plugin);
        assert_eq!(packaged.scope, MemoryScope::Global);
        assert!(packaged.editable, "a writable SKILL.md is editable");
        assert!(packaged.updated_at.is_some());
        assert!(list.skills.iter().all(|s| s.enabled_here.is_none()), "no project was given");
        assert!(list.warnings.is_empty(), "{:?}", list.warnings);
    }

    /// A `.clone` or `temp_` directory is a half-finished install, not a version.
    #[test]
    fn scratch_plugin_directories_are_skipped() {
        let home = tempfile::tempdir().unwrap();
        populated_home(home.path());
        let db = Db::open_in_memory().unwrap();
        let list = list_skills_in(&db, None, home.path()).unwrap();
        assert!(list.skills.iter().all(|s| s.name != "cloned" && s.name != "scratch"), "{:?}", list.skills);
    }

    /// A symlinked skill folder pointing out of its root is not this root's skill.
    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_ignored() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        skill_at(outside.path(), "escapee", "---\nname: escapee\n---\n");
        let root = home.path().join(".claude/skills");
        skill_at(&root, "real", "---\nname: real\n---\n");
        std::os::unix::fs::symlink(outside.path().join("escapee"), root.join("linked")).unwrap();

        let db = Db::open_in_memory().unwrap();
        let list = list_skills_in(&db, None, home.path()).unwrap();
        let ids: Vec<&str> = list.skills.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["claude-user:real"], "{ids:?}");
    }

    /// An unreadable `SKILL.md` is skipped and named in the warnings, and the skills
    /// beside it still list.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_skill_is_counted_not_fatal() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".claude/skills");
        skill_at(&root, "readable", "---\nname: readable\n---\n");
        let locked = skill_at(&root, "locked", "---\nname: locked\n---\n");
        std::fs::set_permissions(locked.join("SKILL.md"), std::fs::Permissions::from_mode(0o000)).unwrap();

        let db = Db::open_in_memory().unwrap();
        let list = list_skills_in(&db, None, home.path()).unwrap();
        assert_eq!(list.skills.len(), 1, "{:?}", list.skills);
        assert_eq!(list.warnings.len(), 1, "{:?}", list.warnings);
        assert!(list.warnings[0].contains("locked"), "{:?}", list.warnings);

        std::fs::set_permissions(locked.join("SKILL.md"), std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    /// A project's own roots join the global ones, and its disabled list shows up as
    /// `enabled_here` without hiding anything.
    #[test]
    fn a_project_widens_the_list_and_gates_it() {
        let home = tempfile::tempdir().unwrap();
        populated_home(home.path());
        let root = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let project = project_at(root.path(), &db);
        SkillRepo::new(&db).create(&NewSkill { project_id: None, name: "native-one".into(), description: "d".into(), body: "b".into() }, "t").unwrap();

        let list = list_skills_in(&db, Some(&project), home.path()).unwrap();
        assert_eq!(list.skills.len(), 6, "3 global, 2 project, 1 native: {:?}", list.skills);
        assert!(list.skills.iter().all(|s| s.enabled_here == Some(true)));

        let project = set_project_skills_disabled_in(&db, project.id, vec!["claude-project:proj-claude".into()], "t", home.path()).unwrap();
        assert_eq!(project.skills_disabled, vec!["claude-project:proj-claude".to_string()]);
        let list = list_skills_in(&db, Some(&project), home.path()).unwrap();
        let gated = list.skills.iter().find(|s| s.id == "claude-project:proj-claude").unwrap();
        assert_eq!(gated.enabled_here, Some(false), "a disabled skill still lists, switched off");
        assert_eq!(list.skills.iter().filter(|s| s.enabled_here == Some(false)).count(), 1);
    }

    #[test]
    fn an_unknown_id_cannot_be_disabled() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let project = project_at(root.path(), &db);
        let err = set_project_skills_disabled_in(&db, project.id, vec!["claude-user:nope".into()], "t", home.path()).unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(ref m) if m.contains("nope")), "{err}");
        assert!(ProjectRepo::new(&db).get(project.id).unwrap().skills_disabled.is_empty(), "nothing is stored on a refusal");
    }

    #[test]
    fn get_reads_the_whole_file_and_lists_its_other_files() {
        let home = tempfile::tempdir().unwrap();
        let text = "---\nname: user-claude\ndescription: A user skill.\n---\n\nBody.\n";
        let dir = skill_at(&home.path().join(".claude/skills"), "user-claude", text);
        std::fs::create_dir_all(dir.join("references")).unwrap();
        std::fs::write(dir.join("references/notes.md"), "notes").unwrap();
        std::fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();

        let db = Db::open_in_memory().unwrap();
        let skill = get_skill_in(&db, None, "claude-user:user-claude", home.path()).unwrap();
        assert_eq!(skill.body, text, "the frontmatter comes back with the body");
        assert_eq!(skill.files, vec!["references/notes.md".to_string(), "run.sh".to_string()]);

        let missing = get_skill_in(&db, None, "claude-user:nope", home.path()).unwrap_err();
        assert!(matches!(missing, AtlasError::NotFound(_)), "{missing}");
    }

    /// A file edit rewrites `SKILL.md` in place, leaves no temp file behind, and
    /// records an audit row naming the path.
    #[test]
    fn writing_a_file_skill_is_atomic_and_audited() {
        let home = tempfile::tempdir().unwrap();
        let dir = skill_at(&home.path().join(".claude/skills"), "user-claude", "---\nname: user-claude\n---\n\nOld.\n");
        let db = Db::open_in_memory().unwrap();

        let updated = "---\nname: user-claude\ndescription: Now described.\n---\n\nNew.\n";
        let skill = write_skill_body_in(&db, None, "claude-user:user-claude", updated.into(), "t", home.path()).unwrap();
        assert_eq!(skill.body, updated);
        assert_eq!(skill.summary.description, "Now described.", "the rewritten frontmatter is what the listing shows");
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(), updated);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(leftovers, vec!["SKILL.md".to_string()], "no temp file survives the rename");

        let audited: i64 = db
            .with_conn(|c| Ok(c.query_row("select count(*) from audit where action = 'skill_edit'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(audited, 1);
    }

    #[test]
    fn writing_a_native_skill_updates_the_row() {
        let home = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let native = SkillRepo::new(&db)
            .create(&NewSkill { project_id: None, name: "native-one".into(), description: "d".into(), body: "old".into() }, "t")
            .unwrap();
        let skill = write_skill_body_in(&db, None, &native.summary.id, "new".into(), "t", home.path()).unwrap();
        assert_eq!(skill.body, "new");
        assert_eq!(SkillRepo::new(&db).get(native.summary.id.parse().unwrap()).unwrap().body, "new");
    }

    /// A read-only `SKILL.md` is refused rather than half-written.
    #[cfg(unix)]
    #[test]
    fn a_read_only_skill_refuses_a_write() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let dir = skill_at(&home.path().join(".claude/skills"), "locked", "---\nname: locked\n---\n\nOld.\n");
        std::fs::set_permissions(dir.join("SKILL.md"), std::fs::Permissions::from_mode(0o444)).unwrap();

        let db = Db::open_in_memory().unwrap();
        let list = list_skills_in(&db, None, home.path()).unwrap();
        assert!(!list.skills[0].editable, "a read-only SKILL.md is not editable");
        let err = write_skill_body_in(&db, None, "claude-user:locked", "new".into(), "t", home.path()).unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(ref m) if m.contains("not editable")), "{err}");
        assert!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap().contains("Old."), "the file is untouched");

        std::fs::set_permissions(dir.join("SKILL.md"), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}
