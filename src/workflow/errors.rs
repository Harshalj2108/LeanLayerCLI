#[derive(thiserror::Error, Debug)]
pub enum WorkflowError {
    #[error("Max Autonomous Iterations Reached ({0})")]
    MaxIterationsExceeded(usize),
    #[error("Workflow Step Failed: {0}")]
    StepFailure(String),
    #[error("Agent Execution Error: {0}")]
    Agent(#[from] crate::agent::errors::AgentError),
}

#[cfg(test)]
mod tests {
    use super::WorkflowError;

    #[test]
    fn formats_iteration_error() {
        let err = WorkflowError::MaxIterationsExceeded(2);
        assert!(err.to_string().contains("2"));
    }
}
