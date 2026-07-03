use std::sync::Arc;

use async_trait::async_trait;

use crate::agent::errors::AgentError;
use crate::agent::tools::Tool;
use crate::agent::workspace::Workspace;
use crate::integration::git::GitWorkspace;
use crate::integration::lsp::LspClient;

pub struct GitStatusTool {
    pub git: GitWorkspace,
}

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &'static str {
        "git_status"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _workspace: &Workspace,
    ) -> Result<serde_json::Value, AgentError> {
        let status = self
            .git
            .get_status()
            .await
            .map_err(|err| AgentError::Tool(err.to_string()))?;
        Ok(serde_json::json!({ "status": status }))
    }
}

pub struct GitCommitTool {
    pub git: GitWorkspace,
}

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &'static str {
        "git_commit"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["message"],
            "properties": {"message": {"type": "string"}}
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _workspace: &Workspace,
    ) -> Result<serde_json::Value, AgentError> {
        let message = args
            .get("message")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Tool("missing commit message".to_string()))?;
        let oid = self
            .git
            .commit_all(message)
            .await
            .map_err(|err| AgentError::Tool(err.to_string()))?;
        Ok(serde_json::json!({ "oid": oid }))
    }
}

pub struct LspHoverTool {
    pub lsp: Arc<LspClient>,
}

#[async_trait]
impl Tool for LspHoverTool {
    fn name(&self) -> &'static str {
        "lsp_hover"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["file_uri", "line", "character"],
            "properties": {
                "file_uri": {"type": "string"},
                "line": {"type": "integer"},
                "character": {"type": "integer"}
            }
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _workspace: &Workspace,
    ) -> Result<serde_json::Value, AgentError> {
        let file_uri = args
            .get("file_uri")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Tool("missing file_uri".to_string()))?;
        let line = args
            .get("line")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| AgentError::Tool("missing line".to_string()))? as u32;
        let character = args
            .get("character")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| AgentError::Tool("missing character".to_string()))? as u32;
        self.lsp
            .hover(file_uri, line, character)
            .await
            .map_err(|err| AgentError::Tool(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{GitCommitTool, GitStatusTool};
    use crate::agent::tools::Tool;
    use crate::integration::git::GitWorkspace;

    #[test]
    fn integration_tools_expose_names() {
        let git = GitWorkspace::new(".");
        assert_eq!(GitStatusTool { git: git.clone() }.name(), "git_status");
        assert_eq!(GitCommitTool { git }.name(), "git_commit");
    }
}
