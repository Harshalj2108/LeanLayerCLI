use anyhow::{Result, Context};
use std::process::{Child, Command};

use tokio::sync::mpsc;
use super::protocol::{BackendMessage, ChatMessage};
use crate::config::Config;

pub struct Backend {
    pub base_url: String,
    pub(crate) child: Option<Child>,
    pub api_provider: String,
    pub api_key: Option<String>,
    pub api_model: Option<String>,
}

impl Backend {
    pub fn spawn(cfg: &Config) -> Result<Self> {
        let api_provider = cfg.api_provider.clone();
        let api_key = crate::config::resolve_api_key(cfg);
        let api_model = cfg.api_model.clone();

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
                other => anyhow::bail!("Unknown API provider: {}", other),
            };
            return Ok(Self {
                base_url,
                child: None,
                api_provider,
                api_key,
                api_model,
            });
        }

        // Local provider: llama-server
        let base_url = format!("http://127.0.0.1:{}", cfg.port);

        // Enhancement #3: Health check — is llama-server already running?
        let health_url = format!("{}/health", base_url);
        let server_running = ureq::get(&health_url)
            .timeout(std::time::Duration::from_secs(2))
            .call()
            .is_ok();

        if server_running {
            return Ok(Self {
                base_url,
                child: None,
                api_provider,
                api_key,
                api_model,
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

            eprintln!("[airllm] Starting llama-server from: {}", server_path);
            eprintln!("[airllm]   model: {}", cfg.model_path);
            eprintln!("[airllm]   port: {}, gpu_layers: {}, ctx_size: {}", cfg.port, cfg.gpu_layers, cfg.ctx_size);

            let child = Command::new(server_path)
                .arg("-m")
                .arg(&cfg.model_path)
                .arg("--port")
                .arg(cfg.port.to_string())
                .arg("-ngl")
                .arg(cfg.gpu_layers.to_string())
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
            loop {
                if start.elapsed() > timeout {
                    anyhow::bail!("llama-server failed to start within 60 seconds");
                }
                if ureq::get(&health_url)
                    .timeout(std::time::Duration::from_secs(1))
                    .call()
                    .is_ok()
                {
                    eprintln!("[airllm] llama-server is ready ({:.1}s)", start.elapsed().as_secs_f64());
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            return Ok(Self {
                base_url,
                child: Some(child),
                api_provider,
                api_key,
                api_model,
            });
        }

        anyhow::bail!(
            "llama-server not running on port {}. Either:\n  1. Start it manually: llama-server -m <model> --port {}\n  2. Set 'llama_server_path' in config.toml for auto-launch\n  3. Use a cloud provider: set api_provider to 'openai', 'gemini', 'anthropic', or 'openrouter'",
            cfg.port, cfg.port
        );
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
        thinking: bool,
    ) {
        match self.api_provider.as_str() {
            "local" => self.send_local(messages, tx, thinking),
            "openai" | "openrouter" => self.send_openai_compat(messages, tx, thinking),
            "gemini" => self.send_gemini(messages, tx, thinking),
            "anthropic" => self.send_anthropic(messages, tx, thinking),
            _ => {
                tx.send(BackendMessage::Error {
                    message: format!("Unknown API provider: {}", self.api_provider),
                }).ok();
            }
        }
    }

    /// Local llama-server (OpenAI-compatible endpoint)
    fn send_local(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        thinking: bool,
    ) {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let serialized_messages = serialize_messages_openai(&messages);

        std::thread::spawn(move || {
            let body = serde_json::json!({
                "model": "local",
                "messages": serialized_messages,
                "stream": true,
                "temperature": if thinking { 0.7 } else { 0.6 },
                "top_p": 0.95,
                "chat_template_kwargs": {
                    "enable_thinking": thinking
                }
            });

            stream_openai_response(&url, &body.to_string(), None, tx);
        });
    }

    /// OpenAI and OpenRouter (standard OpenAI chat/completions API)
    fn send_openai_compat(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        thinking: bool,
    ) {
        let url = if self.api_provider == "openrouter" {
            format!("{}/v1/chat/completions", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };
        let model = self.api_model.clone().unwrap_or_else(|| "gpt-4o".into());
        let api_key = self.api_key.clone();
        let serialized_messages = serialize_messages_openai(&messages);

        std::thread::spawn(move || {
            let body = serde_json::json!({
                "model": model,
                "messages": serialized_messages,
                "stream": true,
                "temperature": if thinking { 0.7 } else { 0.6 },
            });

            stream_openai_response(&url, &body.to_string(), api_key.as_deref(), tx);
        });
    }

    /// Google Gemini (using the OpenAI-compatible endpoint)
    fn send_gemini(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        thinking: bool,
    ) {
        let model = self.api_model.clone().unwrap_or_else(|| "gemini-2.5-flash".into());
        let api_key = self.api_key.clone();
        let serialized_messages = serialize_messages_openai(&messages);

        std::thread::spawn(move || {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
            );

            let body = serde_json::json!({
                "model": model,
                "messages": serialized_messages,
                "stream": true,
                "temperature": if thinking { 0.7 } else { 0.6 },
            });

            stream_openai_response(&url, &body.to_string(), api_key.as_deref(), tx);
        });
    }

    /// Anthropic Messages API
    fn send_anthropic(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
        thinking: bool,
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
                        // img_data is "data:image/png;base64,..."
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

        std::thread::spawn(move || {
            let mut body = serde_json::json!({
                "model": model,
                "messages": anthropic_messages,
                "stream": true,
                "max_tokens": 8192,
                "temperature": if thinking { 0.7 } else { 0.6 },
            });
            if !system_text.is_empty() {
                body["system"] = serde_json::json!(system_text);
            }

            let response = ureq::post(&url)
                .set("Content-Type", "application/json")
                .set("x-api-key", &api_key)
                .set("anthropic-version", "2023-06-01")
                .send_string(&body.to_string());

            match response {
                Ok(resp) => {
                    use std::io::Read;
                    let mut reader = resp.into_reader();
                    let mut buf = [0; 1024];
                    let mut line_buf = String::new();
                    let mut done = false;

                    while !done {
                        match reader.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let s = String::from_utf8_lossy(&buf[..n]);
                                for ch in s.chars() {
                                    if ch == '\n' {
                                        if !line_buf.starts_with("data:") {
                                            line_buf.clear();
                                            continue;
                                        }
                                        let data = line_buf[5..].trim();
                                        if data == "[DONE]" {
                                            tx.send(BackendMessage::Done).ok();
                                            done = true;
                                            break;
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
                                                    done = true;
                                                    break;
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
                            Err(_) => break,
                        }
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

/// Shared SSE streaming parser for OpenAI-compatible endpoints
fn stream_openai_response(
    url: &str,
    body: &str,
    api_key: Option<&str>,
    tx: mpsc::UnboundedSender<BackendMessage>,
) {
    let mut request = ureq::post(url)
        .set("Content-Type", "application/json");

    if let Some(key) = api_key {
        request = request.set("Authorization", &format!("Bearer {}", key));
    }

    let response = request.send_string(body);

    match response {
        Ok(resp) => {
            use std::io::Read;
            let mut reader = resp.into_reader();
            let mut buf = [0; 1024];
            let mut line_buf = String::new();
            let mut done = false;
            let mut in_think = false;

            while !done {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let s = String::from_utf8_lossy(&buf[..n]);
                        for ch in s.chars() {
                            if ch == '\n' {
                                if !line_buf.starts_with("data:") {
                                    line_buf.clear();
                                    continue;
                                }
                                let data = line_buf[5..].trim();
                                if data == "[DONE]" {
                                    tx.send(BackendMessage::Done).ok();
                                    done = true;
                                    break;
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
                                            if !in_think {
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
                    Err(_) => break,
                }
            }
        }
        Err(e) => {
            tx.send(BackendMessage::Error {
                message: e.to_string(),
            }).ok();
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown();
    }
}