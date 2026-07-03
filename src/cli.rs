#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    Start {
        #[arg(short, long)]
        path: Option<std::path::PathBuf>,
    },
    Config {
        #[arg(short, long)]
        edit: bool,
    },
    Vault {
        #[arg(short, long)]
        set_key: Option<String>,
    },
    Sessions,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn cli_structure_is_valid() {
        crate::cli::Cli::command().debug_assert();
    }
}
