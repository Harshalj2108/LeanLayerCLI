mod agent;
mod backend;
mod config;
mod memory;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "airllm", about = "Chat with large LLMs in your terminal")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start an interactive chat session
    Chat,
    /// List past sessions
    Sessions,
    /// Manage configuration
    Config {
        #[arg(long, help = "Open interactive configuration editor")]
        edit: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Chat) {
        Command::Chat => tui::run(false).await?,
        Command::Sessions => memory::list_sessions()?,
        Command::Config { edit } => {
            if edit {
                tui::run(true).await?;
            } else {
                config::print_config_path();
            }
        }
    }

    Ok(())
}