#![allow(dead_code)]
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::fs;

/// Enhancement #9: Workspace file tree awareness
#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Enhancement #12: Git status information
#[derive(Debug, Clone)]
pub struct GitStatus {
    pub branch: String,
    pub modified_files: Vec<String>,
    pub untracked_files: Vec<String>,
}

/// Crawl a workspace directory, respecting .gitignore patterns
pub fn scan_workspace(root: &Path, max_depth: usize) -> Result<Vec<WorkspaceFile>> {
    let mut files = Vec::new();
    let gitignore_path = root.join(".gitignore");

    // Load .gitignore patterns
    let ignore_patterns: Vec<String> = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)?
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| l.trim().to_string())
            .collect()
    } else {
        Vec::new()
    };

    // Always ignore these
    let default_ignores = vec![
        ".git", "node_modules", "target", "__pycache__", ".venv",
        ".env", ".DS_Store", "Thumbs.db",
    ];

    scan_dir_recursive(root, root, &ignore_patterns, &default_ignores, 0, max_depth, &mut files)?;

    // Sort: directories first, then alphabetical
    files.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir)
            .then(a.relative_path.cmp(&b.relative_path))
    });

    Ok(files)
}

fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    ignore_patterns: &[String],
    default_ignores: &[&str],
    depth: usize,
    max_depth: usize,
    files: &mut Vec<WorkspaceFile>,
) -> Result<()> {
    if depth > max_depth {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Check default ignores
        if default_ignores.iter().any(|i| name == *i) {
            continue;
        }

        // Check .gitignore patterns (simple glob matching)
        let relative = path.strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        if should_ignore(&relative, &name, ignore_patterns) {
            continue;
        }

        let is_dir = path.is_dir();
        let size = if is_dir { 0 } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };

        files.push(WorkspaceFile {
            path: path.clone(),
            relative_path: relative,
            is_dir,
            size,
        });

        if is_dir {
            scan_dir_recursive(root, &path, ignore_patterns, default_ignores, depth + 1, max_depth, files)?;
        }
    }

    Ok(())
}

fn should_ignore(relative_path: &str, name: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        let p = pattern.trim_start_matches('/');
        // Simple pattern matching
        if p.ends_with('/') {
            // Directory pattern
            let dir_name = p.trim_end_matches('/');
            if name == dir_name {
                return true;
            }
        } else if p.starts_with("*.") {
            // Extension pattern
            let ext = p.trim_start_matches("*.");
            if name.ends_with(&format!(".{}", ext)) {
                return true;
            }
        } else if name == p || relative_path == p {
            return true;
        }
    }
    false
}

/// Build a tree-formatted string of the workspace for display
pub fn format_file_tree(files: &[WorkspaceFile]) -> String {
    let mut output = String::new();
    for file in files {
        let depth = file.relative_path.matches('/').count();
        let indent = "  ".repeat(depth);
        let icon = if file.is_dir { "📁" } else { "📄" };
        let size_str = if file.is_dir {
            String::new()
        } else {
            format!(" ({})", format_size(file.size))
        };

        let name = file.relative_path.split('/').last()
            .unwrap_or(&file.relative_path);

        output.push_str(&format!("{}{} {}{}\n", indent, icon, name, size_str));
    }
    output
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Enhancement #10: Generate a unified diff between two strings
pub fn generate_diff(original: &str, modified: &str, filename: &str) -> String {
    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n", filename));
    diff.push_str(&format!("+++ b/{}\n", filename));

    let original_lines: Vec<&str> = original.lines().collect();
    let modified_lines: Vec<&str> = modified.lines().collect();

    // Simple line-by-line diff using longest common subsequence approach
    let mut i = 0;
    let mut j = 0;
    let _hunk_start = true;

    while i < original_lines.len() || j < modified_lines.len() {
        if i < original_lines.len() && j < modified_lines.len() && original_lines[i] == modified_lines[j] {
            diff.push_str(&format!(" {}\n", original_lines[i]));
            i += 1;
            j += 1;
        } else if j < modified_lines.len() && (i >= original_lines.len() ||
            !original_lines[i..].contains(&modified_lines[j])) {
            diff.push_str(&format!("+{}\n", modified_lines[j]));
            j += 1;
        } else if i < original_lines.len() {
            diff.push_str(&format!("-{}\n", original_lines[i]));
            i += 1;
        }
    }

    diff
}

/// Enhancement #10: Apply a simple search-and-replace patch
pub fn apply_search_replace(file_content: &str, search: &str, replace: &str) -> Option<String> {
    if file_content.contains(search) {
        Some(file_content.replacen(search, replace, 1))
    } else {
        None
    }
}

/// Enhancement #12: Get basic git status using command line
pub fn get_git_status(workspace_dir: &Path) -> Option<GitStatus> {
    // Get current branch
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace_dir)
        .output()
        .ok()?;

    let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();
    if branch.is_empty() {
        return None;
    }

    // Get modified files
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace_dir)
        .output()
        .ok()?;

    let status_text = String::from_utf8_lossy(&status_output.stdout);
    let mut modified_files = Vec::new();
    let mut untracked_files = Vec::new();

    for line in status_text.lines() {
        if line.len() < 4 { continue; }
        let status = &line[..2];
        let file = line[3..].trim().to_string();

        if status.contains('?') {
            untracked_files.push(file);
        } else {
            modified_files.push(file);
        }
    }

    Some(GitStatus {
        branch,
        modified_files,
        untracked_files,
    })
}

/// Enhancement #12: Create a git commit with a message
pub fn git_commit(workspace_dir: &Path, message: &str) -> Result<String> {
    // Stage all changes
    let add_output = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(workspace_dir)
        .output()?;

    if !add_output.status.success() {
        anyhow::bail!("git add failed: {}", String::from_utf8_lossy(&add_output.stderr));
    }

    // Commit
    let commit_output = std::process::Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(workspace_dir)
        .output()?;

    if commit_output.status.success() {
        Ok(String::from_utf8_lossy(&commit_output.stdout).trim().to_string())
    } else {
        anyhow::bail!("git commit failed: {}", String::from_utf8_lossy(&commit_output.stderr));
    }
}
