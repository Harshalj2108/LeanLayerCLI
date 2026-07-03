#[derive(thiserror::Error, Debug)]
pub enum EditorError {
    #[error("Out of Bounds Edit: Line {0}, Char {1}")]
    BoundsError(usize, usize),
    #[error("Tree-sitter Parsing Failed")]
    SyntaxError,
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Patch Error: {0}")]
    Patch(#[from] crate::diff::errors::DiffError),
}

#[cfg(test)]
mod tests {
    use super::EditorError;

    #[test]
    fn formats_bounds_error() {
        let err = EditorError::BoundsError(1, 2);
        assert!(err.to_string().contains("Line 1"));
    }
}
