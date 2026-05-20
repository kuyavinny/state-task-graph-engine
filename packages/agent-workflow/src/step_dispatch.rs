//! Step dispatch — acquire work for the current phase.
//!
//! PR 6 scope only:
//! - No result-file path.
//! - No approval gates.
//! - No verification.

use crate::adapter_client::AdapterClient;
use crate::criteria::{evaluate_criteria, EvaluationContext, EvaluationResult};
use crate::error::ControllerError;
use crate::graph_client::GraphStatusClient;
use crate::model::WorkflowDefinition;
use crate::paths::ProjectPaths;
use crate::run_state::{PhaseStatus, WorkflowRunState};
use chrono::Utc;

/// Outcome of a dispatch attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchOutcome {
    AwaitingWorker { task_id: String },
    AwaitingResult { task_id: String },
}

/// Execute step dispatch.
pub fn execute_step_dispatch<A, G>(
    paths: &ProjectPaths,
    adapter: &A,
    graph: &G,
    definition: &WorkflowDefinition,
    run_state: &mut WorkflowRunState,
) -> Result<DispatchOutcome, ControllerError>
where
    A: AdapterClient,
    G: GraphStatusClient,
{
    // 1. Check run status
    match run_state.phase_status {
        PhaseStatus::Completed | PhaseStatus::Failed | PhaseStatus::Cancelled => {
            return Err(ControllerError::WorkflowAlreadyStopped {
                run_id: run_state.workflow_run_id.clone(),
                phase_status: format!("{:?}", run_state.phase_status),
            });
        }
        PhaseStatus::Paused => {
            return Err(ControllerError::WorkflowPaused {
                run_id: run_state.workflow_run_id.clone(),
                phase_id: run_state.current_phase.clone().unwrap_or_default(),
                pause_reason: run_state.pause_reason.clone().unwrap_or_default(),
            });
        }
        _ => {}
    }

    let phase_id = run_state
        .current_phase
        .as_ref()
        .ok_or_else(|| ControllerError::UnknownWorkflowError {
            message: "Run has no current phase".to_string(),
        })?
        .clone();

    // 2. Find current phase definition
    let phase = definition
        .phases
        .iter()
        .find(|p| p.phase_id == phase_id)
        .ok_or_else(|| ControllerError::UnknownWorkflowError {
            message: format!("Phase '{}' not found in definition", phase_id),
        })?;

    // 3. Evaluate entry criteria
    let graph_ctx = graph.status(paths)?;

    let eval_ctx = EvaluationContext {
        graph: graph_ctx,
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
        result_packet: None,
    };

    match evaluate_criteria(&phase.entry_criteria, &eval_ctx) {
        EvaluationResult::AllMet => {}
        EvaluationResult::NotMet { reason, .. } => {
            return Err(ControllerError::PhaseEntryCriteriaNotMet {
                run_id: run_state.workflow_run_id.clone(),
                phase_id,
                unmet_criterion: reason,
            });
        }
        EvaluationResult::Invalid { reason, .. } => {
            return Err(ControllerError::PhaseEntryCriteriaInvalid {
                run_id: run_state.workflow_run_id.clone(),
                phase_id,
                criterion: "entry".to_string(),
                reason,
            });
        }
    }

    // 3.5 Check verification
    crate::verification::check_verification(phase, run_state)?;

    // 3.6 Check approval gate
    if phase.operator_approval_required {
        let approved = run_state.approval_records.iter().any(|r| {
            r.phase_id == phase_id && r.decision == crate::run_state::ApprovalDecision::Approved
        });
        if !approved {
            run_state.phase_status = PhaseStatus::Paused;
            run_state.pause_reason = Some("Awaiting operator approval".to_string());
            crate::run::save_run_state(paths, &run_state.workflow_run_id, run_state)
                .map_err(|e| ControllerError::UnknownWorkflowError {
                    message: format!("Failed to save paused state: {}", e),
                })?;
            return Err(ControllerError::WorkflowPaused {
                run_id: run_state.workflow_run_id.clone(),
                phase_id,
                pause_reason: "Awaiting operator approval".to_string(),
            });
        }
    }

    // 4. Active task already present?
    if let Some(ref active) = run_state.active_task_id {
        return Ok(DispatchOutcome::AwaitingResult {
            task_id: active.clone(),
        });
    }

    // 5. Get work from adapter
    let task = adapter.get_work(paths, &definition.adapter_profile)?;

    // 6. Persist task packet
    let packet_name = format!("{}.json", task.task_id);
    let packet_dir = paths.task_packets_dir(&run_state.workflow_run_id);
    std::fs::create_dir_all(&packet_dir).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to create task_packets dir: {}", e),
    })?;
    let packet_path = packet_dir.join(&packet_name);
    std::fs::write(
        &packet_path,
        serde_json::to_string(&task).map_err(|e| ControllerError::UnknownWorkflowError {
            message: format!("Serialize task packet: {}", e),
        })?,
    ).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to write task packet: {}", e),
    })?;

    // 7. Update run state
    run_state.active_task_id = Some(task.task_id.clone());
    run_state.active_task_graph_revision = Some(task.graph_revision);
    run_state.active_task_lease_expires_at = task.lease_expires_at.clone();
    run_state.active_task_packet_ref = Some(packet_name);

    crate::run::save_run_state(paths, &run_state.workflow_run_id, run_state)?;

    // 8. Log
    crate::log::log_event(
        paths,
        "work_acquired",
        &run_state.workflow_run_id,
        &serde_json::json!({"task_id": &task.task_id, "phase_id": &phase_id}),
    )?;

    Ok(DispatchOutcome::AwaitingWorker {
        task_id: task.task_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter_client::mock::MockAdapterClient;
    use crate::criteria_context::CriteriaContext;
    use crate::graph_client::mock::MockGraphStatusClient;
    use crate::model::{Criterion, GraphStateCriterion, Phase, WorkflowDefinition};
    use crate::paths::ProjectPaths;
    use crate::run_state::{
        ApprovalRecord, PhaseHistoryItem, PhaseStatus, WorkflowRunState,
    };
    use chrono::Utc;

    fn minimal_definition() -> WorkflowDefinition {
        WorkflowDefinition {
            workflow_id: "wf_test".to_string(),
            name: "Test".to_string(),
            description: "".to_string(),
            version: "1".to_string(),
            adapter_profile: "default".to_string(),
            phases: vec![Phase {
                phase_id: "phase_1".to_string(),
                name: "Phase 1".to_string(),
                description: "".to_string(),
                entry_criteria: vec![Criterion::GraphState(GraphStateCriterion {
                    key: "status_counts.READY".to_string(),
                    op: ">=".to_string(),
                    value: 1,
                })],
                exit_criteria: vec![],
                operator_approval_required: false,
                verification_required: false,
                allowed_task_types: vec![],
                max_phase_duration_minutes: None,
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
            stop_conditions: vec![],
        }
    }

    fn minimal_run_state() -> WorkflowRunState {
        WorkflowRunState {
            workflow_run_id: "run_001".to_string(),
            workflow_id: "wf_test".to_string(),
            workflow_version: "1".to_string(),
            adapter_profile: "default".to_string(),
            current_phase: Some("phase_1".to_string()),
            phase_status: PhaseStatus::InProgress,
            active_task_id: None,
            active_task_graph_revision: None,
            active_task_lease_expires_at: None,
            active_task_packet_ref: None,
            start_time: Utc::now().to_rfc3339(),
            updated_time: Utc::now().to_rfc3339(),
            pause_reason: None,
            stop_reason: None,
            workflow_retry_counters: crate::run_state::WorkflowRetryCounters::default(),
            approval_records: vec![],
            phase_history: vec![PhaseHistoryItem {
                phase_id: "phase_1".to_string(),
                status: PhaseStatus::Waiting,
                entered_at: Utc::now().to_rfc3339(),
                exited_at: None,
                exit_reason: None,
                result_packet_id: None,
            }],
            run_artifacts: vec![],
        }
    }

    #[test]
    fn test_dispatch_happy_path() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let def = minimal_definition();
        let mut rs = minimal_run_state();
        let adapter = MockAdapterClient::new();
        let mut graph = MockGraphStatusClient::new();
        graph.status_result = Ok(CriteriaContext {
            graph_revision: 1,
            node_count: 5,
            status_counts: {
                let mut m = std::collections::HashMap::new();
                m.insert("READY".to_string(), 2);
                m
            },
            warnings: vec![],
        });

        let result = execute_step_dispatch(&paths, &adapter, &graph, &def, &mut rs,
        );
        assert!(result.is_ok());
        assert!(rs.active_task_id.is_some());
    }

    #[test]
    fn test_dispatch_stopped_returns_error() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let def = minimal_definition();
        let mut rs = minimal_run_state();
        rs.phase_status = PhaseStatus::Completed;

        let adapter = MockAdapterClient::new();
        let graph = MockGraphStatusClient::new();

        let result = execute_step_dispatch(&paths, &adapter, &graph, &def, &mut rs,
        );
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ControllerError::WorkflowAlreadyStopped { .. })
        );
    }

    #[test]
    fn test_dispatch_paused_returns_error() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let def = minimal_definition();
        let mut rs = minimal_run_state();
        rs.phase_status = PhaseStatus::Paused;
        rs.pause_reason = Some("awaiting_approval".to_string());

        let adapter = MockAdapterClient::new();
        let graph = MockGraphStatusClient::new();

        let result = execute_step_dispatch(&paths, &adapter, &graph, &def, &mut rs,
        );
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ControllerError::WorkflowPaused { .. })
        );
    }

    #[test]
    fn test_dispatch_active_task_returns_awaiting() {
        let tmp = tempfile::tempdir().expect("temp");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let def = minimal_definition();
        let mut rs = minimal_run_state();
        rs.active_task_id = Some("task_001".to_string());

        let adapter = MockAdapterClient::new();
        let mut graph = MockGraphStatusClient::new();
        graph.status_result = Ok(CriteriaContext {
            graph_revision: 1,
            node_count: 5,
            status_counts: {
                let mut m = std::collections::HashMap::new();
                m.insert("READY".to_string(), 2);
                m
            },
            warnings: vec![],
        });

        let result = execute_step_dispatch(
            &paths, &adapter, &graph, &def, &mut rs,
        );
        assert_eq!(
            result.unwrap(),
            DispatchOutcome::AwaitingResult {
                task_id: "task_001".to_string()
            }
        );
    }
}
