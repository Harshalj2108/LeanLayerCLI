#[derive(thiserror::Error, Debug)]
pub enum DiffError {
    #[error("Patch Application Failed: {0}")]
    Conflict(String),
    #[error("Malformed Patch Format")]
    Malformed,
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::DiffError;

    #[test]
    fn formats_conflict() {
        let err = DiffError::Conflict("line mismatch".to_string());
        assert!(err.to_string().contains("Patch Application Failed"));
    }
}
