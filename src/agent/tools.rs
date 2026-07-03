#![allow(dead_code)]
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agent::errors::AgentError;
use crate::agent::workspace::Workspace;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(
        &self,
        args: serde_json::Value,
        workspace: &Workspace,
    ) -> std::result::Result<serde_json::Value, AgentError>;
}

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {"type": "string"},
                "working_dir": {"type": "string"}
            }
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        workspace: &Workspace,
    ) -> std::result::Result<serde_json::Value, AgentError> {
        let command = args
            .get("command")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Tool("missing command".to_string()))?;
        let working_dir = args
            .get("working_dir")
            .and_then(|value| value.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| workspace.base_path().to_path_buf());
        let working_dir = workspace.normalize_and_verify(&working_dir)?;

        #[cfg(windows)]
        let mut child = {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(["/C", command]);
            cmd
        };

        #[cfg(not(windows))]
        let mut child = {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.args(["-c", command]);
            cmd
        };

        child
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let output = tokio::time::timeout(Duration::from_secs(10), child.output())
            .await
            .map_err(|_| AgentError::Tool("bash command timed out after 10s".to_string()))?
            .map_err(AgentError::Ipc)?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code().unwrap_or(-1),
            "success": output.status.success()
        }))
    }
}

pub struct FsReadTool;

#[async_trait]
impl Tool for FsReadTool {
    fn name(&self) -> &'static str {
        "fs_read"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        workspace: &Workspace,
    ) -> std::result::Result<serde_json::Value, AgentError> {
        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Tool("missing path".to_string()))?;
        let contents = workspace.read_file(std::path::Path::new(path)).await?;
        Ok(serde_json::json!({ "contents": contents }))
    }
}

pub struct FsWriteTool;

#[async_trait]
impl Tool for FsWriteTool {
    fn name(&self) -> &'static str {
        "fs_write"
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["path", "contents"],
            "properties": {
                "path": {"type": "string"},
                "contents": {"type": "string"}
            }
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        workspace: &Workspace,
    ) -> std::result::Result<serde_json::Value, AgentError> {
        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Tool("missing path".to_string()))?;
        let contents = args
            .get("contents")
            .and_then(|value| value.as_str())
            .or_else(|| args.get("content").and_then(|value| value.as_str()))
            .ok_or_else(|| AgentError::Tool("missing contents".to_string()))?;
        workspace
            .write_file_atomic(std::path::Path::new(path), contents)
            .await?;
        Ok(serde_json::json!({ "written": true }))
    }
}

/// Enhancement #11: Structured tool-calling schemas for the LLM
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    /// Run a shell command (e.g., "cargo check", "pytest")
    RunCommand {
        command: String,
        #[serde(default)]
        working_dir: Option<String>,
    },
    /// Read a file from anywhere on the filesystem
    ReadFile {
        path: String,
    },
    /// Write/patch a file anywhere on the filesystem
    WriteFile {
        path: String,
        content: String,
    },
    /// Search for text in workspace files
    SearchFiles {
        query: String,
        #[serde(default)]
        file_pattern: Option<String>,
    },
    /// Search the web for information
    WebSearch {
        query: String,
    },
    /// Call a tool on an external MCP server
    #[serde(rename = "mcp_tool")]
    McpToolCall {
        server: String,
        name: String,
        args: serde_json::Value,
    },
}

/// Parse tool calls from assistant response
/// The assistant should output JSON blocks tagged with ```tool
pub fn parse_tool_calls(content: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut in_tool_block = false;
    let mut tool_json = String::new();

    for line in content.lines() {
        if line.trim().starts_with("```tool") {
            in_tool_block = true;
            tool_json.clear();
            continue;
        }
        if in_tool_block && line.trim().starts_with("```") {
            in_tool_block = false;
            if let Ok(call) = serde_json::from_str::<ToolCall>(&tool_json) {
                calls.push(call);
            }
            tool_json.clear();
            continue;
        }
        if in_tool_block {
            tool_json.push_str(line);
            tool_json.push('\n');
        }
    }

    calls
}

