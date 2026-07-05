#![allow(dead_code, unused_imports, unused_variables, unused_mut)]
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

mod telemetry;
mod tui;
mod workflow;
mod plugins;

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

    match cli.command.unwrap_or(crate::cli::Commands::Chat) {
        crate::cli::Commands::Chat => {
            crate::tui::run(false)
                .await
                .map_err(|err| color_eyre::eyre::eyre!(err.to_string()))?;
        }
        crate::cli::Commands::Sessions => {
            crate::memory::list_sessions()
                .map_err(|err| color_eyre::eyre::eyre!(err.to_string()))?;
        }
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
                let vault =
                    crate::memory::vault::SecureVault::init(vault_path, &password).await?;
                vault.store_secret("api_key", &secret).await?;
                println!("Secret stored in encrypted vault.");
            } else {
                println!("{}", app_data_dir().join("secure-vault.json").display());
            }
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
