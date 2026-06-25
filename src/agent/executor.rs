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

pub async fn execute_mcp_tool(
    mcp_clients: &std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<crate::agent::mcp::McpClient>>>,
    server: &str,
    name: &str,
    args: serde_json::Value
) -> Result<ExecutionStatus> {
    if let Some(client) = mcp_clients.get(server) {
        let client = client.lock().await;
        match client.call_tool(name, args).await {
            Ok(output) => Ok(ExecutionStatus::Completed { stdout: output, stderr: String::new(), exit_code: 0 }),
            Err(e) => Ok(ExecutionStatus::Failed(e.to_string())),
        }
    } else {
        Ok(ExecutionStatus::Failed(format!("MCP Server '{}' not found", server)))
    }
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
pub async fn execute_web_search(query: &str) -> Result<ExecutionStatus> {
    let encoded_query = query.replace(' ', "+");
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

    let client = reqwest::Client::new();
    let response = client.get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    match response {
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
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

fn parse_duckduckgo_html(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();

    let mut pos = 0;
    while results.len() < 5 {
        let link_marker = "class=\"result__a\"";
        let link_start = match html[pos..].find(link_marker) {
            Some(i) => pos + i,
            None => break,
        };

        let href_start = match html[..link_start].rfind("href=\"") {
            Some(i) => i + 6,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let href_end = match html[href_start..].find('"') {
            Some(i) => href_start + i,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let raw_url = &html[href_start..href_end];

        let title_start = match html[link_start..].find('>') {
            Some(i) => link_start + i + 1,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let title_end = match html[title_start..].find("</a>") {
            Some(i) => title_start + i,
            None => { pos = link_start + link_marker.len(); continue; }
        };
        let title = strip_html_tags(&html[title_start..title_end]).trim().to_string();

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

/// Search files in a directory for a regex pattern, respecting .gitignore.
/// Returns a Vec of (relative_path, line_number, matching_line) tuples.
pub fn search_files(
    root: &std::path::Path,
    pattern: &str,
    file_pattern: Option<&str>,
    max_results: usize,
) -> Result<ExecutionStatus> {
    if !root.exists() {
        return Ok(ExecutionStatus::Failed(format!("Directory not found: {}", root.display())));
    }

    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => return Ok(ExecutionStatus::Failed(format!("Invalid regex pattern: {}", e))),
    };

    let file_re = file_pattern.and_then(|p| regex::Regex::new(&glob_to_regex(p)).ok());

    let mut results = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .max_depth(Some(5))
        .follow_links(true)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > 1_000_000 {
            continue;
        }

        if let Some(ref fr) = file_re {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !fr.is_match(name) {
                continue;
            }
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let relative = entry.path().strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let lines: Vec<&str> = content.lines().collect();
        for (line_num, line) in lines.iter().enumerate() {
            if let Some(mat) = re.find(line) {
                let start = mat.start().saturating_sub(20);
                let end = (mat.end() + 80).min(line.len());
                let context = &line[start..end];
                results.push((relative.clone(), line_num + 1, context.to_string()));
                if results.len() >= max_results {
                    break;
                }
            }
        }

        if results.len() >= max_results {
            break;
        }
    }

    if results.is_empty() {
        return Ok(ExecutionStatus::Completed {
            stdout: "No matches found.".to_string(),
            stderr: String::new(),
            exit_code: 0,
        });
    }

    let mut output = format!("Found {} matches:\n\n", results.len());
    for (path, line_num, context) in results {
        output.push_str(&format!("{}:{}: {}\n", path, line_num, context.trim()));
    }

    Ok(ExecutionStatus::Completed {
        stdout: output,
        stderr: String::new(),
        exit_code: 0,
    })
}

fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::new();
    regex.push('^');
    for ch in glob.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' => regex.push_str("\\."),
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    regex
}

/// Read a file's current content for diff/preview purposes
pub fn read_file_for_diff(path: &str) -> Result<Option<String>> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(p)?))
}

/// Generate a preview diff between original and proposed content
pub fn generate_file_diff(path: &str, original: Option<&str>, proposed: &str) -> String {
    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n", path));
    diff.push_str(&format!("+++ b/{}\n", path));
    diff.push_str("@@ Diff Preview @@\n\n");

    if let Some(orig) = original {
        let orig_lines: Vec<&str> = orig.lines().collect();
        let proposed_lines: Vec<&str> = proposed.lines().collect();

        let has_changes = orig != proposed;

        if !has_changes {
            diff.push_str("(no changes)\n");
            return diff;
        }

        let context = 3usize;
        let mut changed_indices = Vec::new();

        let max_len = orig_lines.len().max(proposed_lines.len());
        for i in 0..max_len {
            let orig_line = orig_lines.get(i);
            let prop_line = proposed_lines.get(i);
            if orig_line != prop_line {
                changed_indices.push(i);
            }
        }

        if changed_indices.is_empty() {
            diff.push_str("(no changes)\n");
            return diff;
        }

        let max_display = 20usize;
        if changed_indices.len() > max_display {
            diff.push_str(&format!("Too many changes to display ({} lines changed).", changed_indices.len()));
        } else {
            let mut last_end = None;
            for &idx in &changed_indices {
                let start = idx.saturating_sub(context);
                let end = (idx + context + 1).min(max_len);

                if let Some(le) = last_end {
                    if start > le { diff.push_str("...\n"); }
                }

                for i in start..end {
                    let orig_opt = orig_lines.get(i);
                    let prop_opt = proposed_lines.get(i);

                    if let (Some(a), Some(b)) = (orig_opt, prop_opt) {
                        if a != b {
                            diff.push_str(&format!("-{}\n", a));
                            diff.push_str(&format!("+{}\n", b));
                        } else {
                            diff.push_str(&format!(" {}\n", a));
                        }
                    } else if let Some(a) = orig_opt {
                        diff.push_str(&format!("-{}\n", a));
                    } else if let Some(b) = prop_opt {
                        diff.push_str(&format!("+{}\n", b));
                    }
                }
                last_end = Some(end);
            }
        }
    } else {
        diff.push_str("(new file)\n");
        let preview_lines: Vec<&str> = proposed.lines().take(20).collect();
        for line in preview_lines {
            diff.push_str(&format!("+{}\n", line));
        }
        if proposed.lines().count() > 20 {
            diff.push_str(&format!("... ({} more lines)\n", proposed.lines().count() - 20));
        }
    }

    diff
}
