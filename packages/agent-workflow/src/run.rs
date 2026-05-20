use crate::error::ControllerError;
use crate::model::WorkflowDefinition;
use crate::paths::ProjectPaths;
use crate::run_state::{
    PhaseStatus, WorkflowRetryCounters, WorkflowRunState,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Load / Save
// ---------------------------------------------------------------------------

/// Load workflow run state from `.agent/workflow_runs/<run_id>/run_state.json`.
pub fn load_run(paths: &ProjectPaths, run_id: &str) -> Result<WorkflowRunState, ControllerError> {
    let run_state_path = paths.run_state_file(run_id);
    if !run_state_path.exists() {
        return Err(ControllerError::UnknownWorkflowError {
            message: format!("Run '{}' not found", run_id),
        });
    }
    let contents = std::fs::read_to_string(&run_state_path).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to read run state '{}': {}", run_id, e),
        }
    })?;
    let state: WorkflowRunState = serde_json::from_str(&contents).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to parse run state '{}': {}", run_id, e),
        }
    })?;
    Ok(state)
}

/// Save workflow run state atomically via temp file + rename.
pub fn save_run_state(
    paths: &ProjectPaths,
    run_id: &str,
    state: &WorkflowRunState,
) -> Result<(), ControllerError> {
    let run_state_path = paths.run_state_file(run_id);
    let json = serde_json::to_string_pretty(state).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to serialize run state '{}': {}", run_id, e),
        }
    })?;

    let temp_path = run_state_path.with_extension("tmp");
    std::fs::write(&temp_path, &json).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to write run state '{}': {}", run_id, e),
        }
    })?;
    std::fs::rename(&temp_path, &run_state_path).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to finalize run state '{}': {}", run_id, e),
        }
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// init_run
// ---------------------------------------------------------------------------

/// Initialize a new workflow run.
///
/// Generates `run_id`, creates per-run directory structure, and writes
/// `run_state.json` with initial `WAITING` state for the first phase.
pub fn init_run(
    paths: &ProjectPaths,
    workflow_id: &str,
    profile: &str,
    workflow: &WorkflowDefinition,
) -> Result<String, ControllerError> {
    let run_id = format!(
        "run_{}_{}",
        Utc::now().format("%Y-%m-%dT%H-%M-%S"),
        Uuid::new_v4()
    );

    // Create directory structure
    std::fs::create_dir_all(paths.run_dir(&run_id)).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to create run dir for '{}': {}", run_id, e),
        }
    })?;
    std::fs::create_dir_all(paths.task_packets_dir(&run_id)).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!(
                "Failed to create task_packets dir for '{}': {}",
                run_id, e
            ),
        }
    })?;
    std::fs::create_dir_all(paths.result_packets_dir(&run_id)).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!(
                "Failed to create result_packets dir for '{}': {}",
                run_id, e
            ),
        }
    })?;
    std::fs::create_dir_all(paths.artifacts_dir(&run_id)).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!(
                "Failed to create artifacts dir for '{}': {}",
                run_id, e
            ),
        }
    })?;

    let first_phase = workflow.phases.first().ok_or_else(|| {
        ControllerError::InvalidWorkflowDefinition {
            message: "Workflow definition has no phases".to_string(),
        }
    })?;

    let now = Utc::now().to_rfc3339();
    let state = WorkflowRunState {
        workflow_run_id: run_id.clone(),
        workflow_id: workflow_id.to_string(),
        workflow_version: workflow.version.clone(),
        adapter_profile: profile.to_string(),
        current_phase: Some(first_phase.phase_id.clone()),
        phase_status: PhaseStatus::Waiting,
        active_task_id: None,
        active_task_graph_revision: None,
        active_task_lease_expires_at: None,
        active_task_packet_ref: None,
        start_time: now.clone(),
        updated_time: now,
        pause_reason: None,
        stop_reason: None,
        workflow_retry_counters: WorkflowRetryCounters {
            total_attempts: 0,
            sequential_task_failures: 0,
            max_workflow_retries: workflow.retry_policy.workflow_max_retries,
        },
        approval_records: vec![],
        phase_history: vec![],
        run_artifacts: vec![],
    };

    save_run_state(paths, &run_id, &state)?;

    Ok(run_id)
}

// ---------------------------------------------------------------------------
// list_runs
// ---------------------------------------------------------------------------

/// Summary entry for `list_runs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub workflow_id: String,
    pub current_phase: Option<String>,
    pub phase_status: String,
    pub active_task_id: Option<String>,
}

/// List all workflow runs, optionally filtered by `workflow_id`.
pub fn list_runs(
    paths: &ProjectPaths,
    workflow_id: Option<&str>,
) -> Result<Vec<RunSummary>, ControllerError> {
    let runs_dir = paths.workflow_runs_dir();
    if !runs_dir.exists() {
        return Ok(vec![]);
    }

    let mut summaries = Vec::new();
    let entries = std::fs::read_dir(&runs_dir).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to read runs directory: {}", e),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| ControllerError::UnknownWorkflowError {
            message: format!("Failed to read runs directory entry: {}", e),
        })?;
        let run_id = entry.file_name().to_string_lossy().to_string();
        let run_state_path = paths.run_state_file(&run_id);
        if run_state_path.exists() {
            if let Ok(state) = load_run(paths, &run_id) {
                let match_filter = match workflow_id {
                    Some(w) => w == state.workflow_id,
                    None => true,
                };
                if match_filter {
                    summaries.push(RunSummary {
                        run_id: state.workflow_run_id,
                        workflow_id: state.workflow_id,
                        current_phase: state.current_phase,
                        phase_status: format!("{:?}", state.phase_status),
                        active_task_id: state.active_task_id,
                    });
                }
            }
        }
    }

    Ok(summaries)
}

