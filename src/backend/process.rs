use anyhow::Result;

use tokio::sync::mpsc;
use super::protocol::{BackendMessage, ChatMessage};

pub struct Backend {
    pub base_url: String,
}

impl Backend {
    pub fn spawn(_script: &str, _model_path: &str) -> Result<Self> {
        // No subprocess — talk directly to llama-server
        Ok(Self {
            base_url: "http://127.0.0.1:8081".into(),
        })
    }

    pub fn send_generate(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<BackendMessage>,
    ) {
        let url = format!("{}/v1/chat/completions", self.base_url);

        std::thread::spawn(move || {
            let body = serde_json::json!({
                "model": "local",
                "messages": messages,
                "stream": true,
                "temperature": 0.6,
                "top_p": 0.95,
            });

            let response = ureq::post(&url)
                .set("Content-Type", "application/json")
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
                                            if let Some(delta) = chunk["choices"][0]["delta"].as_object() {
                                                let mut text = String::new();
                                                if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                                                    text.push_str(reasoning);
                                                }
                                                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                                    text.push_str(content);
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
        });
    }
}