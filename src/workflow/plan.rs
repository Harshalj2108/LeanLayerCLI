#[derive(Clone, Debug, PartialEq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct WorkflowStep {
    pub id: uuid::Uuid,
    pub description: String,
    pub status: StepStatus,
    pub generated_patch: Option<crate::diff::engine::FilePatch>,
}

pub struct WorkflowPlan {
    pub goal: String,
    pub steps: Vec<WorkflowStep>,
    pub current_step_index: usize,
}

impl WorkflowPlan {
    pub fn new(goal: String, steps: Vec<String>) -> Self {
        Self {
            goal,
            steps: steps
                .into_iter()
                .map(|description| WorkflowStep {
                    id: uuid::Uuid::new_v4(),
                    description,
                    status: StepStatus::Pending,
                    generated_patch: None,
                })
                .collect(),
            current_step_index: 0,
        }
    }

    pub fn advance(&mut self) -> Option<&WorkflowStep> {
        if self.current_step_index >= self.steps.len() {
            return None;
        }

        match self.steps[self.current_step_index].status {
            StepStatus::Pending => {
                self.steps[self.current_step_index].status = StepStatus::InProgress;
                return self.steps.get(self.current_step_index);
            }
            StepStatus::InProgress => {
                self.steps[self.current_step_index].status = StepStatus::Completed;
                self.current_step_index += 1;
            }
            StepStatus::Completed => {
                self.current_step_index += 1;
            }
            StepStatus::Failed(_) => return self.steps.get(self.current_step_index),
        }

        if self.current_step_index < self.steps.len() {
            self.steps[self.current_step_index].status = StepStatus::InProgress;
            self.steps.get(self.current_step_index)
        } else {
            None
        }
    }

    pub fn mark_failed(&mut self, reason: String) {
        if let Some(step) = self.steps.get_mut(self.current_step_index) {
            step.status = StepStatus::Failed(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StepStatus, WorkflowPlan};

    #[test]
    fn advance_transitions_steps_in_order() {
        let mut plan = WorkflowPlan::new(
            "goal".to_string(),
            vec!["one".to_string(), "two".to_string(), "three".to_string()],
        );
        assert_eq!(plan.steps[0].status, StepStatus::Pending);
        plan.advance();
        assert_eq!(plan.steps[0].status, StepStatus::InProgress);
        plan.advance();
        assert_eq!(plan.steps[0].status, StepStatus::Completed);
        assert_eq!(plan.steps[1].status, StepStatus::InProgress);
    }
}