// ---------------------------------------------------------------------------
// cancel_run
// ---------------------------------------------------------------------------

/// Cancel a workflow run.
///
/// If an `active_task_id` exists, returns `CANNOT_RELEASE_TASK` (deferred to PR 9).
/// Otherwise marks the run as `CANCELLED`.
pub fn cancel_run(
    paths: &ProjectPaths,
    run_id: &str,
    reason: &str,
) -> Result<(), ControllerError> {
    let mut state = load_run(paths, run_id)?;

    if let Some(ref task_id) = state.active_task_id {
        return Err(ControllerError::CannotReleaseTask {
            run_id: run_id.to_string(),
            task_id: task_id.clone(),
            reason: "Cannot cancel: active task exists. Use agent-adapter release-work first."
                .to_string(),
        });
    }

    let stop_reason = if reason.is_empty() {
        "operator_cancelled"
    } else {
        reason
    };

    state.phase_status = PhaseStatus::Cancelled;
    state.stop_reason = Some(stop_reason.to_string());
    state.updated_time = Utc::now().to_rfc3339();

    save_run_state(paths, run_id, &state)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_workflow() -> WorkflowDefinition {
        WorkflowDefinition {
            workflow_id: "test_workflow".to_string(),
            name: "Test".to_string(),
            description: "Test workflow".to_string(),
            version: "1.0.0".to_string(),
            adapter_profile: "default".to_string(),
            phases: vec![crate::model::Phase {
                phase_id: "p1".to_string(),
                name: "Phase 1".to_string(),
                description: "desc".to_string(),
                entry_criteria: vec![],
                exit_criteria: vec![],
                operator_approval_required: false,
                verification_required: false,
                allowed_task_types: vec![],
                max_phase_duration_minutes: Some(30),
            }],
            timeout_policy: crate::model::TimeoutPolicy {
                default_phase_timeout_minutes: 60,
                total_workflow_timeout_minutes: 120,
                on_timeout: "fail".to_string(),
            },
            retry_policy: crate::model::RetryPolicy {
                workflow_max_retries: 3,
                sequential_task_failure_threshold: 2,
            },
            stop_conditions: vec!["all_phases_completed".to_string()],
        }
    }

    #[test]
    fn test_init_run_creates_directories_and_state() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let workflow = dummy_workflow();

        let run_id = init_run(&paths,
            &workflow.workflow_id,
            "default",
            &workflow,
        )
        .expect("init_run");

        assert!(paths.run_dir(&run_id).exists());
        assert!(paths.task_packets_dir(&run_id).exists());
        assert!(paths.result_packets_dir(&run_id).exists());
        assert!(paths.artifacts_dir(&run_id).exists());
        assert!(paths.run_state_file(&run_id).exists());

        let state = load_run(&paths, &run_id).expect("load_run");
        assert_eq!(state.workflow_id, "test_workflow");
        assert_eq!(state.current_phase, Some("p1".to_string()));
        assert_eq!(state.phase_status, PhaseStatus::Waiting);
        assert_eq!(state.active_task_id, None);
        assert_eq!(state.workflow_retry_counters.total_attempts, 0);
    }

    #[test]
    fn test_init_run_is_atomic() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let workflow = dummy_workflow();

        let run_id = init_run(&paths, &workflow.workflow_id, "default", &workflow).expect("init_run");

        // After init_run returns, the run_state.json must exist (atomicity enforced)
        let state_path = paths.run_state_file(&run_id);
        assert!(state_path.exists());

        // Verify valid JSON
        let contents = std::fs::read_to_string(&state_path).expect("read");
        let _parsed: serde_json::Value = serde_json::from_str(&contents).expect("valid json");
    }

    #[test]
    fn test_load_run_missing() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let result = load_run(&paths, "nonexistent_run");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), "UNKNOWN_WORKFLOW_ERROR");
    }

    #[test]
    fn test_list_runs_filters_by_workflow_id() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let wf1 = dummy_workflow();
        let run_id1 = init_run(&paths, &wf1.workflow_id, "p1", &wf1).expect("init_run1");

        let mut wf2 = dummy_workflow();
        wf2.workflow_id = "wf2".to_string();
        let run_id2 = init_run(&paths, &wf2.workflow_id, "p2", &wf2).expect("init_run2");

        let all_runs = list_runs(&paths, None).expect("list all");
        assert_eq!(all_runs.len(), 2);

        let filtered = list_runs(&paths, Some("test_workflow")).expect("list filter");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].run_id, run_id1);
    }

    #[test]
    fn test_cancel_run_without_active_task() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let workflow = dummy_workflow();

        let run_id = init_run(&paths, &workflow.workflow_id, "default", &workflow).expect("init_run");

        cancel_run(&paths, &run_id, "user request").expect("cancel");

        let state = load_run(&paths, &run_id).expect("load_run");
        assert_eq!(state.phase_status, PhaseStatus::Cancelled);
        assert_eq!(state.stop_reason, Some("user request".to_string()));
    }

    #[test]
    fn test_cancel_run_with_active_task_rejected() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let workflow = dummy_workflow();

        let run_id = init_run(&paths, &workflow.workflow_id, "default", &workflow).expect("init_run");

        // Manually inject an active task
        let mut state = load_run(&paths, &run_id).expect("load_run");
        state.active_task_id = Some("task_123".to_string());
        save_run_state(&paths, &run_id, &state).expect("save");

        let result = cancel_run(&paths, &run_id, "stop it");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), "CANNOT_RELEASE_TASK");

        let state = load_run(&paths, &run_id).expect("load_run");
        // State must NOT have changed (cancel was rejected)
        assert_eq!(state.phase_status, PhaseStatus::Waiting);
    }
}
