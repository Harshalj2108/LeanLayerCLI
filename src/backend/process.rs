use anyhow::{Result, Context};
use std::process::{Child, Command};
use std::time::Duration;

use tokio::sync::mpsc;
use futures_util::StreamExt;

use super::protocol::{BackendMessage, ChatMessage};
use super::ratelimit::RateLimiterHandle;
use crate::config::Config;
use crate::tui::app::AppMode;

pub struct Backend {
    pub base_url: String,
    pub(crate) child: Option<Child>,
    pub api_provider: String,
    pub api_key: Option<String>,
    pub api_model: Option<String>,
    pub client: reqwest::Client,
    pub rate_limiter: RateLimiterHandle,
    pub actual_ctx_size: usize,
}

impl Backend {
    pub fn get_rate_limiter(&self) -> RateLimiterHandle {
        self.rate_limiter.clone()
    }
    pub fn spawn(cfg: &Config) -> Result<Self> {
        let api_provider = cfg.api_provider.clone();
        let api_key = crate::config::resolve_api_key(cfg);
        let api_model = cfg.api_model.clone();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let rate_limiter = RateLimiterHandle::new(40);

        // For cloud providers, no local server needed
        if api_provider != "local" {
            if api_key.is_none() {
                anyhow::bail!(
                    "API key required for provider '{}'. Set it in config.toml or via environment variable.",
                    api_provider
                );
            }
            let base_url = match api_provider.as_str() {
                "openai" => "https://api.openai.com".to_string(),
                "gemini" => "https://generativelanguage.googleapis.com".to_string(),
                "anthropic" => "https://api.anthropic.com".to_string(),
                "openrouter" => "https://openrouter.ai/api".to_string(),
                "nvidia" => "https://integrate.api.nvidia.com".to_string(),
                other => anyhow::bail!("Unknown API provider: {}", other),
            };
            let actual_ctx_size = match api_provider.as_str() {
                "gemini" => 2000000,
                "anthropic" => 200000,
                _ => 128000,
            };
            return Ok(Self {
                base_url,
                child: None,
                api_provider,
                api_key,
                api_model,
                client,
                rate_limiter,
                actual_ctx_size,
            });
        }

        // Local provider: llama-server
        let base_url = format!("http://127.0.0.1:{}", cfg.port);

        // Health check using reqwest::blocking for simplicity during sync init
        let server_running = {
            let health_url = format!("{}/health", base_url);
            let client_clone = client.clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(_) => return false,
                };
                rt.block_on(async {
                    match client_clone
                        .get(&health_url)
                        .timeout(Duration::from_secs(2))
                        .send()
                        .await
                    {
                        Ok(resp) => resp.status().is_success(),
                        Err(_) => false,
                    }
                })
            })
            .join()
            .unwrap_or(false)
        };

        if server_running {
            let actual_ctx_size = Self::fetch_ctx_size(&client, &base_url).unwrap_or(cfg.ctx_size);
            return Ok(Self {
                base_url,
                child: None,
                api_provider,
                api_key,
                api_model,
                client,
                rate_limiter,
                actual_ctx_size,
            });
        }

        // Try to auto-launch llama-server if path is configured
        if let Some(server_path) = &cfg.llama_server_path {
            let path = std::path::Path::new(server_path);
            if !path.exists() {
                anyhow::bail!(
                    "llama-server not found at: {}. Start it manually or fix llama_server_path in config.",
                    server_path
                );
            }

            let mut vram_gb = 0;
            if let Ok(output) = std::process::Command::new("nvidia-smi").arg("--query-gpu=memory.total").arg("--format=csv,noheader,nounits").output() {
                if output.status.success() {
                    let s = String::from_utf8_lossy(&output.stdout);
                    if let Ok(mb) = s.trim().parse::<u64>() {
                        vram_gb = mb / 1024;
                    }
                }
            }
            
            let mut sys = sysinfo::System::new();
            sys.refresh_memory();
            let total_ram_gb = sys.total_memory() / (1024 * 1024 * 1024);

            let computed_gpu_layers = if cfg.gpu_layers == 99 {
                if vram_gb >= 16 { 99 }
                else if vram_gb >= 8 { 32 }
                else if vram_gb > 0 { 16 }
                else if total_ram_gb > 16 { 0 } // Fallback to CPU if no GPU
                else { 0 }
            } else {
                cfg.gpu_layers
            };

            eprintln!("[airllm] Starting llama-server from: {}", server_path);
            eprintln!("[airllm]   model: {}", cfg.model_path);
            eprintln!("[airllm]   port: {}, gpu_layers: {} (detected VRAM: {}GB, RAM: {}GB), ctx_size: {}", 
                      cfg.port, computed_gpu_layers, vram_gb, total_ram_gb, cfg.ctx_size);

            let child = Command::new(server_path)
                .arg("-m")
                .arg(&cfg.model_path)
                .arg("--port")
                .arg(cfg.port.to_string())
                .arg("-ngl")
                .arg(computed_gpu_layers.to_string())
                .arg("--ctx-size")
                .arg(cfg.ctx_size.to_string())
                .arg("--no-warmup")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("Failed to launch llama-server subprocess")?;

            // Wait for server to become healthy (up to 60 seconds)
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(60);
            let client_clone = client.clone();
            loop {
                if start.elapsed() > timeout {
                    anyhow::bail!("llama-server failed to start within 60 seconds");
                }
                
                let health_url = format!("{}/health", base_url);
                let client_inner = client_clone.clone();
                let is_healthy = std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(_) => return false,
                    };
                    rt.block_on(async {
                        match client_inner
                            .get(&health_url)
                            .timeout(Duration::from_secs(1))
                            .send()
                            .await
                        {
                            Ok(resp) => resp.status().is_success(),
                            Err(_) => false,
                        }
                    })
                })
                .join()
                .unwrap_or(false);

                if is_healthy {
                    eprintln!("[airllm] llama-server is ready ({:.1}s)", start.elapsed().as_secs_f64());
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            let actual_ctx_size = Self::fetch_ctx_size(&client, &base_url).unwrap_or(cfg.ctx_size);
            return Ok(Self {
                base_url,
                child: Some(child),
                api_provider,
                api_key,
                api_model,
                client,
                rate_limiter,
                actual_ctx_size,
            });
        }

        anyhow::bail!(
            "llama-server not running on port {}. Either:\n  1. Start it manually: llama-server -m <model> --port {}\n  2. Set 'llama_server_path' in config.toml for auto-launch\n  3. Use a cloud provider: set api_provider to 'openai', 'gemini', 'anthropic', 'openrouter', or 'nvidia'",
            cfg.port, cfg.port
        )
    }

    fn fetch_ctx_size(client: &reqwest::Client, base_url: &str) -> Option<usize> {
        let props_url = format!("{}/props", base_url);
        let client_clone = client.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(_) => return None,
            };
            rt.block_on(async {
                match client_clone.get(&props_url).timeout(Duration::from_secs(2)).send().await {
                    Ok(resp) => {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if let Some(n_ctx) = json.get("default_generation_settings").and_then(|s| s.get("n_ctx")).and_then(|n| n.as_u64()) {
                                return Some(n_ctx as usize);
                            }
                        }
                        None
                    },
                    Err(_) => None,
                }
            })
        })
        .join()
        .unwrap_or(None)
    }

    /// Gracefully terminate the child llama-server process if we spawned it
    pub fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            eprintln!("[airllm] Shutting down llama-server...");
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn send_generate(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        mode: AppMode,
    ) {
        match self.api_provider.as_str() {
            "local" => self.send_local(messages, tx, mode),
            "openai" | "openrouter" | "nvidia" => self.send_openai_compat(messages, tx, mode),
            "gemini" => self.send_gemini(messages, tx, mode),
            "anthropic" => self.send_anthropic(messages, tx, mode),
            _ => {
                tx.send(BackendMessage::Error {
                    message: format!("Unknown API provider: {}", self.api_provider),
                }).ok();
            }
        }
    }

    fn send_local(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        mode: AppMode,
    ) {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let serialized_messages = serialize_messages_openai(&messages);

        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let temperature = mode.temperature();
        let thinking = mode.thinking_enabled();
        tokio::spawn(async move {
            rate_limiter.check_and_wait("local").await;

            let body = serde_json::json!({
                "model": "local",
                "messages": serialized_messages,
                "stream": true,
                "temperature": temperature,
                "top_p": 0.95,
                "chat_template_kwargs": {
                    "enable_thinking": thinking
                }
            });

            stream_openai_response(&client, &url, body, None, tx).await;
        });
    }

    fn send_openai_compat(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        mode: AppMode,
    ) {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let model = self.api_model.clone().unwrap_or_else(|| "gpt-4o".into());
        let api_key = self.api_key.clone();
        let serialized_messages = serialize_messages_openai(&messages);

        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let provider = self.api_provider.clone();
        let temperature = mode.temperature();
        tokio::spawn(async move {
            rate_limiter.check_and_wait(&provider).await;

            let body = serde_json::json!({
                "model": model,
                "messages": serialized_messages,
                "stream": true,
                "temperature": temperature,
            });

            let api_key_ref = api_key.as_deref();
            stream_openai_response(&client, &url, body, api_key_ref, tx).await;
        });
    }

    fn send_gemini(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        mode: AppMode,
    ) {
        let model = self.api_model.clone().unwrap_or_else(|| "gemini-2.5-flash".into());
        let api_key = self.api_key.clone();
        let serialized_messages = serialize_messages_openai(&messages);

        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let temperature = mode.temperature();
        tokio::spawn(async move {
            rate_limiter.check_and_wait("gemini").await;

            let url = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";

            let body = serde_json::json!({
                "model": model,
                "messages": serialized_messages,
                "stream": true,
                "temperature": temperature,
            });

            stream_openai_response(&client, url, body, api_key.as_deref(), tx).await;
        });
    }

    fn send_anthropic(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        mode: AppMode,
    ) {
        let model = self.api_model.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        let api_key = self.api_key.clone().unwrap_or_default();
        let url = format!("{}/v1/messages", self.base_url);

        // Anthropic uses a separate system message
        let system_text: String = messages.iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");

        let mut anthropic_messages = Vec::new();
        for msg in &messages {
            if msg.role == "system" { continue; }
            let role = if msg.role == "assistant" { "assistant" } else { "user" };

            if msg.has_images() {
                let mut parts = Vec::new();
                if let Some(images) = &msg.images {
                    for img_data in images {
                        if let Some(comma_idx) = img_data.find(',') {
                            let media_type = img_data[5..comma_idx]
                                .split(';')
                                .next()
                                .unwrap_or("image/png");
                            let b64 = &img_data[comma_idx + 1..];
                            parts.push(serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": b64
                                }
                            }));
                        }
                    }
                }
                parts.push(serde_json::json!({
                    "type": "text",
                    "text": msg.content
                }));
                anthropic_messages.push(serde_json::json!({
                    "role": role,
                    "content": parts
                }));
            } else {
                anthropic_messages.push(serde_json::json!({
                    "role": role,
                    "content": msg.content
                }));
            }
        }

        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let temperature = mode.temperature();
        tokio::spawn(async move {
            rate_limiter.check_and_wait("anthropic").await;

            let mut body = serde_json::json!({
                "model": model,
                "messages": anthropic_messages,
                "stream": true,
                "max_tokens": 8192,
                "temperature": temperature,
            });
            if !system_text.is_empty() {
                body["system"] = serde_json::json!(system_text);
            }

            let resp = client.post(&url)
                .header("Content-Type", "application/json")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(response) => {
                    if let Err(e) = stream_anthropic_response(response, tx.clone()).await {
                        tx.send(BackendMessage::Error {
                            message: format!("Anthropic streaming error: {}", e),
                        }).ok();
                    }
                }
                Err(e) => {
                    tx.send(BackendMessage::Error {
                        message: format!("Anthropic API error: {}", e),
                    }).ok();
                }
            }
        });
    }
}

