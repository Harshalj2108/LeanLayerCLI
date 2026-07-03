use anyhow::Result;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use chrono::Local;
use rand::RngCore;
use std::fs;
use std::path::PathBuf;

use crate::backend::protocol::ChatMessage;
use crate::config::Config;
use crate::memory::errors::MemoryError;

pub struct SecureVault {
    pub master_key: [u8; 32],
    pub storage_path: PathBuf,
}

impl SecureVault {
    pub async fn init(storage_path: PathBuf, password: &str) -> std::result::Result<Self, MemoryError> {
        let mut master_key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
            password.as_bytes(),
            b"LeanLayerCLI secure vault v1",
            120_000,
            &mut master_key,
        );
        if let Some(parent) = storage_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::metadata(&storage_path).await.is_err() {
            tokio::fs::write(&storage_path, "{}").await?;
        }
        Ok(Self {
            master_key,
            storage_path,
        })
    }

    pub async fn store_secret(&self, key: &str, secret: &str) -> std::result::Result<(), MemoryError> {
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|err| MemoryError::Vault(err.to_string()))?;
        let mut nonce_bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, secret.as_bytes())?;

        let mut data = self.read_store().await?;
        data[key] = serde_json::json!({
            "nonce": base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
            "ciphertext": base64::engine::general_purpose::STANDARD.encode(ciphertext)
        });
        self.write_store(&data).await
    }

    pub async fn retrieve_secret(&self, key: &str) -> std::result::Result<String, MemoryError> {
        let data = self.read_store().await?;
        let entry = data
            .get(key)
            .ok_or_else(|| MemoryError::Vault(format!("secret not found: {}", key)))?;
        let nonce = entry
            .get("nonce")
            .and_then(|value| value.as_str())
            .ok_or_else(|| MemoryError::Vault("secret nonce missing".to_string()))?;
        let ciphertext = entry
            .get("ciphertext")
            .and_then(|value| value.as_str())
            .ok_or_else(|| MemoryError::Vault("secret ciphertext missing".to_string()))?;
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(nonce)
            .map_err(|err| MemoryError::Vault(err.to_string()))?;
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(ciphertext)
            .map_err(|err| MemoryError::Vault(err.to_string()))?;
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|err| MemoryError::Vault(err.to_string()))?;
        let plaintext = cipher.decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())?;
        String::from_utf8(plaintext).map_err(|err| MemoryError::Vault(err.to_string()))
    }

    async fn read_store(&self) -> std::result::Result<serde_json::Value, MemoryError> {
        let contents = tokio::fs::read_to_string(&self.storage_path).await?;
        Ok(serde_json::from_str(&contents)?)
    }

    async fn write_store(&self, data: &serde_json::Value) -> std::result::Result<(), MemoryError> {
        tokio::fs::write(&self.storage_path, serde_json::to_string_pretty(data)?).await?;
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::SecureVault;
    use crate::memory::errors::MemoryError;

    #[tokio::test]
    async fn secure_vault_roundtrip_and_wrong_password_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("vault.json");
        let vault = SecureVault::init(path.clone(), "password")
            .await
            .expect("vault");
        vault
            .store_secret("api", "secret-value")
            .await
            .expect("store");
        let secret = vault.retrieve_secret("api").await.expect("retrieve");
        assert_eq!(secret, "secret-value");

        let wrong = SecureVault::init(path, "wrong-password")
            .await
            .expect("wrong vault");
        let err = wrong.retrieve_secret("api").await.expect_err("decrypt fail");
        assert!(matches!(err, MemoryError::Vault(_)));
    }
}
