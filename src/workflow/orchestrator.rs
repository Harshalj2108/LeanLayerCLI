use std::sync::Arc;

use crate::agent::mcp::McpSession;
use crate::agent::workspace::Workspace;
use crate::workflow::errors::WorkflowError;
use crate::workflow::plan::{StepStatus, WorkflowPlan};

pub struct WorkflowEngine {
    pub max_iterations: usize,
    pub agent_session: Arc<McpSession>,
}

impl WorkflowEngine {
    pub fn new(max_iterations: usize, agent_session: Arc<McpSession>) -> Self {
        Self {
            max_iterations,
            agent_session,
        }
    }

    pub async fn execute_plan(
        &self,
        mut plan: WorkflowPlan,
        workspace: &Workspace,
    ) -> Result<WorkflowPlan, WorkflowError> {
        let mut iteration_count = 0usize;

        while plan.current_step_index < plan.steps.len() {
            if iteration_count >= self.max_iterations {
                return Err(WorkflowError::MaxIterationsExceeded(self.max_iterations));
            }

            let current_index = plan.current_step_index;
            if matches!(plan.steps[current_index].status, StepStatus::Pending) {
                plan.advance();
            }

            if plan.steps[current_index]
                .description
                .to_ascii_lowercase()
                .contains("write")
            {
                let patch = crate::diff::engine::generate(
                    &workspace.base_path().join("workflow.patch.preview"),
                    "",
                    &format!("{}\n", plan.steps[current_index].description),
                );
                plan.steps[current_index].generated_patch = Some(patch);
                return Ok(plan);
            }

            plan.advance();
            iteration_count += 1;
        }

        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkflowEngine;
    use crate::agent::mcp::McpSession;
    use crate::agent::workspace::Workspace;
    use crate::workflow::errors::WorkflowError;
    use crate::workflow::plan::WorkflowPlan;
    use std::sync::Arc;

    #[tokio::test]
    async fn execute_plan_stops_at_max_iterations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::new(dir.path()).expect("workspace");
        let engine = WorkflowEngine::new(1, Arc::new(McpSession::new()));
        let plan = WorkflowPlan::new(
            "loop".to_string(),
            vec!["step one".to_string(), "step two".to_string()],
        );
        let err = engine
            .execute_plan(plan, &workspace)
            .await
            .expect_err("max iterations");
        assert!(matches!(err, WorkflowError::MaxIterationsExceeded(1)));
    }
}
