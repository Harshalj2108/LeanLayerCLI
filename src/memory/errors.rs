#[derive(thiserror::Error, Debug)]
pub enum MemoryError {
    #[error("Graph Error: {0}")]
    Graph(String),
    #[error("Vault Encryption Error: {0}")]
    Vault(String),
    #[error("Token Limit Exceeded: {0}/{1}")]
    ContextOverflow(usize, usize),
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization Error: {0}")]
    Serialization(String),
}

impl From<toml::de::Error> for MemoryError {
    fn from(value: toml::de::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<toml::ser::Error> for MemoryError {
    fn from(value: toml::ser::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<serde_json::Error> for MemoryError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<aes_gcm::Error> for MemoryError {
    fn from(value: aes_gcm::Error) -> Self {
        Self::Vault(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryError;

    #[test]
    fn formats_context_overflow() {
        let err = MemoryError::ContextOverflow(12, 10);
        assert!(err.to_string().contains("12/10"));
    }
}
