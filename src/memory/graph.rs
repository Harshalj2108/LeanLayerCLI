use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub connections: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Session,
    Concept,
}

pub struct MemoryGraph {
    pub nodes: HashMap<String, Node>,
}

impl MemoryGraph {
    pub fn load(vault_path: &PathBuf) -> Result<Self> {
        let mut nodes = HashMap::new();

        // Load sessions
        let sessions_dir = vault_path.join("sessions");
        if sessions_dir.exists() {
            for entry in fs::read_dir(&sessions_dir)?.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let id = name.trim_end_matches(".md").to_string();
                let connections = extract_wikilinks(&fs::read_to_string(entry.path())?);
                nodes.insert(id.clone(), Node {
                    id: id.clone(),
                    label: id[..id.len().min(16)].to_string(), // truncate for display
                    kind: NodeKind::Session,
                    connections,
                });
            }
        }

        // Load concepts
        let concepts_dir = vault_path.join("concepts");
        if concepts_dir.exists() {
            for entry in fs::read_dir(&concepts_dir)?.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let id = name.trim_end_matches(".md").to_string();
                let connections = extract_wikilinks(&fs::read_to_string(entry.path())?);
                nodes.insert(id.clone(), Node {
                    id: id.clone(),
                    label: id.clone(),
                    kind: NodeKind::Concept,
                    connections,
                });
            }
        }

        Ok(Self { nodes })
    }

    /// Return the N most recent/relevant nodes for display
    pub fn recent_nodes(&self, n: usize) -> Vec<&Node> {
        let mut sessions: Vec<_> = self.nodes.values()
            .filter(|n| matches!(n.kind, NodeKind::Session))
            .collect();
        sessions.sort_by(|a, b| b.id.cmp(&a.id));

        let mut result: Vec<&Node> = sessions.into_iter().take(n).collect();

        // Add concepts connected to those sessions
        let connected_ids: Vec<String> = result.iter()
            .flat_map(|n| n.connections.clone())
            .collect();

        for id in connected_ids {
            if let Some(node) = self.nodes.get(&id) {
                if !result.iter().any(|n| n.id == id) {
                    result.push(node);
                }
            }
        }

        result
    }
}

fn extract_wikilinks(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '[' && chars.peek() == Some(&'[') {
            chars.next();
            let mut link = String::new();
            for inner in chars.by_ref() {
                if inner == ']' {
                    break;
                }
                link.push(inner);
            }
            if !link.is_empty() {
                // Strip path prefix if any (e.g. "sessions/2025-05-03" -> "2025-05-03")
                let clean = link.split('/').last().unwrap_or(&link).to_string();
                links.push(clean);
            }
        }
    }

    links
}