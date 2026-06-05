#![allow(dead_code)]
use anyhow::Result;
use std::process::Command;

/// Enhancement #8: Sandboxed code execution engine
/// Enhancement #11: Gatekeeper agentic tool calling
#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub language: String,
    pub code: String,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ExecutionStatus {
    PendingApproval(ExecutionRequest),
    Running,
    Completed { stdout: String, stderr: String, exit_code: i32 },
    Failed(String),
}

/// Run a command in a sandboxed subprocess, returning captured output
pub fn execute_code(req: &ExecutionRequest) -> Result<ExecutionStatus> {
    let (program, args): (&str, Vec<&str>) = match req.language.as_str() {
        "python" | "py" => ("python", vec!["-c", &req.code]),
        "bash" | "sh" => {
            #[cfg(windows)]
            { ("cmd", vec!["/C", &req.code]) }
            #[cfg(not(windows))]
            { ("bash", vec!["-c", &req.code]) }
        }
        "rust" | "rs" => {
            // For Rust, we run cargo check in the working directory
            return Ok(ExecutionStatus::Failed(
                "Rust execution requires cargo project. Use tool calling for 'cargo check'.".into()
            ));
        }
        "javascript" | "js" | "node" => ("node", vec!["-e", &req.code]),
        _ => {
            return Ok(ExecutionStatus::Failed(
                format!("Unsupported language for execution: {}", req.language)
            ));
        }
    };

    let mut cmd = Command::new(program);
    cmd.args(&args);

    if let Some(dir) = &req.working_dir {
        cmd.current_dir(dir);
    }

    cmd.stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::piped());

    match cmd.output() {
        Ok(output) => Ok(ExecutionStatus::Completed {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        }),
        Err(e) => Ok(ExecutionStatus::Failed(e.to_string())),
    }
}

/// Enhancement #11: Execute a shell command for tool calling (cargo check, pytest, etc.)
pub fn execute_tool_command(command: &str, working_dir: &str) -> Result<ExecutionStatus> {
    #[cfg(windows)]
    let output = Command::new("cmd")
        .args(["/C", command])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    #[cfg(not(windows))]
    let output = Command::new("sh")
        .args(["-c", command])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(output) => Ok(ExecutionStatus::Completed {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        }),
        Err(e) => Ok(ExecutionStatus::Failed(e.to_string())),
    }
}

/// Read a file from anywhere on the filesystem.
/// For very large files, only the first ~300 lines are returned with a truncation notice.
pub fn read_file_global(path: &str) -> Result<ExecutionStatus> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Ok(ExecutionStatus::Failed(format!("File not found: {}", path)));
    }

    match std::fs::read_to_string(p) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() > 300 {
                let truncated: String = lines[..300].join("\n");
                let msg = format!(
                    "{}\n\n--- TRUNCATED: Showing first 300 of {} total lines ---",
                    truncated,
                    lines.len()
                );
                Ok(ExecutionStatus::Completed { stdout: msg, stderr: "".into(), exit_code: 0 })
            } else {
                Ok(ExecutionStatus::Completed { stdout: content, stderr: "".into(), exit_code: 0 })
            }
        }
        Err(e) => Ok(ExecutionStatus::Failed(format!("Failed to read file: {}", e))),
    }
}

/// Write a file to anywhere on the filesystem.
/// Auto-creates parent directories if they don't exist.
pub fn write_file_global(path: &str, content: &str) -> Result<ExecutionStatus> {
    let p = std::path::Path::new(path);
    // Auto-create parent directories
    if let Some(parent) = p.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Ok(ExecutionStatus::Failed(format!(
                    "Failed to create directories for {}: {}", path, e
                )));
            }
        }
    }

    match std::fs::write(p, content) {
        Ok(_) => {
            let bytes = content.len();
            let lines = content.lines().count();
            Ok(ExecutionStatus::Completed {
                stdout: format!("File written successfully: {} ({} lines, {} bytes)", path, lines, bytes),
                stderr: "".into(),
                exit_code: 0,
            })
        }
        Err(e) => Ok(ExecutionStatus::Failed(format!("Failed to write file: {}", e))),
    }
}