/// Format tool result for injection back into the chat context
pub fn format_tool_result(call: &ToolCall, output: &str, success: bool) -> String {
    let tool_name = match call {
        ToolCall::RunCommand { command, .. } => format!("run_command: {}", command),
        ToolCall::ReadFile { path } => format!("read_file: {}", path),
        ToolCall::WriteFile { path, .. } => format!("write_file: {}", path),
        ToolCall::SearchFiles { query, .. } => format!("search: {}", query),
        ToolCall::WebSearch { query } => format!("web_search: {}", query),
        ToolCall::McpToolCall { server, name, .. } => format!("mcp_tool: {}/{}", server, name),
    };

    let status = if success { "SUCCESS" } else { "FAILED" };
    format!("[Tool Result: {} ({})]\n{}\n[/Tool Result]", tool_name, status, output)
}

/// Build the system prompt that teaches the LLM how to use tools
pub fn build_tool_system_prompt(role: crate::tui::app::AgentRole, mcp_tools: &std::collections::HashMap<String, crate::agent::mcp::McpToolDef>) -> String {
    let persona = match role {
        crate::tui::app::AgentRole::Chat => "You are an intelligent assistant with access to powerful tools. You can execute actions by outputting structured JSON blocks wrapped in ```tool fences.",
        crate::tui::app::AgentRole::Plan => "You are a software Architect and Planner. Your goal is to explore the codebase and write detailed Markdown implementation plans. Do NOT write code files or run modifying commands. Rely heavily on read_file and search_files. You can execute actions by outputting structured JSON blocks wrapped in ```tool fences.",
        crate::tui::app::AgentRole::Build => "You are a Builder. Your goal is to aggressively implement features. Do not ask for permission to write code or run commands; just use the write_file and run_command tools to get the job done quickly. You can execute actions by outputting structured JSON blocks wrapped in ```tool fences.",
    };

    let mut prompt = format!(r#"{}

## Available Tools

### 1. Read File
Read any file on the computer:
```tool
{{"tool": "read_file", "path": "/absolute/path/to/file"}}
```

### 2. Write File
Create or overwrite any file on the computer:
```tool
{{"tool": "write_file", "path": "/absolute/path/to/file", "content": "file contents here"}}
```

### 3. Run Command
Execute any terminal/shell command:
```tool
{{"tool": "run_command", "command": "cargo build", "working_dir": "/optional/path"}}
```

### 4. Search Files
Search for text patterns in workspace files:
```tool
{{"tool": "search_files", "query": "search term", "file_pattern": "*.rs"}}
```

### 5. Web Search
Search the internet for information:
```tool
{{"tool": "web_search", "query": "search query here"}}
```
"#, persona);

    if !mcp_tools.is_empty() {
        prompt.push_str("\n### External MCP Tools\n");
        for (i, (server, tool)) in mcp_tools.iter().enumerate() {
            prompt.push_str(&format!(
                "#### 6.{} {} ({})\n{}\n```tool\n{{\"tool\": \"mcp_tool\", \"server\": \"{}\", \"name\": \"{}\", \"args\": {}}}\n```\n",
                i + 1,
                tool.name,
                server,
                tool.description,
                server,
                tool.name,
                tool.input_schema
            ));
        }
    }

    prompt.push_str(r#"
## Rules
- You can use multiple tools in a single response
- Each tool call must be in its own ```tool block
- The user will see a confirmation prompt before any tool is executed
- Tool results will be returned to you so you can use them in your response
- For file paths, use absolute paths when possible
- When editing files, always read them first to understand the current content"#);

    prompt
}

#[cfg(test)]
mod tests {
    use super::{BashTool, FsReadTool, FsWriteTool, Tool};
    use crate::agent::workspace::Workspace;

    #[tokio::test]
    async fn fs_read_and_write_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::new(dir.path()).expect("workspace");
        FsWriteTool
            .execute(
                serde_json::json!({"path": "sample.txt", "contents": "sample"}),
                &workspace,
            )
            .await
            .expect("write");
        let read = FsReadTool
            .execute(serde_json::json!({"path": "sample.txt"}), &workspace)
            .await
            .expect("read");
        assert_eq!(read["contents"], "sample");
    }

    #[tokio::test]
    async fn bash_tool_times_out_slow_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::new(dir.path()).expect("workspace");
        #[cfg(windows)]
        let command = "powershell -NoProfile -Command Start-Sleep -Seconds 20";
        #[cfg(not(windows))]
        let command = "sleep 20";

        let result = BashTool
            .execute(serde_json::json!({"command": command}), &workspace)
            .await;
        assert!(result.is_err());
    }
}
