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
#[derive(Clone)]
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

    pub fn insert_newline(&mut self) -> Result<(), EditorError> {
        self.insert_char('\n')?;
        self.cursor.line += 1;
        self.cursor.char_idx = 0;
        Ok(())
    }

    pub fn delete_char(&mut self) -> Result<(), EditorError> {
        let line_start = self.rope.line_to_char(self.cursor.line);
        let char_index = line_start + self.cursor.char_idx;
        if char_index > 0 {
            self.rope.remove(char_index - 1..char_index);
            if self.cursor.char_idx > 0 {
                self.cursor.char_idx -= 1;
            } else {
                self.cursor.line = self.cursor.line.saturating_sub(1);
                self.cursor.char_idx = self.rope.line(self.cursor.line).len_chars().saturating_sub(1);
            }
            self.modified = true;
        }
        Ok(())
    }

    pub fn move_cursor(&mut self, dx: isize, dy: isize) {
        if dy < 0 {
            self.cursor.line = self.cursor.line.saturating_sub((-dy) as usize);
        } else if dy > 0 {
            self.cursor.line = self.cursor.line.saturating_add(dy as usize).min(self.rope.len_lines().saturating_sub(1));
        }

        let line_len = self.rope.line(self.cursor.line).len_chars().saturating_sub(1); // excluding newline
        if dx < 0 {
            self.cursor.char_idx = self.cursor.char_idx.saturating_sub((-dx) as usize);
        } else if dx > 0 {
            self.cursor.char_idx = self.cursor.char_idx.saturating_add(dx as usize).min(line_len);
        } else {
            // Keep cursor bounded within the new line length when moving vertically
            self.cursor.char_idx = self.cursor.char_idx.min(line_len);
        }
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
