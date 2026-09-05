//! `atlas persona ...`: list the library and read one persona as a bundle. The CLI
//! has no session, so it never adopts a persona; adoption is an MCP session concern.

use crate::remote::RemoteBackend;
use atlas_core::backend::PersonaBackend;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum PersonaCmd {
    /// List every persona in the library
    List,
    /// Print one persona as a bundle: the persona plus the skills, workflows,
    /// practices and MCP servers it names, with a warning for each missing one
    Show {
        /// The persona's name or slug
        name: String,
    },
}

pub async fn run(cmd: PersonaCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        PersonaCmd::List => {
            let personas = backend.list_personas().await?;
            let rows: Vec<Vec<String>> = personas
                .iter()
                .map(|p| vec![p.name.clone(), p.slug.clone(), p.role.clone(), truncate(&p.summary, 60)])
                .collect();
            super::print_table(&["NAME", "SLUG", "ROLE", "SUMMARY"], &rows);
            Ok(())
        }
        PersonaCmd::Show { name } => super::print_json(&backend.resolve_persona(&name, None).await?),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars().take(max.saturating_sub(1)).chain(std::iter::once('…')).collect()
}
