use crate::remote::RemoteBackend;
use atlas_core::backend::LibraryBackend;
use atlas_core::models::NewAgent;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AgentCmd {
    /// List every agent
    List,
    /// Print one agent as JSON
    Show { name: String },
    /// Create or replace an agent
    Save {
        name: String,
        #[arg(long)]
        description: String,
        /// File holding the agent's instructions; `-` reads standard input
        #[arg(long)]
        instructions_file: PathBuf,
        #[arg(long)]
        model: Option<String>,
        /// Tool the agent may use; repeatable
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// Tag to file the agent under; repeatable
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Remove an agent
    Delete { name: String },
}

pub async fn run(cmd: AgentCmd, backend: &RemoteBackend) -> anyhow::Result<()> {
    match cmd {
        AgentCmd::List => {
            let agents = backend.list_agents().await?;
            let rows: Vec<Vec<String>> = agents
                .iter()
                .map(|a| vec![a.name.clone(), a.description.clone(), a.model_hint.clone().unwrap_or_default(), a.tags.join(",")])
                .collect();
            super::print_table(&["NAME", "DESCRIPTION", "MODEL", "TAGS"], &rows);
            Ok(())
        }
        AgentCmd::Show { name } => super::print_json(&backend.get_agent(&name).await?),
        AgentCmd::Save { name, description, instructions_file, model, tools, tags } => {
            let instructions = super::read_source(&instructions_file)?;
            let agent = NewAgent { name, description, instructions, model_hint: model, tools, tags };
            super::print_json(&backend.save_agent(agent, "cli").await?)
        }
        AgentCmd::Delete { name } => {
            backend.delete_agent(&name, "cli").await?;
            println!("deleted agent {name}");
            Ok(())
        }
    }
}
