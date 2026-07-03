use anyhow::Result;
use crate::backend::protocol::ChatMessage;
use crate::memory::errors::MemoryError;
use crate::memory::graph::MemoryNode;

pub struct SessionSummary {
    pub summary: String,
    pub concepts: Vec<String>,
    pub related: Vec<String>,
}

pub async fn summarize_session(history: &[ChatMessage]) -> Result<SessionSummary> {
    let transcript: String = history
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
Transcript:
{}"#, transcript);

    let body = serde_json::json!({
        "model": "local",
        "messages": [{"role": "user", "content": prompt}],
        "stream": false,
        "temperature": 0.3,
    });

    let client = reqwest::Client::new();
    let resp = client.post("http://127.0.0.1:8081/v1/chat/completions")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let json: serde_json::Value = resp.json().await?;
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

pub fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().saturating_mul(4) / 3
}

pub async fn compress_context(
    mut nodes: Vec<MemoryNode>,
    max_tokens: usize,
) -> std::result::Result<String, MemoryError> {
    nodes.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let mut output = String::new();
    let mut used_tokens = 0usize;
    let mut excluded = 0usize;

    for node in nodes {
        let candidate = format!("{:?}: {}\n", node.node_type, node.content);
        let candidate_tokens = estimate_tokens(&candidate);
        if used_tokens + candidate_tokens > max_tokens {
            excluded += 1;
            continue;
        }
        used_tokens += candidate_tokens;
        output.push_str(&candidate);
    }

    if excluded > 0 {
        let marker = format!("\n[{} older memory nodes summarized out of context]\n", excluded);
        if estimate_tokens(&output) + estimate_tokens(&marker) <= max_tokens {
            output.push_str(&marker);
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{compress_context, estimate_tokens};
    use crate::memory::graph::{MemoryNode, NodeType};

    #[tokio::test]
    async fn compress_context_truncates_without_utf8_boundary_panic() {
        let text = "åß∂ƒ ".repeat(6000);
        let node = MemoryNode {
            id: uuid::Uuid::new_v4(),
            node_type: NodeType::AgentThought("large".to_string()),
            content: text,
            timestamp: std::time::SystemTime::now(),
        };
        let compressed = compress_context(vec![node], 5000).await.expect("compress");
        assert!(estimate_tokens(&compressed) <= 5000);
    }
}