/// Serialize messages into OpenAI-compatible format, handling multimodal content
fn serialize_messages_openai(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();

    // Merge all system messages into one at the beginning
    let system_text = messages.iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    if !system_text.is_empty() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system_text
        }));
    }

    for msg in messages {
        if msg.role == "system" { continue; }

        if msg.has_images() {
            let mut parts = Vec::new();
            if let Some(images) = &msg.images {
                for img_data in images {
                    parts.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": img_data }
                    }));
                }
            }
            parts.push(serde_json::json!({
                "type": "text",
                "text": msg.content
            }));
            out.push(serde_json::json!({
                "role": msg.role,
                "content": parts
            }));
        } else {
            out.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
    }

    out
}

/// Shared SSE streaming parser for OpenAI-compatible endpoints using reqwest async
async fn stream_openai_response(
    client: &reqwest::Client,
    url: &str,
    body: serde_json::Value,
    api_key: Option<&str>,
    tx: mpsc::UnboundedSender<BackendMessage>,
) {
    let mut request = client.post(url)
        .header("Content-Type", "application/json");

    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    let response = request.json(&body).send().await;

    match response {
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                tx.send(BackendMessage::Error {
                    message: format!("API error ({}): {}", status, body),
                }).ok();
                return;
            }
            if let Err(e) = stream_openai_chunks(resp, tx.clone()).await {
                tx.send(BackendMessage::Error {
                    message: format!("OpenAI streaming error: {}", e),
                }).ok();
            }
        }
        Err(e) => {
            tx.send(BackendMessage::Error {
                message: format!("OpenAI API error: {}", e),
            }).ok();
        }
    }
}

