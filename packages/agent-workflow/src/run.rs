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
// Helpers
// ---------------------------------------------------------------------------

/// RAII guard that deletes a temporary file on drop unless explicitly released.
///
/// Call `release()` after the atomic rename succeeds to prevent cleanup.
struct TmpFileGuard {
    path: std::path::PathBuf,
    released: bool,
}

impl TmpFileGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            released: false,
        }
    }
    fn release(&mut self) {
        self.released = true;
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        if !self.released {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Load / Save
// ---------------------------------------------------------------------------

/// Load workflow run state from `.agent/workflow_runs/<run_id>/run_state.json`.
pub fn load_run(paths: &ProjectPaths, run_id: &str) -> Result<WorkflowRunState, ControllerError> {
    let run_state_path = paths.run_state_file(run_id);
    let contents = std::fs::read_to_string(&run_state_path).map_err(|e| {
        let kind = e.kind().to_string();
        if e.kind() == std::io::ErrorKind::NotFound {
            ControllerError::UnknownWorkflowError {
                message: format!("Run '{}' not found", run_id),
            }
        } else {
            ControllerError::UnknownWorkflowError {
                message: format!(
                    "Failed to read run state '{}': {} (kind: {})",
                    run_id, e, kind
                ),
            }
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
    let mut guard = TmpFileGuard::new(temp_path.clone());
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
    guard.release();

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

    // Create directory structure. On any failure after the first directory
    // is created, attempt cleanup of the run directory to avoid partial state.
    std::fs::create_dir_all(paths.run_dir(&run_id)).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Failed to create run dir for '{}': {}", run_id, e),
        }
    })?;
    if let Err(e) = std::fs::create_dir_all(paths.task_packets_dir(&run_id)) {
        let _ = std::fs::remove_dir_all(paths.run_dir(&run_id));
        return Err(ControllerError::UnknownWorkflowError {
            message: format!(
                "Failed to create task_packets dir for '{}': {}",
                run_id, e
            ),
        });
    }
    if let Err(e) = std::fs::create_dir_all(paths.result_packets_dir(&run_id)) {
        let _ = std::fs::remove_dir_all(paths.run_dir(&run_id));
        return Err(ControllerError::UnknownWorkflowError {
            message: format!(
                "Failed to create result_packets dir for '{}': {}",
                run_id, e
            ),
        });
    }
    if let Err(e) = std::fs::create_dir_all(paths.artifacts_dir(&run_id)) {
        let _ = std::fs::remove_dir_all(paths.run_dir(&run_id));
        return Err(ControllerError::UnknownWorkflowError {
            message: format!(
                "Failed to create artifacts dir for '{}': {}",
                run_id, e
            ),
        });
    }

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

/// Cancel a run, releasing any active task lease via the adapter first.
pub fn cancel_run_with_adapter<A>(
    paths: &ProjectPaths,
    adapter: &A,
    definition: &WorkflowDefinition,
    run_id: &str,
    reason: &str,
) -> Result<(), ControllerError>
where
    A: crate::adapter_client::AdapterClient,
{
    let mut state = load_run(paths, run_id)?;

    // If active task, release it first
    if let (Some(ref task_id), Some(revision)) = (
        &state.active_task_id,
        state.active_task_graph_revision,
    ) {
        let result = adapter.release_work(
            paths,
            &definition.adapter_profile,
            task_id,
            revision,
            &format!("cancel_run: {}", reason),
        );
        match result {
            Ok(_) => {
                crate::log::log_event(
                    paths,
                    "lease_released",
                    run_id,
                    &serde_json::json!({"task_id": task_id, "reason": reason}),
                )?;
            }
            Err(_) => {
                return Err(ControllerError::CannotReleaseTask {
                    run_id: run_id.to_string(),
                    task_id: task_id.clone(),
                    reason: "adapter release-work failed".to_string(),
                });
            }
        }
    }

    let stop_reason = if reason.is_empty() {
        "operator_cancelled"
    } else {
        reason
    };

    state.phase_status = PhaseStatus::Cancelled;
    state.stop_reason = Some(stop_reason.to_string());
    state.active_task_id = None;
    state.active_task_graph_revision = None;
    state.active_task_lease_expires_at = None;
    state.active_task_packet_ref = None;
    state.updated_time = Utc::now().to_rfc3339();

    save_run_state(paths, run_id, &state)?;

    crate::log::log_event(
        paths,
        "run_cancelled",
        run_id,
        &serde_json::json!({"reason": stop_reason}),
    )?;

    Ok(())
}

/// Check if a phase or workflow has exceeded its timeout.
pub fn check_timeout(
    run_state: &WorkflowRunState,
    definition: &WorkflowDefinition,
) -> Option<TimeoutAction> {
    let phase = definition.phases.iter().find(|p| {
        Some(p.phase_id.as_str()) == run_state.current_phase.as_deref()
    })?;

    let phase_timeout = phase
        .max_phase_duration_minutes
        .unwrap_or(definition.timeout_policy.default_phase_timeout_minutes);

    let phase_started = run_state
        .phase_history
        .last()
        .and_then(|h| h.entered_at.parse::<chrono::DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);

    let elapsed_minutes = Utc::now().signed_duration_since(phase_started).num_minutes().max(0) as u64;

    if elapsed_minutes >= phase_timeout {
        Some(TimeoutAction {
            action: definition.timeout_policy.on_timeout.clone(),
            elapsed_minutes,
            limit_minutes: phase_timeout,
        })
    } else {
        None
    }
}

/// Action to take when a timeout is detected.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeoutAction {
    pub action: String,
    pub elapsed_minutes: u64,
    pub limit_minutes: u64,
}

/// Check if workflow retry threshold exceeded.
pub fn check_retry(
    run_state: &WorkflowRunState,
    definition: &WorkflowDefinition,
) -> Option<u64> {
    if run_state.workflow_retry_counters.total_attempts >= definition.retry_policy.workflow_max_retries {
        Some(run_state.workflow_retry_counters.total_attempts)
    } else {
        None
    }
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

    fn run_with_active_task() -> WorkflowRunState {
        let mut state = WorkflowRunState {
            workflow_run_id: "run_001".to_string(),
            workflow_id: "test_workflow".to_string(),
            workflow_version: "1.0.0".to_string(),
            adapter_profile: "default".to_string(),
            current_phase: Some("p1".to_string()),
            phase_status: crate::run_state::PhaseStatus::InProgress,
            active_task_id: Some("task_001".to_string()),
            active_task_graph_revision: Some(1),
            active_task_lease_expires_at: None,
            active_task_packet_ref: None,
            start_time: Utc::now().to_rfc3339(),
            updated_time: Utc::now().to_rfc3339(),
            pause_reason: None,
            stop_reason: None,
            workflow_retry_counters: crate::run_state::WorkflowRetryCounters::default(),
            approval_records: vec![],
            phase_history: vec![crate::run_state::PhaseHistoryItem {
                phase_id: "p1".to_string(),
                status: crate::run_state::PhaseStatus::InProgress,
                entered_at: Utc::now().to_rfc3339(),
                exited_at: None,
                exit_reason: None,
                result_packet_id: None,
            }],
            run_artifacts: vec![],
        };
        state
    }

    fn load_state_at_phase_start() -> WorkflowRunState {
        let mut state = run_with_active_task();
        state.phase_history = vec![crate::run_state::PhaseHistoryItem {
            phase_id: "p1".to_string(),
            status: crate::run_state::PhaseStatus::InProgress,
            entered_at: (Utc::now() - chrono::Duration::hours(2)).to_rfc3339(),
            exited_at: None,
            exit_reason: None,
            result_packet_id: None,
        }];
        state
    }

    fn load_state_with_retries(n: u64) -> WorkflowRunState {
        let mut state = run_with_active_task();
        state.workflow_retry_counters.total_attempts = n;
        state
    }

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

    #[test]
    fn test_save_run_state_no_orphan_temp() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let workflow = dummy_workflow();

        let run_id = init_run(&paths, &workflow.workflow_id, "default", &workflow).expect("init_run");

        let tmp_path = paths.run_state_file(&run_id).with_extension("tmp");
        assert!(
            !tmp_path.exists(),
            "tmp file should not exist after successful save_run_state"
        );
    }

    #[test]
    fn test_load_run_error_includes_kind() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        // Make the run state file a directory so read_to_string fails with non-NotFound error
        let run_id = "test_run";
        let run_state_path = paths.run_state_file(run_id);
        std::fs::create_dir_all(&run_state_path).expect("create dir as run state file");

        let result = load_run(&paths, run_id);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("kind:"),
            "Error message should include error kind: {}",
            msg
        );
    }

    #[test]
    fn test_check_timeout_exceeded() {
        let mut state = load_state_at_phase_start();
        // Already has phase_history with entered_at

        let def = dummy_workflow();

        let action = check_timeout(&state, &def);
        assert!(action.is_some());
        assert_eq!(action.unwrap().action, "fail");
    }

    #[test]
    fn test_check_retry_exceeded() {
        let mut state = load_state_with_retries(5);
        let def = dummy_workflow();
        assert!(check_retry(&state, &def).is_some());
    }

    #[test]
    fn test_check_retry_below_threshold() {
        let mut state = load_state_with_retries(1);
        let def = dummy_workflow();
        assert!(check_retry(&state, &def).is_none());
    }
}
