use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use tokio::sync::mpsc;

use super::protocol::{BackendMessage, FrontendMessage};

pub struct Backend {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl Backend {
    pub fn spawn(script: &str, model_path: &str) -> Result<Self> {
        let mut child = Command::new("python3")
            .arg(script)
            .arg(model_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // show Python errors in terminal
            .spawn()
            .context("Failed to spawn airllm_backend.py — is Python + AirLLM installed?")?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut backend = Self { child, stdin, reader };

        // Wait for "ready" message
        backend.wait_for_ready()?;

        Ok(backend)
    }

    fn wait_for_ready(&mut self) -> Result<()> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        let msg: BackendMessage = serde_json::from_str(line.trim())?;
        match msg {
            BackendMessage::Ready => Ok(()),
            BackendMessage::Error { message } => {
                anyhow::bail!("Backend error on startup: {}", message)
            }
            _ => anyhow::bail!("Unexpected message from backend: {:?}", msg),
        }
    }

    pub fn send(&mut self, msg: &FrontendMessage) -> Result<()> {
        let line = serde_json::to_string(msg)?;
        writeln!(self.stdin, "{}", line)?;
        self.stdin.flush()?;
        Ok(())
    }

    /// Read messages until Done or Error, sending tokens over the channel
    pub fn stream_response(&mut self, tx: &mpsc::UnboundedSender<BackendMessage>) -> Result<()> {
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line)?;
            if line.is_empty() {
                break;
            }
            let msg: BackendMessage = serde_json::from_str(line.trim())?;
            let is_terminal = matches!(msg, BackendMessage::Done | BackendMessage::Error { .. });
            tx.send(msg).ok();
            if is_terminal {
                break;
            }
        }
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}