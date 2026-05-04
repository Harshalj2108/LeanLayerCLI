use ureq;
use anyhow::Result;
use crate::backend::protocol::ChatMessage;

pub struct SessionSummary {
    pub summary: String,
    pub concepts: Vec<String>,
    pub related: Vec<String>,
}

pub fn summarize_session(_backend: &mut crate::backend::process::Backend, history: &[ChatMessage]) -> Result<SessionSummary> {
    let transcript = history
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"Summarize this conversation for Obsidian vault storage. Respond ONLY with JSON, no other text:
{{
  "summary": "2-3 sentence summary",
  "concepts": ["concept1", "concept2"],
  "related": ["topic1", "topic2"]
}}

Conversation:
{}"#,
        transcript
    );

    let body = serde_json::json!({
        "model": "local",
        "messages": [{"role": "user", "content": prompt}],
        "stream": false,
        "temperature": 0.3,
    });

    let resp = ureq::post("http://127.0.0.1:8081/v1/chat/completions")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())?;

    let json: serde_json::Value = resp.into_json()?;
    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let start = text.find('{').unwrap_or(0);
    let end = text.rfind('}').map(|i| i + 1).unwrap_or(text.len());
    let parsed: serde_json::Value = serde_json::from_str(&text[start..end])
        .unwrap_or_else(|_| serde_json::json!({
            "summary": text.trim(),
            "concepts": [],
            "related": []
        }));

    Ok(SessionSummary {
        summary: parsed["summary"].as_str().unwrap_or("").to_string(),
        concepts: parsed["concepts"].as_array().unwrap_or(&vec![])
            .iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect(),
        related: parsed["related"].as_array().unwrap_or(&vec![])
            .iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect(),
    })
}