#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("IPC Error: {0}")]
    Ipc(#[from] std::io::Error),
    #[error("MCP Protocol Error: {0}")]
    Mcp(String),
    #[error("Workspace Security Violation: {0}")]
    Security(String),
    #[error("Tool Execution Failed: {0}")]
    Tool(String),
    #[error("Serialization Error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Channel Closed")]
    ChannelClosed,
}

#[cfg(test)]
mod tests {
    use super::AgentError;

    #[test]
    fn formats_security_error() {
        let err = AgentError::Security("outside workspace".to_string());
        assert!(err.to_string().contains("Workspace Security Violation"));
    }
}
