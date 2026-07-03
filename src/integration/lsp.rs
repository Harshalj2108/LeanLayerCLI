use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};

use crate::integration::errors::IntegrationError;

pub struct LspClient {
    pub process: Child,
    stdin: Mutex<ChildStdin>,
    pub request_id: AtomicUsize,
    pub pending_requests: Arc<DashMap<usize, oneshot::Sender<serde_json::Value>>>,
}

#[derive(Serialize)]
pub struct LspRequest<'a> {
    pub jsonrpc: &'a str,
    pub id: usize,
    pub method: &'a str,
    pub params: serde_json::Value,
}

impl LspClient {
    pub async fn start(
        server_command: &str,
        args: &[&str],
    ) -> Result<Arc<Self>, IntegrationError> {
        let mut process = Command::new(server_command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| IntegrationError::Lsp("language server stdin unavailable".to_string()))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| IntegrationError::Lsp("language server stdout unavailable".to_string()))?;

        let client = Arc::new(Self {
            process,
            stdin: Mutex::new(stdin),
            request_id: AtomicUsize::new(1),
            pending_requests: Arc::new(DashMap::new()),
        });

        let pending = Arc::clone(&client.pending_requests);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(message)) = read_lsp_message(&mut reader).await {
                let id = message.get("id").and_then(|id| id.as_u64()).map(|id| id as usize);
                if let Some(id) = id {
                    if let Some((_, tx)) = pending.remove(&id) {
                        let _ = tx.send(message);
                    }
                }
            }
        });

        Ok(client)
    }

    pub async fn initialize(&self, root_uri: &str) -> Result<serde_json::Value, IntegrationError> {
        self.send_request(
            "initialize",
            serde_json::json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {}
            }),
        )
        .await
    }

    pub async fn did_open(&self, file_uri: &str, text: &str) -> Result<(), IntegrationError> {
        self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": file_uri,
                    "languageId": language_id(file_uri),
                    "version": 1,
                    "text": text
                }
            }),
        )
        .await
    }

    pub async fn hover(
        &self,
        file_uri: &str,
        line: u32,
        character: u32,
    ) -> Result<serde_json::Value, IntegrationError> {
        self.send_request(
            "textDocument/hover",
            serde_json::json!({
                "textDocument": {"uri": file_uri},
                "position": {"line": line, "character": character}
            }),
        )
        .await
    }

    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IntegrationError> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = LspRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let (tx, rx) = oneshot::channel();
        self.pending_requests.insert(id, tx);
        self.write_payload(&request).await?;
        let response = rx.await.map_err(|_| IntegrationError::ChannelClosed)?;
        if let Some(error) = response.get("error") {
            return Err(IntegrationError::Lsp(error.to_string()));
        }
        Ok(response
            .get("result")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})))
    }

    async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), IntegrationError> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_payload(&notification).await
    }

    async fn write_payload<T: Serialize>(&self, payload: &T) -> Result<(), IntegrationError> {
        let body = serde_json::to_vec(payload)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(header.as_bytes()).await?;
        stdin.write_all(&body).await?;
        stdin.flush().await?;
        Ok(())
    }
}

async fn read_lsp_message<R>(
    reader: &mut BufReader<R>,
) -> Result<Option<serde_json::Value>, IntegrationError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|err| IntegrationError::Lsp(err.to_string()))?,
            );
        }
    }

    let len = content_length.ok_or_else(|| {
        IntegrationError::Lsp("missing Content-Length header from language server".to_string())
    })?;
    let mut body = vec![0; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn language_id(file_uri: &str) -> &'static str {
    if file_uri.ends_with(".py") {
        "python"
    } else if file_uri.ends_with(".rs") {
        "rust"
    } else {
        "plaintext"
    }
}

#[cfg(test)]
mod tests {
    use super::LspClient;

    #[tokio::test]
    async fn initialize_routes_mock_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("mock_lsp.py");
        std::fs::write(
            &script,
            r#"
import json, sys
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode().split(":", 1)
        headers[name.lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    req = json.loads(body)
    resp = {"jsonrpc": "2.0", "id": req["id"], "result": {"capabilities": {}}}
    payload = json.dumps(resp).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(payload)}\r\n\r\n".encode() + payload)
    sys.stdout.buffer.flush()
"#,
        )
        .expect("write script");

        let script_arg = script.to_string_lossy().to_string();
        let client = LspClient::start("python", &[&script_arg]).await.expect("start");
        let result = client
            .initialize("file:///tmp/workspace")
            .await
            .expect("initialize");
        assert!(result.get("capabilities").is_some());
    }
}
