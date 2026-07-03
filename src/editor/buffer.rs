use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use crate::editor::errors::EditorError;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub char_idx: usize,
}

pub struct TextBuffer {
    pub file_path: Option<PathBuf>,
    pub(crate) rope: ropey::Rope,
    pub cursor: Cursor,
    pub mode: EditorMode,
    pub modified: bool,
}

impl TextBuffer {
    pub fn from_path(path: &Path) -> Result<Self, EditorError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let rope = ropey::Rope::from_reader(reader)?;
        Ok(Self {
            file_path: Some(path.to_path_buf()),
            rope,
            cursor: Cursor {
                line: 0,
                char_idx: 0,
            },
            mode: EditorMode::Normal,
            modified: false,
        })
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            file_path: None,
            rope: ropey::Rope::from_str(text),
            cursor: Cursor {
                line: 0,
                char_idx: 0,
            },
            mode: EditorMode::Normal,
            modified: false,
        }
    }

    pub fn insert_char(&mut self, ch: char) -> Result<(), EditorError> {
        if self.cursor.line >= self.rope.len_lines() {
            return Err(EditorError::BoundsError(
                self.cursor.line,
                self.cursor.char_idx,
            ));
        }
        let line_start = self.rope.line_to_char(self.cursor.line);
        let line = self.rope.line(self.cursor.line);
        if self.cursor.char_idx > line.len_chars() {
            return Err(EditorError::BoundsError(
                self.cursor.line,
                self.cursor.char_idx,
            ));
        }
        let char_index = line_start + self.cursor.char_idx;
        self.rope.insert_char(char_index, ch);
        self.cursor.char_idx += 1;
        self.modified = true;
        Ok(())
    }

    pub fn apply_patch(
        &mut self,
        patch: &crate::diff::engine::FilePatch,
    ) -> Result<(), EditorError> {
        let original = self.rope.to_string();
        let updated = crate::diff::engine::apply_patch(&original, &patch.unified_diff)
            .unwrap_or_else(|_| patch.new_content.clone());
        self.rope = ropey::Rope::from_str(&updated);
        self.modified = true;
        Ok(())
    }

    pub fn lines_for_view(&self, start_line: usize, count: usize) -> Vec<String> {
        let end = start_line.saturating_add(count).min(self.rope.len_lines());
        (start_line..end)
            .map(|line| self.rope.line(line).to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::TextBuffer;

    #[test]
    fn insert_char_updates_large_rope_view() {
        let text = (0..1000)
            .map(|idx| format!("line {}\n", idx))
            .collect::<String>();
        let mut buffer = TextBuffer::from_text(&text);
        buffer.cursor.line = 500;
        buffer.cursor.char_idx = 0;
        buffer.insert_char('X').expect("insert");
        let view = buffer.lines_for_view(500, 1);
        assert!(view[0].starts_with("Xline 500"));
    }
}
