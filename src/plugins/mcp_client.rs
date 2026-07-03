use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

use crate::agent::errors::AgentError;
use crate::agent::mcp::{McpResponse, McpSession};

pub struct ExternalMcpServer {
    pub process: Child,
    pub session: Arc<McpSession>,
    stdin: Mutex<ChildStdin>,
    pub supported_tools: Vec<serde_json::Value>,
}

impl ExternalMcpServer {
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self, AgentError> {
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = process.stdin.take().ok_or_else(|| {
            AgentError::Ipc(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "external MCP stdin unavailable",
            ))
        })?;
        let stdout = process.stdout.take().ok_or_else(|| {
            AgentError::Ipc(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "external MCP stdout unavailable",
            ))
        })?;
        let session = Arc::new(McpSession::new());
        let session_reader = Arc::clone(&session);

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Ok(response) = serde_json::from_str::<McpResponse>(&line) {
                            let _ = session_reader.handle_incoming(response);
                        }
                    }
                }
            }
        });

        Ok(Self {
            process,
            session,
            stdin: Mutex::new(stdin),
            supported_tools: Vec::new(),
        })
    }

    pub async fn discover_tools(&mut self) -> Result<(), AgentError> {
        let receiver = {
            let mut stdin = self.stdin.lock().await;
            self.session
                .send_request("tools/list", serde_json::json!({}), &mut stdin)
                .await?
        };
        let response = receiver.await.map_err(|_| AgentError::ChannelClosed)?;
        self.supported_tools = response
            .result
            .and_then(|result| result.get("tools").cloned())
            .and_then(|tools| tools.as_array().cloned())
            .unwrap_or_default();
        Ok(())
    }

    pub async fn proxy_tool_call(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        let receiver = {
            let mut stdin = self.stdin.lock().await;
            self.session
                .send_request(
                    "tools/call",
                    serde_json::json!({"name": tool_name, "arguments": arguments}),
                    &mut stdin,
                )
                .await?
        };
        let response = receiver.await.map_err(|_| AgentError::ChannelClosed)?;
        if let Some(error) = response.error {
            return Err(AgentError::Mcp(error.message));
        }
        Ok(response.result.unwrap_or_else(|| serde_json::json!({})))
    }
}

#[cfg(test)]
mod tests {
    use super::ExternalMcpServer;

    #[tokio::test]
    async fn discover_tools_populates_supported_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("mock_mcp.py");
        std::fs::write(
            &script,
            r#"
import json, sys
for line in sys.stdin:
    req = json.loads(line)
    if req.get("method") == "tools/list":
        resp = {"jsonrpc":"2.0","id":req["id"],"result":{"tools":[{"name":"echo","inputSchema":{}}]}}
    else:
        resp = {"jsonrpc":"2.0","id":req["id"],"result":{"ok": True}}
    print(json.dumps(resp), flush=True)
"#,
        )
        .expect("write script");

        let script_arg = script.to_string_lossy().to_string();
        let mut server = ExternalMcpServer::spawn("python", &[&script_arg])
            .await
            .expect("spawn");
        server.discover_tools().await.expect("discover");
        assert_eq!(server.supported_tools.len(), 1);
    }
}
