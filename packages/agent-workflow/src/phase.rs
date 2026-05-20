use crate::error::ControllerError;
use crate::model::{Criterion, WorkflowDefinition};
use crate::paths::ProjectPaths;
use crate::run::load_run;
use crate::run_state::PhaseStatus;
use serde::{Deserialize, Serialize};

/// Information about the current phase of a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseInfo {
    pub run_id: String,
    pub workflow_id: String,
    pub current_phase_id: String,
    pub phase_name: String,
    pub phase_description: String,
    pub entry_criteria: Vec<Criterion>,
    pub exit_criteria: Vec<Criterion>,
    pub operator_approval_required: bool,
    pub phase_status: PhaseStatus,
    pub active_task_id: Option<String>,
}

/// Show the current phase definition and status for a run.
///
/// Loads the run state and then the corresponding workflow definition
/// (YAML preferred, JSON fallback).
pub fn show_phase(paths: &ProjectPaths, run_id: &str) -> Result<PhaseInfo, ControllerError> {
    let state = load_run(paths, run_id)?;

    let current_phase_id = state.current_phase.as_ref().ok_or_else(|| {
        ControllerError::WorkflowAlreadyStopped {
            run_id: run_id.to_string(),
            phase_status: format!("{:?}", state.phase_status),
        }
    })?;

    // Load definition: try YAML first, then JSON
    let workflow = {
        let yaml_path = paths.workflow_yaml(&state.workflow_id);
        let json_path = paths.workflow_json(&state.workflow_id);
        if yaml_path.exists() {
            WorkflowDefinition::from_yaml_file(&yaml_path)?
        } else if json_path.exists() {
            WorkflowDefinition::from_json_file(&json_path)?
        } else {
            return Err(ControllerError::WorkflowDefinitionNotFound {
                workflow_id: state.workflow_id.clone(),
            });
        }
    };

    let phase = workflow
        .phases
        .iter()
        .find(|p| p.phase_id == *current_phase_id)
        .ok_or_else(|| ControllerError::UnknownWorkflowError {
            message: format!(
                "Phase '{}' not found in workflow definition for '{}'",
                current_phase_id, state.workflow_id
            ),
        })?;

    Ok(PhaseInfo {
        run_id: run_id.to_string(),
        workflow_id: state.workflow_id,
        current_phase_id: current_phase_id.clone(),
        phase_name: phase.name.clone(),
        phase_description: phase.description.clone(),
        entry_criteria: phase.entry_criteria.clone(),
        exit_criteria: phase.exit_criteria.clone(),
        operator_approval_required: phase.operator_approval_required,
        phase_status: state.phase_status,
        active_task_id: state.active_task_id,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Phase, RetryPolicy, TimeoutPolicy, WorkflowDefinition,
    };
    use crate::run::init_run;
    use crate::paths::ProjectPaths;

    fn dummy_workflow() -> WorkflowDefinition {
        WorkflowDefinition {
            workflow_id: "show_phase_test".to_string(),
            name: "Show Phase Test".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            adapter_profile: "default".to_string(),
            phases: vec![
                Phase {
                    phase_id: "p1".to_string(),
                    name: "First".to_string(),
                    description: "The first phase".to_string(),
                    entry_criteria: vec![],
                    exit_criteria: vec![],
                    operator_approval_required: false,
                    verification_required: false,
                    allowed_task_types: vec![],
                    max_phase_duration_minutes: Some(30),
                },
                Phase {
                    phase_id: "p2".to_string(),
                    name: "Second".to_string(),
                    description: "The second phase".to_string(),
                    entry_criteria: vec![],
                    exit_criteria: vec![],
                    operator_approval_required: true,
                    verification_required: false,
                    allowed_task_types: vec![],
                    max_phase_duration_minutes: Some(20),
                },
            ],
            timeout_policy: TimeoutPolicy {
                default_phase_timeout_minutes: 60,
                total_workflow_timeout_minutes: 120,
                on_timeout: "fail".to_string(),
            },
            retry_policy: RetryPolicy {
                workflow_max_retries: 3,
                sequential_task_failure_threshold: 2,
            },
            stop_conditions: vec!["all_phases_completed".to_string()],
        }
    }

    #[test]
    fn test_show_phase_loads_current_phase() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let workflow = dummy_workflow();

        // Write the workflow definition YAML so show_phase can find it
        let yaml = serde_yaml::to_string(&workflow).expect("serialize");
        std::fs::create_dir_all(tmp.path().join(".agent/workflows")).expect("create dir");
        std::fs::write(
            tmp.path().join(".agent/workflows/show_phase_test.yml"),
            yaml,
        )
        .expect("write");

        let run_id = init_run(&paths, &workflow.workflow_id, "default", &workflow).expect("init_run");

        let info = show_phase(&paths, &run_id).expect("show_phase");
        assert_eq!(info.current_phase_id, "p1");
        assert_eq!(info.phase_name, "First");
        assert_eq!(info.phase_status, PhaseStatus::Waiting);
        assert_eq!(info.operator_approval_required, false);
        assert_eq!(info.workflow_id, "show_phase_test");
    }

    #[test]
    fn test_show_phase_missing_run() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let result = show_phase(&paths, "no_such_run");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), "UNKNOWN_WORKFLOW_ERROR");
    }
}
