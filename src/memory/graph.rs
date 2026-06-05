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
    pub vault_path: PathBuf,
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

        Ok(Self { nodes, vault_path: vault_path.clone() })
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

    /// Enhancement #2: Find concept nodes whose IDs appear as keywords in the user prompt
    pub fn find_matching_concepts(&self, prompt: &str) -> Vec<&Node> {
        let prompt_lower = prompt.to_lowercase();
        let prompt_words: Vec<&str> = prompt_lower.split_whitespace().collect();

        self.nodes.values()
            .filter(|n| matches!(n.kind, NodeKind::Concept))
            .filter(|n| {
                let concept_lower = n.id.to_lowercase();
                // Match if any concept word appears in prompt, or vice versa
                let concept_parts: Vec<&str> = concept_lower.split('-').collect();
                concept_parts.iter().any(|part| {
                    part.len() >= 3 && prompt_words.iter().any(|w| w.contains(part))
                }) || prompt_words.iter().any(|w| concept_lower.contains(w) && w.len() >= 3)
            })
            .collect()
    }

    /// Enhancement #2: Build RAG context string from matched concepts and their linked sessions
    pub fn build_concept_context(&self, prompt: &str) -> String {
        let matched = self.find_matching_concepts(prompt);
        if matched.is_empty() {
            return String::new();
        }

        let mut ctx = String::from("Relevant knowledge from your vault:\n\n");

        for concept in &matched {
            // Try to read concept file
            let concept_path = self.vault_path
                .join("concepts")
                .join(format!("{}.md", concept.id));

            if let Ok(content) = fs::read_to_string(&concept_path) {
                ctx.push_str(&format!("## Concept: {}\n", concept.id));

                // Extract summary lines (skip frontmatter)
                let mut in_frontmatter = false;
                for line in content.lines() {
                    if line.trim() == "---" {
                        in_frontmatter = !in_frontmatter;
                        continue;
                    }
                    if !in_frontmatter {
                        ctx.push_str(line);
                        ctx.push('\n');
                    }
                }
                ctx.push('\n');
            }

            // Load linked session summaries (limit to 3 most recent)
            for session_id in concept.connections.iter().take(3) {
                let session_path = self.vault_path
                    .join("sessions")
                    .join(format!("{}.md", session_id));

                if let Ok(content) = fs::read_to_string(&session_path) {
                    // Extract just the summary section
                    let mut in_summary = false;
                    let mut summary_lines = Vec::new();
                    for line in content.lines() {
                        if line.starts_with("## Summary") {
                            in_summary = true;
                            continue;
                        }
                        if in_summary {
                            if line.starts_with("## ") {
                                break;
                            }
                            summary_lines.push(line);
                        }
                    }
                    if !summary_lines.is_empty() {
                        ctx.push_str(&format!("  Related session {}: {}\n",
                            session_id,
                            summary_lines.join(" ").trim()
                        ));
                    }
                }
            }
            ctx.push('\n');
        }

        ctx
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