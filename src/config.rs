use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub model_path: String,
    pub vault_path: String,
    pub max_context_nodes: usize,
    pub summarize_on_exit: bool,
    pub backend_script: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_path: "/models/gemma-4-31b".into(),
            vault_path: default_vault_path(),
            max_context_nodes: 5,
            summarize_on_exit: true,
            backend_script: "airllm_backend.py".into(),
        }
    }
}

fn default_vault_path() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("airllm-vault")
        .to_string_lossy()
        .into()
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("airllm")
        .join("config.toml")
}

pub fn load() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let contents = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&contents)?)
    } else {
        // Write defaults on first run
        let cfg = Config::default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(&cfg)?)?;
        Ok(cfg)
    }
}

pub fn print_config_path() {
    println!("{}", config_path().display());
}