#[derive(thiserror::Error, Debug)]
pub enum BootstrapError {
    #[error("Python 3.10+ not found in PATH")]
    PythonNotFound,
    #[error("Failed to create Virtual Environment: {0}")]
    VenvCreation(String),
    #[error("Dependency Installation Failed: {0}")]
    PipError(String),
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::BootstrapError;

    #[test]
    fn formats_python_error() {
        assert!(BootstrapError::PythonNotFound
            .to_string()
            .contains("Python 3.10+"));
    }
}