/// Parse streaming chunks from an OpenAI-compatible SSE response
async fn stream_openai_chunks(
    resp: reqwest::Response,
    tx: mpsc::UnboundedSender<BackendMessage>,
) -> Result<(), reqwest::Error> {
    let mut stream = resp.bytes_stream();
    let mut line_buf = String::new();
    let mut in_think = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let s = String::from_utf8_lossy(&chunk);
        
        for ch in s.chars() {
            if ch == '\n' {
                if !line_buf.starts_with("data:") {
                    line_buf.clear();
                    continue;
                }
                let data = line_buf[5..].trim();
                if data == "[DONE]" {
                    tx.send(BackendMessage::Done).ok();
                    return Ok(());
                }
                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = chunk["choices"][0]["delta"].as_object() {
                        let mut text = String::new();
                        if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                            text.push_str(reasoning);
                        }
                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                            let mut c = content.to_string();
                            if c.contains("<think>") {
                                in_think = true;
                                c = c.replace("<think>", "");
                            }
                            if c.contains("</think>") {
                                in_think = false;
                                c = c.replace("</think>", "");
                            }
                            if !in_think && !c.is_empty() {
                                text.push_str(&c);
                            }
                        }
                        if !text.is_empty() {
                            tx.send(BackendMessage::Token {
                                content: text,
                            }).ok();
                        }
                    }
                }
                line_buf.clear();
            } else {
                line_buf.push(ch);
            }
        }
    }
    Ok(())
}

/// Parse Anthropic's SSE streaming response format
async fn stream_anthropic_response(
    resp: reqwest::Response,
    tx: mpsc::UnboundedSender<BackendMessage>,
) -> Result<(), reqwest::Error> {
    let mut stream = resp.bytes_stream();
    let mut line_buf = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let s = String::from_utf8_lossy(&chunk);
        
        for ch in s.chars() {
            if ch == '\n' {
                if !line_buf.starts_with("data:") {
                    line_buf.clear();
                    continue;
                }
                let data = line_buf[5..].trim();
                if data == "[DONE]" {
                    tx.send(BackendMessage::Done).ok();
                    return Ok(());
                }
                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                    let event_type = chunk.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match event_type {
                        "content_block_delta" => {
                            if let Some(delta) = chunk.get("delta") {
                                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        tx.send(BackendMessage::Token {
                                            content: text.to_string(),
                                        }).ok();
                                    }
                                }
                            }
                        }
                        "message_stop" => {
                            tx.send(BackendMessage::Done).ok();
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                line_buf.clear();
            } else {
                line_buf.push(ch);
            }
        }
    }
    Ok(())
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown();
    }
}
