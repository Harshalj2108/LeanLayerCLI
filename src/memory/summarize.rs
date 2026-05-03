use anyhow::Result;
use crate::backend::{process::Backend, protocol::{ChatMessage, FrontendMessage}};
use tokio::sync::mpsc;
use crate::backend::protocol::BackendMessage;

pub struct SessionSummary {
    pub summary: String,
    pub concepts: Vec<String>,
    pub related: Vec<String>,
}

pub fn summarize_session(backend: &mut Backend, history: &[ChatMessage]) -> Result<SessionSummary> {
    // Build a transcript for Gemma to summarize
    let transcript = history
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"You are summarizing a conversation for long-term memory storage in an Obsidian vault.

Given this conversation transcript:
{}

Respond ONLY with a JSON object in this exact format, no other text:
{{
  "summary": "2-3 sentence summary of what was discussed",
  "concepts": ["concept1", "concept2", "concept3"],
  "related": ["related-topic1", "related-topic2"]
}}

concepts: key technical topics, tools, or ideas discussed (2-6 items, lowercase, hyphenated)
related: broader topics this connects to from previous knowledge"#,
        transcript
    );

    let messages = vec![ChatMessage {
        role: "user".into(),
        content: prompt,
    }];

    let (tx, mut rx) = mpsc::unbounded_channel();
    backend.send(&FrontendMessage::Generate { messages })?;

    let tx_clone = tx.clone();
    backend.stream_response(&tx_clone)?;

    let mut full_response = String::new();
    while let Ok(msg) = rx.try_recv() {
        if let BackendMessage::Token { content } = msg {
            full_response.push_str(&content);
        }
    }

    // Parse JSON response
    let trimmed = full_response.trim();
    let start = trimmed.find('{').unwrap_or(0);
    let end = trimmed.rfind('}').map(|i| i + 1).unwrap_or(trimmed.len());
    let json_str = &trimmed[start..end];

    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .unwrap_or_else(|_| serde_json::json!({
            "summary": full_response.trim(),
            "concepts": [],
            "related": []
        }));

    Ok(SessionSummary {
        summary: parsed["summary"].as_str().unwrap_or("").to_string(),
        concepts: parsed["concepts"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect(),
        related: parsed["related"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect(),
    })
}