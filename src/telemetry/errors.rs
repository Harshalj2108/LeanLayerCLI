#[derive(thiserror::Error, Debug)]
pub enum TelemetryError {
    #[error("Tracing Initialization Failed")]
    TracingInit,
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::TelemetryError;

    #[test]
    fn formats_tracing_error() {
        assert_eq!(
            TelemetryError::TracingInit.to_string(),
            "Tracing Initialization Failed"
        );
    }
}
