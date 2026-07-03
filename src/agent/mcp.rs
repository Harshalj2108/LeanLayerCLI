use anyhow::{anyhow, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use std::process::Stdio;

use crate::agent::errors::AgentError;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: usize,
    pub method: String,
    pub params: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpErrorBody {
    pub code: i64,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

pub struct McpSession {
    pub request_id: AtomicUsize,
    pub pending: DashMap<usize, oneshot::Sender<McpResponse>>,
}

impl Default for McpSession {
    fn default() -> Self {
        Self {
            request_id: AtomicUsize::new(1),
            pending: DashMap::new(),
        }
    }
}

impl McpSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn send_request<T: Serialize>(
        &self,
        method: &str,
        params: T,
        writer: &mut ChildStdin,
    ) -> std::result::Result<oneshot::Receiver<McpResponse>, AgentError> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params: serde_json::to_value(params)?,
        };

        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);

        let mut encoded = serde_json::to_vec(&request)?;
        encoded.push(b'\n');
        if let Err(err) = writer.write_all(&encoded).await {
            self.pending.remove(&id);
            return Err(AgentError::Ipc(err));
        }
        writer.flush().await.map_err(AgentError::Ipc)?;
        Ok(rx)
    }

    pub fn handle_incoming(&self, response: McpResponse) -> std::result::Result<(), AgentError> {
        match self.pending.remove(&response.id) {
            Some((_, sender)) => sender
                .send(response)
                .map_err(|_| AgentError::Mcp("request receiver dropped".to_string())),
            None => Err(AgentError::Mcp(format!(
                "no pending request for id {}",
                response.id
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::{McpResponse, McpSession};

    #[tokio::test]
    async fn handle_incoming_resolves_pending_receiver() {
        let session = McpSession::new();
        let (tx, rx) = tokio::sync::oneshot::channel();
        session.pending.insert(1, tx);

        session
            .handle_incoming(McpResponse {
                jsonrpc: "2.0".to_string(),
                id: 1,
                result: Some(serde_json::json!({"ok": true})),
                error: None,
            })
            .expect("route response");

        let response = rx.await.expect("response");
        assert_eq!(response.result.expect("result")["ok"], true);
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

pub struct McpClient {
    child: Child,
    stdin_tx: mpsc::Sender<String>,
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    next_id: AtomicU64,
    pub server_name: String,
}

impl McpClient {
    pub async fn spawn(name: &str, config: &crate::config::McpServerConfig) -> Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        cmd.envs(&config.env);
        
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit()); // Pass stderr to the terminal for debugging

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Failed to open stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to open stdout"))?;

        let pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> = Arc::new(Mutex::new(HashMap::new()));
        let pending_requests_clone = pending_requests.clone();

        // Stdin writer task
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(100);
        let mut stdin = stdin;
        tokio::spawn(async move {
            while let Some(msg) = stdin_rx.recv().await {
                if stdin.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.write_all(b"\n").await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Stdout reader task
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                            let mut pending = pending_requests_clone.lock().await;
                            if let Some(tx) = pending.remove(&resp.id) {
                                let _ = tx.send(resp);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut client = Self {
            child,
            stdin_tx,
            pending_requests,
            next_id: AtomicU64::new(1),
            server_name: name.to_string(),
        };

        // Initialize handshake
        client.initialize().await?;

        Ok(client)
    }

    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<JsonRpcResponse> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, tx);
        }

        let msg = serde_json::to_string(&req)?;
        self.stdin_tx.send(msg).await?;

        // Wait for response
        let resp = rx.await?;
        if let Some(err) = resp.error {
            return Err(anyhow!("MCP Error: {}", err.to_string()));
        }
        Ok(resp)
    }

    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        let msg = serde_json::to_string(&notif)?;
        self.stdin_tx.send(msg).await?;
        Ok(())
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "airllm-rs",
                "version": "0.1.0"
            }
        });

        let _resp = self.send_request("initialize", Some(params)).await?;
        
        // After initialize, we must send initialized notification
        self.send_notification("initialized", None).await?;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let resp = self.send_request("tools/list", None).await?;
        if let Some(result) = resp.result {
            if let Some(tools) = result.get("tools") {
                let parsed: Vec<McpToolDef> = serde_json::from_value(tools.clone())?;
                return Ok(parsed);
            }
        }
        Ok(vec![])
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String> {
        let params = serde_json::json!({
            "name": name,
            "arguments": args
        });

        let resp = self.send_request("tools/call", Some(params)).await?;
        if let Some(result) = resp.result {
            if let Some(content_array) = result.get("content").and_then(|c| c.as_array()) {
                let mut output = String::new();
                for item in content_array {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                return Ok(output.trim().to_string());
            } else if let Some(is_err) = result.get("isError").and_then(|e| e.as_bool()) {
                 if is_err {
                     return Err(anyhow!("Tool call returned an error flag: {:?}", result));
                 }
            }
        }
        Err(anyhow!("Invalid response from tool call"))
    }
}
