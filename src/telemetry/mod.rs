pub mod errors;
pub mod logger;

#[cfg(test)]
mod tests {
    #[test]
    fn telemetry_error_type_is_available() {
        let _ = crate::telemetry::errors::TelemetryError::TracingInit;
    }
}