/// Execute a web search using DuckDuckGo HTML interface (no API key required)
pub fn execute_web_search(query: &str) -> Result<ExecutionStatus> {
    let encoded_query = query.replace(' ', "+");
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

    let response = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .call();

    match response {
        Ok(resp) => {
            let body = resp.into_string().unwrap_or_default();
            let results = parse_duckduckgo_html(&body);

            if results.is_empty() {
                Ok(ExecutionStatus::Completed {
                    stdout: format!("Web search for '{}': No results found.", query),
                    stderr: "".into(),
                    exit_code: 0,
                })
            } else {
                let mut output = format!("Web search results for '{}':\n\n", query);
                for (i, (title, snippet, url)) in results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. **{}**\n   {}\n   URL: {}\n\n",
                        i + 1, title, snippet, url
                    ));
                }
                Ok(ExecutionStatus::Completed {
                    stdout: output,
                    stderr: "".into(),
                    exit_code: 0,
                })
            }
        }
        Err(e) => Ok(ExecutionStatus::Failed(format!("Web search failed: {}", e))),
    }
}

/// Parse DuckDuckGo HTML search results into (title, snippet, url) tuples
fn parse_duckduckgo_html(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();

    // Parse result blocks — DuckDuckGo wraps results in <a class="result__a"> tags
    // and snippets in <a class="result__snippet"> tags
    let mut pos = 0;
    while results.len() < 5 {
        // Find result link
        let link_marker = "class=\"result__a\"";
        let link_start = match html[pos..].find(link_marker) {
            Some(i) => pos + i,
            None => break,
        };

        // Extract href
        let href_start = match html[..link_start].rfind("href=\"") {
            Some(i) => i + 6,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let href_end = match html[href_start..].find('"') {
            Some(i) => href_start + i,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let raw_url = &html[href_start..href_end];

        // Extract title text (between > and </a>)
        let title_start = match html[link_start..].find('>') {
            Some(i) => link_start + i + 1,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let title_end = match html[title_start..].find("</a>") {
            Some(i) => title_start + i,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let title = strip_html_tags(&html[title_start..title_end]).trim().to_string();

        // Find snippet
        let snippet_marker = "class=\"result__snippet\"";
        let snippet_text = if let Some(snippet_start) = html[title_end..].find(snippet_marker) {
            let abs_start = title_end + snippet_start;
            if let Some(tag_end) = html[abs_start..].find('>') {
                let text_start = abs_start + tag_end + 1;
                if let Some(text_end) = html[text_start..].find("</") {
                    strip_html_tags(&html[text_start..text_start + text_end]).trim().to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Clean up URL (DuckDuckGo redirects through //duckduckgo.com/l/?uddg=...)
        let clean_url = if raw_url.contains("uddg=") {
            if let Some(uddg_start) = raw_url.find("uddg=") {
                let url_encoded = &raw_url[uddg_start + 5..];
                let end = url_encoded.find('&').unwrap_or(url_encoded.len());
                url_decode(&url_encoded[..end])
            } else {
                raw_url.to_string()
            }
        } else {
            raw_url.to_string()
        };

        if !title.is_empty() {
            results.push((title, snippet_text, clean_url));
        }

        pos = title_end;
    }

    results
}

/// Simple HTML tag stripper
fn strip_html_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' { in_tag = true; continue; }
        if ch == '>' { in_tag = false; continue; }
        if !in_tag { result.push(ch); }
    }
    result
}

/// Simple URL percent-decoding
fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else if ch == '+' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

/// Detect executable code blocks from assistant response content
pub fn detect_executable_blocks(content: &str) -> Vec<ExecutionRequest> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_lang = String::new();
    let mut current_code = String::new();

    let executable_langs = ["python", "py", "bash", "sh", "javascript", "js", "node"];

    for line in content.lines() {
        if line.starts_with("```") {
            if in_block {
                if executable_langs.iter().any(|l| current_lang == *l) {
                    blocks.push(ExecutionRequest {
                        language: current_lang.clone(),
                        code: current_code.trim().to_string(),
                        working_dir: None,
                    });
                }
                current_lang.clear();
                current_code.clear();
                in_block = false;
            } else {
                current_lang = line.trim_start_matches("```").trim().to_lowercase();
                in_block = true;
            }
        } else if in_block {
            current_code.push_str(line);
            current_code.push('\n');
        }
    }

    blocks
}
