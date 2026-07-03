use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::editor::errors::EditorError;

pub struct SyntaxHighlighter {
    parser: tree_sitter::Parser,
    tree: Option<tree_sitter::Tree>,
    language: String,
}

impl SyntaxHighlighter {
    pub fn new(language: &str) -> Result<Self, EditorError> {
        let mut parser = tree_sitter::Parser::new();
        match language {
            "rust" | "rs" => parser
                .set_language(&tree_sitter_rust::language())
                .map_err(|_| EditorError::SyntaxError)?,
            "python" | "py" => parser
                .set_language(&tree_sitter_python::language())
                .map_err(|_| EditorError::SyntaxError)?,
            _ => return Err(EditorError::SyntaxError),
        }
        Ok(Self {
            parser,
            tree: None,
            language: language.to_string(),
        })
    }

    pub fn highlight<'a>(
        &mut self,
        rope: &ropey::Rope,
        start_line: usize,
        count: usize,
    ) -> Vec<Line<'a>> {
        let source = rope.to_string();
        self.tree = self.parser.parse(&source, self.tree.as_ref());
        let end = start_line.saturating_add(count).min(rope.len_lines());
        (start_line..end)
            .map(|line_idx| style_line(&rope.line(line_idx).to_string(), &self.language))
            .collect()
    }
}

fn style_line<'a>(line: &str, language: &str) -> Line<'a> {
    let keywords = if language.starts_with("py") || language == "python" {
        ["def", "class", "import", "from", "return", "async", "await"].as_slice()
    } else {
        ["fn", "struct", "enum", "impl", "pub", "use", "async", "await"].as_slice()
    };

    let mut spans = Vec::new();
    let mut first = true;
    for word in line.split_inclusive(' ') {
        if !first {
            // split_inclusive keeps spaces in the previous word, so no separator is needed.
        }
        first = false;
        let trimmed = word.trim();
        let style = if keywords.contains(&trimmed) {
            Style::default().fg(Color::Cyan)
        } else if trimmed.starts_with("//") || trimmed.starts_with('#') {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        };
        spans.push(Span::styled(word.to_string(), style));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::SyntaxHighlighter;

    #[test]
    fn rust_highlighter_returns_lines() {
        let mut highlighter = SyntaxHighlighter::new("rust").expect("highlighter");
        let rope = ropey::Rope::from_str("pub fn main() {}\n");
        let lines = highlighter.highlight(&rope, 0, 1);
        assert_eq!(lines.len(), 1);
    }
}
