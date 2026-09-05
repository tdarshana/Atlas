//! `atlas skill ...`: list, read and gate the skills agents can use here.

use crate::remote::RemoteBackend;
use atlas_core::backend::{ProjectBackend, SkillBackend};
use atlas_core::models::*;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum SkillCmd {
    /// List the skills that apply, global or for one project
    List {
        /// A project root path or board key; without it, only the global skills are listed
        #[arg(long)]
        project: Option<String>,
    },
    /// Print one skill's full text
    Show {
        /// A skill id from `skill list`
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Switch a skill off for a project
    Disable {
        id: String,
        #[arg(long)]
        project: String,
    },
    /// Switch a skill back on for a project
    Enable {
        id: String,
        #[arg(long)]
        project: String,
    },
}

pub async fn run(cmd: SkillCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        SkillCmd::List { project } => {
            let project = resolve_project(project.as_deref(), backend).await?;
            let list = backend.list_skills(project.map(|p| p.id)).await?;
            let rows: Vec<Vec<String>> = list
                .skills
                .iter()
                .map(|s| {
                    vec![
                        s.id.clone(),
                        s.source.to_string(),
                        s.name.clone(),
                        match s.enabled_here {
                            Some(false) => "off".into(),
                            Some(true) => "on".into(),
                            None => "-".into(),
                        },
                        truncate(&s.description, 60),
                    ]
                })
                .collect();
            super::print_table(&["ID", "SOURCE", "NAME", "HERE", "DESCRIPTION"], &rows);
            for warning in &list.warnings {
                eprintln!("warning: {warning}");
            }
        }
        SkillCmd::Show { id, project } => {
            let project = resolve_project(project.as_deref(), backend).await?;
            let skill = backend.get_skill(project.map(|p| p.id), &id).await?;
            println!("{}", skill.body);
        }
        SkillCmd::Disable { id, project } => set_enabled(&id, &project, false, backend).await?,
        SkillCmd::Enable { id, project } => set_enabled(&id, &project, true, backend).await?,
    }
    Ok(())
}

/// Adds or removes one id from the project's disabled list and stores the whole list
/// back, which is what the route takes.
async fn set_enabled(id: &str, project: &str, enabled: bool, backend: &RemoteBackend) -> anyhow::Result<()> {
    let project = resolve_project(Some(project), backend).await?.expect("a named project always resolves or fails");
    let mut disabled = project.skills_disabled.clone();
    disabled.retain(|d| d != id);
    if !enabled {
        disabled.push(id.to_string());
    }
    let updated = backend.set_project_skills_disabled(project.id, disabled, &backend.actor).await?;
    println!("{} skills disabled in {}", updated.skills_disabled.len(), updated.name);
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars().take(max.saturating_sub(1)).chain(std::iter::once('…')).collect()
}

/// The project a skill command works on. Unlike `atlas framework`, no argument is not
/// an error: the global skills are a real answer, so `None` means "the global list".
async fn resolve_project(arg: Option<&str>, backend: &RemoteBackend) -> anyhow::Result<Option<Project>> {
    let Some(arg) = arg else { return Ok(None) };
    let projects = backend.list_projects().await?;
    if let Some(p) = projects.iter().find(|p| p.board_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(arg))) {
        return Ok(Some(p.clone()));
    }
    let abs = super::abs_path(Some(PathBuf::from(arg)))?;
    let abs = abs.to_string_lossy().to_string();
    projects
        .into_iter()
        .find(|p| p.root_path == arg || p.root_path == abs)
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("no project with root or board key '{arg}'"))
}
