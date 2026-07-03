pub mod errors;
pub mod python_env;

#[cfg(test)]
mod tests {
    #[test]
    fn bootstrap_error_type_is_available() {
        let _ = crate::bootstrap::errors::BootstrapError::PythonNotFound;
    }
}
