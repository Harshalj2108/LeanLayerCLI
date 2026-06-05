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
    // Enhancement #3: llama-server subprocess management
    #[serde(default)]
    pub llama_server_path: Option<String>,
    #[serde(default = "default_gpu_layers")]
    pub gpu_layers: i32,
    #[serde(default = "default_ctx_size")]
    pub ctx_size: usize,
    #[serde(default = "default_port")]
    pub port: u16,

    // Cloud / multi-provider support
    /// Backend provider: "local", "openai", "gemini", "anthropic", "openrouter"
    #[serde(default = "default_api_provider")]
    pub api_provider: String,
    /// API key for cloud providers (can also use env vars: OPENAI_API_KEY, GEMINI_API_KEY, etc.)
    #[serde(default)]
    pub api_key: Option<String>,
    /// Model name for cloud APIs (e.g. "gpt-4o", "gemini-2.5-flash", "claude-sonnet-4-20250514")
    #[serde(default)]
    pub api_model: Option<String>,
    /// Optional API key for web search (reserved for future use)
    #[serde(default)]
    pub search_api_key: Option<String>,
}

fn default_gpu_layers() -> i32 { 99 }
fn default_ctx_size() -> usize { 8192 }
fn default_port() -> u16 { 8081 }
fn default_api_provider() -> String { "local".into() }

impl Default for Config {
    fn default() -> Self {
        Self {
            model_path: "/models/gemma-4-31b".into(),
            vault_path: default_vault_path(),
            max_context_nodes: 5,
            summarize_on_exit: true,
            backend_script: "airllm_backend.py".into(),
            llama_server_path: None,
            gpu_layers: default_gpu_layers(),
            ctx_size: default_ctx_size(),
            port: default_port(),
            api_provider: default_api_provider(),
            api_key: None,
            api_model: None,
            search_api_key: None,
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

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(cfg)?)?;
    Ok(())
}

pub fn print_config_path() {
    println!("{}", config_path().display());
}

/// Resolve API key: check config first, then environment variable
pub fn resolve_api_key(cfg: &Config) -> Option<String> {
    if let Some(key) = &cfg.api_key {
        if !key.is_empty() {
            return Some(key.clone());
        }
    }

    let env_var = match cfg.api_provider.as_str() {
        "openai" => "OPENAI_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        _ => return None,
    };

    std::env::var(env_var).ok()
}