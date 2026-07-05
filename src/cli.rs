#[derive(clap::Parser)]
#[command(name = "airllm", about = "Chat with large LLMs in your terminal")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    /// Start an interactive chat session
    Chat,
    /// List past sessions
    Sessions,
    /// Manage configuration
    Config {
        #[arg(long, help = "Open interactive configuration editor")]
        edit: bool,
    },
    /// Manage encrypted vault
    Vault {
        #[arg(short, long)]
        set_key: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn cli_structure_is_valid() {
        crate::cli::Cli::command().debug_assert();
    }
}
