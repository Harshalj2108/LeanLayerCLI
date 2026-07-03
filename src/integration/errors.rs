#[derive(thiserror::Error, Debug)]
pub enum IntegrationError {
    #[error("Git Error: {0}")]
    Git(#[from] git2::Error),
    #[error("LSP Protocol Error: {0}")]
    Lsp(String),
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Thread Join Error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("Serialization Error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Channel Closed")]
    ChannelClosed,
}

#[cfg(test)]
mod tests {
    use super::IntegrationError;

    #[test]
    fn formats_lsp_error() {
        let err = IntegrationError::Lsp("bad response".to_string());
        assert!(err.to_string().contains("LSP Protocol Error"));
    }
}
