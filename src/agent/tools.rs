#![allow(dead_code)]
use serde::{Deserialize, Serialize};

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
    };

    let status = if success { "SUCCESS" } else { "FAILED" };
    format!("[Tool Result: {} ({})]\n{}\n[/Tool Result]", tool_name, status, output)
}

/// Build the system prompt that teaches the LLM how to use tools
pub fn build_tool_system_prompt() -> String {
    r#"You are an intelligent assistant with access to powerful tools. You can execute actions by outputting structured JSON blocks wrapped in ```tool fences.

## Available Tools

### 1. Read File
Read any file on the computer:
```tool
{"tool": "read_file", "path": "/absolute/path/to/file"}
```

### 2. Write File
Create or overwrite any file on the computer:
```tool
{"tool": "write_file", "path": "/absolute/path/to/file", "content": "file contents here"}
```

### 3. Run Command
Execute any terminal/shell command:
```tool
{"tool": "run_command", "command": "cargo build", "working_dir": "/optional/path"}
```

### 4. Search Files
Search for text patterns in workspace files:
```tool
{"tool": "search_files", "query": "search term", "file_pattern": "*.rs"}
```

### 5. Web Search
Search the internet for information:
```tool
{"tool": "web_search", "query": "search query here"}
```

## Rules
- You can use multiple tools in a single response
- Each tool call must be in its own ```tool block
- The user will see a confirmation prompt before any tool is executed
- Tool results will be returned to you so you can use them in your response
- For file paths, use absolute paths when possible
- When editing files, always read them first to understand the current content"#.to_string()
}
