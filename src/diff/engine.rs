use std::path::{Path, PathBuf};

use similar::TextDiff;

use crate::diff::errors::DiffError;

#[derive(Clone, Debug)]
pub struct FilePatch {
    pub file_path: PathBuf,
    pub old_content: String,
    pub new_content: String,
    pub unified_diff: String,
}

pub fn generate(file_path: &Path, old_content: &str, new_content: &str) -> FilePatch {
    let diff = TextDiff::from_lines(old_content, new_content);
    let display_path = file_path.to_string_lossy();
    let unified_diff = diff
        .unified_diff()
        .header(&format!("a/{}", display_path), &format!("b/{}", display_path))
        .to_string();

    FilePatch {
        file_path: file_path.to_path_buf(),
        old_content: old_content.to_string(),
        new_content: new_content.to_string(),
        unified_diff,
    }
}

pub fn apply_patch(original: &str, patch: &str) -> Result<String, DiffError> {
    let original_lines = original.lines().map(ToString::to_string).collect::<Vec<_>>();
    let patch_lines = patch.lines().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut original_index = 0usize;
    let mut patch_index = 0usize;
    let mut saw_hunk = false;

    while patch_index < patch_lines.len() {
        let line = patch_lines[patch_index];
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            patch_index += 1;
            continue;
        }

        if !line.starts_with("@@") {
            patch_index += 1;
            continue;
        }

        saw_hunk = true;
        let (old_start, _old_count) = parse_hunk_header(line)?;
        let hunk_original_start = old_start.saturating_sub(1);
        if hunk_original_start < original_index {
            return Err(DiffError::Conflict(
                "hunk overlaps an already-applied region".to_string(),
            ));
        }

        while original_index < hunk_original_start {
            let Some(line) = original_lines.get(original_index) else {
                return Err(DiffError::Conflict("hunk starts beyond original".to_string()));
            };
            output.push(line.clone());
            original_index += 1;
        }

        patch_index += 1;
        while patch_index < patch_lines.len() && !patch_lines[patch_index].starts_with("@@") {
            let hunk_line = patch_lines[patch_index];
            if hunk_line.starts_with("--- ") || hunk_line.starts_with("+++ ") {
                break;
            }
            let (marker, text) = hunk_line.split_at(1);
            match marker {
                " " => {
                    let original_line = original_lines.get(original_index).ok_or_else(|| {
                        DiffError::Conflict("context extends beyond original".to_string())
                    })?;
                    if original_line != text {
                        return Err(DiffError::Conflict(format!(
                            "context mismatch: expected {:?}, found {:?}",
                            text, original_line
                        )));
                    }
                    output.push(original_line.clone());
                    original_index += 1;
                }
                "-" => {
                    let original_line = original_lines.get(original_index).ok_or_else(|| {
                        DiffError::Conflict("removal extends beyond original".to_string())
                    })?;
                    if original_line != text {
                        return Err(DiffError::Conflict(format!(
                            "removal mismatch: expected {:?}, found {:?}",
                            text, original_line
                        )));
                    }
                    original_index += 1;
                }
                "+" => output.push(text.to_string()),
                "\\" => {}
                _ => return Err(DiffError::Malformed),
            }
            patch_index += 1;
        }
    }

    if !saw_hunk {
        return Err(DiffError::Malformed);
    }

    while original_index < original_lines.len() {
        output.push(original_lines[original_index].clone());
        original_index += 1;
    }

    let mut result = output.join("\n");
    if original.ends_with('\n') || patch.contains("\n\\ No newline at end of file") {
        result.push('\n');
    }
    Ok(result)
}

fn parse_hunk_header(header: &str) -> Result<(usize, usize), DiffError> {
    let mut parts = header.split_whitespace();
    let first = parts.next();
    let old = parts.next();
    if first != Some("@@") {
        return Err(DiffError::Malformed);
    }
    let old = old
        .and_then(|part| part.strip_prefix('-'))
        .ok_or(DiffError::Malformed)?;
    let mut old_parts = old.split(',');
    let start = old_parts
        .next()
        .ok_or(DiffError::Malformed)?
        .parse::<usize>()
        .map_err(|_| DiffError::Malformed)?;
    let count = old_parts
        .next()
        .unwrap_or("1")
        .parse::<usize>()
        .map_err(|_| DiffError::Malformed)?;
    Ok((start, count))
}

#[cfg(test)]
mod tests {
    use super::{apply_patch, generate};
    use crate::diff::errors::DiffError;

    #[test]
    fn generate_contains_unified_hunk_header() {
        let old = "one\ntwo\nthree\nfour\nfive\n";
        let new = "one\ntwo\nTHREE\nfour\nfive\n";
        let patch = generate(std::path::Path::new("file.txt"), old, new);
        assert!(patch.unified_diff.contains("@@ -1,5 +1,5 @@"));
    }

    #[test]
    fn apply_patch_detects_conflict() {
        let patch = "--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n nope\n+new\n";
        let err = apply_patch("one\ntwo\n", patch).expect_err("conflict");
        assert!(matches!(err, DiffError::Conflict(_)));
    }
}
