mod agent;
mod backend;
mod bootstrap;
mod cli;
mod config;
mod diff;
mod editor;
mod explorer;
mod integration;
mod memory;
mod plugins;
mod telemetry;
mod tui;
mod workflow;

use clap::Parser;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let cli = crate::cli::Cli::parse();

    let log_dir = app_data_dir().join("logs");
    let _guard = match crate::telemetry::logger::init(&log_dir) {
        Ok(guard) => Some(guard),
        Err(err) => {
            eprintln!("telemetry disabled: {}", err);
            None
        }
    };

    match cli.command {
        crate::cli::Commands::Config { edit } => {
            if edit {
                crate::tui::run(true)
                    .await
                    .map_err(|err| color_eyre::eyre::eyre!(err.to_string()))?;
            } else {
                crate::config::print_config_path();
            }
        }
        crate::cli::Commands::Vault { set_key } => {
            if let Some(secret) = set_key {
                let password = std::env::var("LEANLAYER_VAULT_PASSWORD")
                    .unwrap_or_else(|_| "local-development-password".to_string());
                let vault_path = app_data_dir().join("secure-vault.json");
                let vault = crate::memory::vault::SecureVault::init(vault_path, &password).await?;
                vault.store_secret("api_key", &secret).await?;
                println!("Secret stored in encrypted vault.");
            } else {
                println!("{}", app_data_dir().join("secure-vault.json").display());
            }
        }
        crate::cli::Commands::Sessions => {
            crate::memory::list_sessions()
                .map_err(|err| color_eyre::eyre::eyre!(err.to_string()))?;
        }
        crate::cli::Commands::Start { path } => {
            crate::tui::setup_panic_hook();

            let app_dir = app_data_dir();
            let python_env =
                crate::bootstrap::python_env::PythonEnvironment::ensure_initialized(&app_dir)
                    .await?;
            python_env
                .sync_requirements(&std::path::PathBuf::from("requirements.txt"))
                .await?;

            let app_config_path = app_dir.join("app-config.toml");
            let mut app_config =
                crate::config::AppConfig::load_or_default(&app_config_path).await?;
            if let Some(path) = path {
                app_config.workspace_root = path;
                app_config.save(&app_config_path).await?;
            }

            let _graph_memory = crate::memory::graph::GraphMemory::new();
            let password = std::env::var("LEANLAYER_VAULT_PASSWORD")
                .unwrap_or_else(|_| "local-development-password".to_string());
            let _vault =
                crate::memory::vault::SecureVault::init(app_dir.join("secure-vault.json"), &password)
                    .await?;
            let _workspace =
                crate::agent::workspace::Workspace::new(&app_config.workspace_root)?;

            let (_ui_tx, _ui_rx) = tokio::sync::mpsc::channel::<String>(64);
            let (_agent_tx, _agent_rx) = tokio::sync::mpsc::channel::<crate::tui::app::AgentMessage>(64);

            crate::tui::run(false)
                .await
                .map_err(|err| color_eyre::eyre::eyre!(err.to_string()))?;
        }
    }

    Ok(())
}

fn app_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
        .join("leanlayer-cli")
}

#[cfg(test)]
mod tests {
    struct BootOrchestrator {
        agent: tokio::sync::mpsc::Sender<String>,
        ui: tokio::sync::mpsc::Sender<crate::tui::app::AgentMessage>,
    }

    #[tokio::test]
    async fn dropping_mock_boot_orchestrator_closes_channels() {
        let (agent_tx, mut agent_rx) = tokio::sync::mpsc::channel::<String>(1);
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel::<crate::tui::app::AgentMessage>(1);
        let orchestrator = BootOrchestrator {
            agent: agent_tx,
            ui: ui_tx,
        };
        drop(orchestrator);
        assert!(agent_rx.recv().await.is_none());
        assert!(ui_rx.recv().await.is_none());
    }
}
