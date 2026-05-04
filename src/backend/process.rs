use anyhow::Result;
use std::io::{BufRead, BufReader};
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
                    let reader = BufReader::new(resp.into_reader());
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(_) => break,
                        };
                        if !line.starts_with("data:") {
                            continue;
                        }
                        let data = line[5..].trim();
                        if data == "[DONE]" {
                            tx.send(BackendMessage::Done).ok();
                            break;
                        }
                        if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = chunk["choices"][0]["delta"]["content"].as_str() {
                                if !content.is_empty() {
                                    tx.send(BackendMessage::Token {
                                        content: content.to_string(),
                                    }).ok();
                                }
                            }
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