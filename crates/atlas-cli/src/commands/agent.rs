//! `atlas agent ...`: list the library and read one agent as a bundle. The CLI has no
//! session, so it never adopts an agent; adoption is an MCP session concern
//! (`agent_use`). `atlas persona` is a hidden alias for one release (ATL-427).

use crate::remote::RemoteBackend;
use atlas_core::backend::PersonaBackend;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum AgentCmd {
    /// List every agent in the library
    List,
    /// Print one agent as a bundle: the agent plus the skills, workflows,
    /// practices and MCP servers it names, with a warning for each missing one
    Show {
        /// The agent's name or slug
        name: String,
    },
}

pub async fn run(cmd: AgentCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        AgentCmd::List => {
            let agents = backend.list_personas().await?;
            let rows: Vec<Vec<String>> = agents
                .iter()
                .map(|p| vec![p.name.clone(), p.slug.clone(), p.role.clone(), truncate(&p.summary, 60)])
                .collect();
            super::print_table(&["NAME", "SLUG", "ROLE", "SUMMARY"], &rows);
            Ok(())
        }
        AgentCmd::Show { name } => super::print_json(&backend.resolve_persona(&name, None).await?),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars().take(max.saturating_sub(1)).chain(std::iter::once('…')).collect()
}
