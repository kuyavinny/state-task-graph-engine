//! Step result intake — submit result and advance phase.
//!
//! Scope (PR 7 only):
//! - Load run state
//! - Require active_task_id when --result-file provided
//! - Parse result packet JSON for criteria (read-only, no modification)
//! - Evaluate exit criteria before adapter call
//! - Call adapter submit-result with original file path (unchanged)
//! - Copy result file byte-for-byte to result_packets/
//! - On success: clear active task, update phase history, advance phase
//!
//! NOT in PR 7:
//! - Approval gates (--approve)
//! - Verification placeholder
//! - Timeout / retry logic

use crate::adapter_client::AdapterClient;
use crate::criteria::{evaluate_criteria, EvaluationContext, EvaluationResult};
use crate::error::ControllerError;
use crate::model::WorkflowDefinition;
use crate::paths::ProjectPaths;
use crate::run_state::{PhaseHistoryItem, PhaseStatus, WorkflowRunState};
use chrono::Utc;

/// Outcome of result intake.
#[derive(Debug, Clone, PartialEq)]
pub enum IntakeOutcome {
    PhaseAdvanced { phase_id: String },
    WorkflowCompleted,
}

/// Execute result intake with the given result file.
pub fn execute_step_intake<A>(
    paths: &ProjectPaths,
    adapter: &A,
    result_file: &std::path::Path,
    definition: &WorkflowDefinition,
    run_state: &mut WorkflowRunState,
) -> Result<IntakeOutcome, ControllerError>
where
    A: AdapterClient,
{
    let phase_id = run_state
        .current_phase
        .as_ref()
        .ok_or_else(|| ControllerError::UnknownWorkflowError {
            message: "Run has no current phase".to_string(),
        })?
        .clone();

    let active_task_id = run_state
        .active_task_id
        .as_ref()
        .ok_or_else(|| ControllerError::UnknownWorkflowError {
            message: "No active task to submit result for".to_string(),
        })?
        .clone();

    // 1. Parse result packet JSON (read-only)
    let result_json = std::fs::read_to_string(result_file).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Cannot read result file: {}", e),
        }
    })?;
    let result_packet: serde_json::Value = serde_json::from_str(&result_json).map_err(|e| {
        ControllerError::UnknownWorkflowError {
            message: format!("Malformed result packet JSON: {}", e),
        }
    })?;

    // 2. Evaluate exit criteria BEFORE adapter call
    let phase = definition
        .phases
        .iter()
        .find(|p| p.phase_id == phase_id)
        .ok_or_else(|| ControllerError::UnknownWorkflowError {
            message: format!("Phase '{}' not in definition", phase_id),
        })?;

    let eval_ctx = EvaluationContext {
        // For exit criteria we don't need graph status (minimal)
        graph: crate::criteria_context::CriteriaContext {
            graph_revision: run_state.active_task_graph_revision.unwrap_or(0),
            node_count: 0,
            status_counts: std::collections::HashMap::new(),
            warnings: vec![],
        },
        paths: paths.clone(),
        approval_records: run_state.approval_records.clone(),
        phase_id: phase_id.clone(),
        phase_started_at: run_state
            .phase_history
            .last()
            .and_then(|h| h.entered_at.parse().ok())
            .unwrap_or_else(Utc::now),
        workflow_started_at: run_state.start_time.parse().unwrap_or_else(|_| Utc::now()),
        now: Utc::now(),
        result_packet: Some(result_packet.clone()),
    };

    match evaluate_criteria(&phase.exit_criteria, &eval_ctx) {
        EvaluationResult::AllMet => {}
        EvaluationResult::NotMet { reason, .. } => {
            return Err(ControllerError::ResultSubmissionBlocked {
                run_id: run_state.workflow_run_id.clone(),
                phase_id: phase_id.clone(),
                reason,
            });
        }
        EvaluationResult::Invalid { reason, .. } => {
            return Err(ControllerError::PhaseEntryCriteriaInvalid {
                run_id: run_state.workflow_run_id.clone(),
                phase_id,
                criterion: "exit".to_string(),
                reason,
            });
        }
    }

    // 3. Call adapter submit-result with original file path (unchanged)
    let submit = adapter.submit_result(
        paths,
        &definition.adapter_profile,
        result_file,
    )?;

    // 4. Copy result file byte-for-byte to result_packets/
    let dest_name = format!("{}_{}.json", active_task_id, Utc::now().timestamp_millis());
    let dest_dir = paths.result_packets_dir(&run_state.workflow_run_id);
    std::fs::create_dir_all(&dest_dir).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to create result_packets dir: {}", e),
    })?;
    let dest = dest_dir.join(&dest_name);
    std::fs::copy(result_file, &dest).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to copy result file: {}", e),
    })?;

    // 5. Update phase history
    if let Some(entry) = run_state.phase_history.last_mut() {
        if entry.phase_id == phase_id {
            entry.exited_at = Some(Utc::now().to_rfc3339());
            entry.exit_reason = Some(format!("completed: {}", submit.status));
            entry.result_packet_id = Some(dest_name.clone());
        }
    }

    // 6. Clear active task
    run_state.active_task_id = None;
    run_state.active_task_graph_revision = None;
    run_state.active_task_lease_expires_at = None;
    run_state.active_task_packet_ref = None;

    // 7. Advance phase or complete workflow
    let current_phase_idx = definition
        .phases
        .iter()
        .position(|p| p.phase_id == phase_id)
        .unwrap_or(0);

    if current_phase_idx + 1 < definition.phases.len() {
        let next_phase = &definition.phases[current_phase_idx + 1];
        run_state.current_phase = Some(next_phase.phase_id.clone());
        run_state.phase_status = PhaseStatus::InProgress;
        run_state.phase_history.push(PhaseHistoryItem {
            phase_id: next_phase.phase_id.clone(),
            status: PhaseStatus::Waiting,
            entered_at: Utc::now().to_rfc3339(),
            exited_at: None,
            exit_reason: None,
            result_packet_id: None,
        });

        // Save run state
        crate::run::save_run_state(
            paths,
            &run_state.workflow_run_id,
            run_state,
        )?;

        // Log
        crate::log::log_event(
            paths,
            "phase_transition",
            &run_state.workflow_run_id,
            &serde_json::json!({
                "from_phase": &phase_id,
                "to_phase": &next_phase.phase_id,
            }),
        )?;

        Ok(IntakeOutcome::PhaseAdvanced {
            phase_id: next_phase.phase_id.clone(),
        })
    } else {
        // Final phase completed
        run_state.phase_status = PhaseStatus::Completed;
        run_state.phase_history.push(PhaseHistoryItem {
            phase_id: "_final".to_string(),
            status: PhaseStatus::Completed,
            entered_at: Utc::now().to_rfc3339(),
            exited_at: Some(Utc::now().to_rfc3339()),
            exit_reason: Some("workflow_completed".to_string()),
            result_packet_id: None,
        });

        crate::run::save_run_state(
            paths,
            &run_state.workflow_run_id,
            run_state,
        )?;

        crate::log::log_event(
            paths,
            "workflow_completed",
            &run_state.workflow_run_id,
            &serde_json::json!({"final_phase": &phase_id}),
        )?;

        Ok(IntakeOutcome::WorkflowCompleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter_client::mock::MockAdapterClient;
    use crate::model::{Criterion, ResultCriterion, Phase, WorkflowDefinition};
    use crate::paths::ProjectPaths;
    use crate::run_state::{PhaseStatus, PhaseHistoryItem, WorkflowRunState};
    use chrono::Utc;

    fn two_phase_definition() -> WorkflowDefinition {
        WorkflowDefinition {
            workflow_id: "wf_test".to_string(),
            name: "Test".to_string(),
            description: "".to_string(),
            version: "1".to_string(),
            adapter_profile: "default".to_string(),
            phases: vec![
                Phase {
                    phase_id: "phase_1".to_string(),
                    name: "Phase 1".to_string(),
                    description: "".to_string(),
                    entry_criteria: vec![],
                    exit_criteria: vec![Criterion::Result(ResultCriterion {
                        status: "success".to_string(),
                        last_task_completed: None,
                    })],
                    operator_approval_required: false,
                    verification_required: false,
                    allowed_task_types: vec![],
                    max_phase_duration_minutes: None,
                },
                Phase {
                    phase_id: "phase_2".to_string(),
                    name: "Phase 2".to_string(),
                    description: "".to_string(),
                    entry_criteria: vec![],
                    exit_criteria: vec![],
                    operator_approval_required: false,
                    verification_required: false,
                    allowed_task_types: vec![],
                    max_phase_duration_minutes: None,
                },
            ],
            timeout_policy: crate::model::TimeoutPolicy {
                default_phase_timeout_minutes: 60,
                total_workflow_timeout_minutes: 120,
                on_timeout: "fail".to_string(),
            },
            retry_policy: crate::model::RetryPolicy {
                workflow_max_retries: 3,
                sequential_task_failure_threshold: 2,
            },
            stop_conditions: vec![],
        }
    }

    fn run_with_active_task() -> WorkflowRunState {
        WorkflowRunState {
            workflow_run_id: "run_001".to_string(),
            workflow_id: "wf_test".to_string(),
            workflow_version: "1".to_string(),
            adapter_profile: "default".to_string(),
            current_phase: Some("phase_1".to_string()),
            phase_status: PhaseStatus::InProgress,
            active_task_id: Some("task_001".to_string()),
            active_task_graph_revision: Some(1),
            active_task_lease_expires_at: None,
            active_task_packet_ref: Some("task_001.json".to_string()),
            start_time: Utc::now().to_rfc3339(),
            updated_time: Utc::now().to_rfc3339(),
            pause_reason: None,
            stop_reason: None,
            workflow_retry_counters: crate::run_state::WorkflowRetryCounters::default(),
            approval_records: vec![],
            phase_history: vec![PhaseHistoryItem {
                phase_id: "phase_1".to_string(),
                status: PhaseStatus::InProgress,
                entered_at: Utc::now().to_rfc3339(),
                exited_at: None,
                exit_reason: None,
                result_packet_id: None,
            }],
            run_artifacts: vec![],
        }
    }

    #[test]
    fn test_intake_advances_phase() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let def = two_phase_definition();
        let mut rs = run_with_active_task();

        // Write valid result packet
        let result_file = tmp.path().join("result.json");
        std::fs::write(
            &result_file,
            serde_json::json!({"status": "success", "task_id": "task_001"}).to_string(),
        ).unwrap();

        let adapter = MockAdapterClient::new();

        let result = execute_step_intake(
            &paths, &adapter, &result_file, &def, &mut rs,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), IntakeOutcome::PhaseAdvanced {
            phase_id: "phase_2".to_string()
        });
        assert!(rs.active_task_id.is_none());
        assert_eq!(rs.current_phase, Some("phase_2".to_string()));
    }

    #[test]
    fn test_intake_completes_workflow() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let mut def = two_phase_definition();
        def.phases.pop(); // Only one phase

        let mut rs = run_with_active_task();

        let result_file = tmp.path().join("result.json");
        std::fs::write(
            &result_file,
            serde_json::json!({"status": "success"}).to_string(),
        ).unwrap();

        let adapter = MockAdapterClient::new();

        let result = execute_step_intake(
            &paths, &adapter, &result_file, &def, &mut rs,
        );
        assert_eq!(result.unwrap(), IntakeOutcome::WorkflowCompleted);
        assert_eq!(rs.phase_status, PhaseStatus::Completed);
    }

    #[test]
    fn test_intake_blocks_on_exit_criteria() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let def = two_phase_definition();
        let mut rs = run_with_active_task();

        // Result status "failure" does not match required "success"
        let result_file = tmp.path().join("result.json");
        std::fs::write(
            &result_file,
            serde_json::json!({"status": "failure"}).to_string(),
        ).unwrap();

        let adapter = MockAdapterClient::new();

        let result = execute_step_intake(
            &paths, &adapter, &result_file, &def, &mut rs,
        );
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ControllerError::ResultSubmissionBlocked { .. })
        );
        // Active task should still be present (adapter NOT called)
        assert!(rs.active_task_id.is_some());
    }
}
