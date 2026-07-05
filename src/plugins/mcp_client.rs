use std::sync::Arc;
use tokio::process::{Child, Command};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::Value;

use crate::agent::errors::AgentError;
use crate::agent::mcp::{McpSession, McpResponse, JsonRpcRequest};

pub struct ExternalMcpServer {
    pub process: Child,
    pub session: Arc<McpSession>,
    pub supported_tools: Vec<Value>,
}

impl ExternalMcpServer {
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self, AgentError> {
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(AgentError::Ipc)?;

        let stdout = process.stdout.take().ok_or_else(|| {
            AgentError::Ipc(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "No stdout"))
        })?;
        
        let session = Arc::new(McpSession::new());
        let session_clone = Arc::clone(&session);
        
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(response) = serde_json::from_str::<McpResponse>(&line) {
                    let _ = session_clone.handle_incoming(response);
                }
            }
        });

        Ok(Self {
            process,
            session,
            supported_tools: Vec::new(),
        })
    }

    pub async fn discover_tools(&mut self) -> Result<(), AgentError> {
        let stdin = self.process.stdin.as_mut().ok_or_else(|| {
            AgentError::Ipc(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "No stdin"))
        })?;
        
        let rx = self.session.send_request("tools/list", serde_json::json!({}), stdin).await?;
        
        let response = rx.await.map_err(|_| AgentError::Mcp("Request cancelled".into()))?;
        if let Some(err) = response.error {
            return Err(AgentError::Mcp(err.message));
        }

        if let Some(result) = response.result {
            if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                self.supported_tools = tools.clone();
            }
        }
        
        Ok(())
    }

    pub async fn proxy_tool_call(&mut self, tool_name: &str, arguments: Value) -> Result<Value, AgentError> {
        let stdin = self.process.stdin.as_mut().ok_or_else(|| {
            AgentError::Ipc(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "No stdin"))
        })?;
        
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        });
        
        let rx = self.session.send_request("tools/call", params, stdin).await?;
        let response = rx.await.map_err(|_| AgentError::Mcp("Request cancelled".into()))?;
        
        if let Some(err) = response.error {
            return Err(AgentError::Mcp(err.message));
        }
        
        Ok(response.result.unwrap_or_else(|| serde_json::json!({})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_mock_python_mcp_server() {
        // Create a mock python script that responds to tools/list
        let python_script = r#"
import sys
import json

for line in sys.stdin:
    req = json.loads(line)
    if req.get("method") == "tools/list":
        res = {
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "tools": [
                    {
                        "name": "mock_tool",
                        "description": "A mock tool",
                        "inputSchema": { "type": "object", "properties": {} }
                    }
                ]
            }
        }
        print(json.dumps(res), flush=True)
        break
"#;
        let mut script_file = NamedTempFile::new().unwrap();
        write!(script_file, "{}", python_script).unwrap();
        script_file.flush().unwrap();

        let path_str = script_file.path().to_str().unwrap();

        let mut server = ExternalMcpServer::spawn("python", &[path_str]).await.expect("Failed to spawn");
        server.discover_tools().await.expect("Failed to discover tools");

        assert_eq!(server.supported_tools.len(), 1);
        assert_eq!(server.supported_tools[0]["name"], "mock_tool");
    }
}
