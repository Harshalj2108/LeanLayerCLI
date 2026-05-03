use anyhow::Result;
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::protocol::ChatMessage;
use crate::config::Config;

pub struct VaultWriter {
    vault_path: PathBuf,
}

impl VaultWriter {
    pub fn new(cfg: &Config) -> Result<Self> {
        let vault_path = PathBuf::from(&cfg.vault_path);
        fs::create_dir_all(vault_path.join("sessions"))?;
        fs::create_dir_all(vault_path.join("concepts"))?;
        Ok(Self { vault_path })
    }

    pub fn write_session(
        &self,
        summary: &str,
        concepts: &[String],
        related: &[String],
        messages: &[ChatMessage],
    ) -> Result<PathBuf> {
        let date = Local::now().format("%Y-%m-%d-%H%M%S").to_string();
        let filename = format!("{}.md", date);
        let path = self.vault_path.join("sessions").join(&filename);

        let tags = concepts
            .iter()
            .map(|c| format!("  - {}", c))
            .collect::<Vec<_>>()
            .join("\n");

        let related_links = related
            .iter()
            .map(|r| format!("[[{}]]", r))
            .collect::<Vec<_>>()
            .join(", ");

        let concept_links = concepts
            .iter()
            .map(|c| format!("- [[{}]]", c))
            .collect::<Vec<_>>()
            .join("\n");

        // Build transcript
        let transcript = messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "user" => "**You**",
                    "assistant" => "**Gemma**",
                    _ => &m.role,
                };
                format!("{}: {}", role, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let content = format!(
            r#"---
date: {}
model: gemma4:31b
tags:
{}
related: {}
---

# Session: {}

## Summary
{}

## Key Concepts
{}

## Transcript
{}
"#,
            Local::now().format("%Y-%m-%d %H:%M"),
            tags,
            related_links,
            date,
            summary,
            concept_links,
            transcript
        );

        fs::write(&path, content)?;

        // Update concept nodes
        for concept in concepts {
            self.upsert_concept(concept, &date)?;
        }

        Ok(path)
    }

    fn upsert_concept(&self, concept: &str, session_date: &str) -> Result<()> {
        let filename = format!("{}.md", concept.to_lowercase().replace(' ', "-"));
        let path = self.vault_path.join("concepts").join(&filename);

        if path.exists() {
            // Append the session link
            let existing = fs::read_to_string(&path)?;
            let updated = format!("{}\n- [[sessions/{}]]", existing.trim_end(), session_date);
            fs::write(&path, updated)?;
        } else {
            // Create new concept node
            let content = format!(
                r#"---
concept: {}
first_seen: {}
---

# {}

## Sessions
- [[sessions/{}]]
"#,
                concept,
                Local::now().format("%Y-%m-%d"),
                concept,
                session_date
            );
            fs::write(&path, content)?;
        }

        Ok(())
    }
}

pub fn list_sessions() -> Result<()> {
    let cfg = crate::config::load()?;
    let sessions_dir = PathBuf::from(&cfg.vault_path).join("sessions");

    if !sessions_dir.exists() {
        println!("No sessions yet. Run `airllm chat` to start.");
        return Ok(());
    }

    let mut entries: Vec<_> = fs::read_dir(&sessions_dir)?
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by_key(|e| e.file_name());
    entries.reverse();

    println!("\n Past Sessions\n");
    for entry in entries.iter().take(20) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let name = name.trim_end_matches(".md");

        // Try to read summary line
        let summary = fs::read_to_string(entry.path())
            .ok()
            .and_then(|c| {
                c.lines()
                    .skip_while(|l| *l != "## Summary")
                    .nth(1)
                    .map(|l| l.to_string())
            })
            .unwrap_or_default();

        println!("  {}  {}", name, summary);
    }
    println!();

    Ok(())
}