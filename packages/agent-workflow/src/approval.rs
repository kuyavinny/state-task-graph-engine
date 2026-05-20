//! Operator approval gate handling.
//!
//! Approval semantics:
//! - APPROVED → resume/advance
//! - REJECTED → fail workflow (stop_reason: operator_rejected)
//! - DEFERRED → remain paused (update pause_reason)
//! - Reason required for APPROVED and REJECTED

use crate::error::ControllerError;
use crate::run_state::{
    ApprovalDecision, ApprovalRecord, PhaseStatus, WorkflowRunState,
};
use chrono::Utc;

/// Operator's approval decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Approval {
    Approved { reason: String },
    Rejected { reason: String },
    Deferred { reason: String },
}

/// Check if an approval decision resolves a paused workflow.
/// Returns the resolved phase status after applying the decision.
pub fn resolve_approval(
    approval: Option<&Approval>,
    run_state: &mut WorkflowRunState,
    phase_id: &str,
) -> Result<ApprovalResolution, ControllerError> {
    // If not paused, nothing to resolve
    if run_state.phase_status != PhaseStatus::Paused {
        return Ok(ApprovalResolution::NotPaused);
    }

    // Determine if approval is required for this phase
    // (This should be checked by caller before calling resolve_approval)

    // Handle the decision
    match approval {
        Some(Approval::Approved { reason }) => {
            persist_approval(run_state, phase_id, ApprovalDecision::Approved, reason)?;
            run_state.phase_status = PhaseStatus::InProgress;
            run_state.pause_reason = None;
            Ok(ApprovalResolution::Approved)
        }
        Some(Approval::Rejected { reason }) => {
            persist_approval(run_state, phase_id, ApprovalDecision::Rejected, reason)?;
            run_state.phase_status = PhaseStatus::Failed;
            run_state.stop_reason = Some("operator_rejected".to_string());
            Ok(ApprovalResolution::Rejected)
        }
        Some(Approval::Deferred { reason }) => {
            persist_approval(run_state, phase_id, ApprovalDecision::Deferred, reason)?;
            run_state.pause_reason = Some(reason.clone());
            Ok(ApprovalResolution::Deferred)
        }
        None => {
            // If paused but no approval provided, return WorkflowPaused error
            Err(ControllerError::WorkflowPaused {
                run_id: run_state.workflow_run_id.clone(),
                phase_id: phase_id.to_string(),
                pause_reason: run_state.pause_reason.clone().unwrap_or_default(),
            })
        }
    }
}

/// Persist an approval record to the run state.
pub fn persist_approval(
    run_state: &mut WorkflowRunState,
    phase_id: &str,
    decision: ApprovalDecision,
    reason: &str,
) -> Result<(), ControllerError> {
    let record = ApprovalRecord {
        approval_id: format!("{}_{}", phase_id, Utc::now().timestamp()),
        phase_id: phase_id.to_string(),
        operator: "operator".to_string(), // Placeholder — actual operator from CLI args
        decision,
        reason: reason.to_string(),
        timestamp: Utc::now().to_rfc3339(),
    };
    run_state.approval_records.push(record);
    Ok(())
}

/// Result of an approval resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalResolution {
    NotPaused,
    Approved,
    Rejected,
    Deferred,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_state::{WorkflowRunState, PhaseHistoryItem, PhaseStatus};
    use chrono::Utc;

    fn paused_run() -> WorkflowRunState {
        WorkflowRunState {
            workflow_run_id: "run_001".to_string(),
            workflow_id: "wf_test".to_string(),
            workflow_version: "1".to_string(),
            adapter_profile: "default".to_string(),
            current_phase: Some("phase_1".to_string()),
            phase_status: PhaseStatus::Paused,
            active_task_id: None,
            active_task_graph_revision: None,
            active_task_lease_expires_at: None,
            active_task_packet_ref: None,
            start_time: Utc::now().to_rfc3339(),
            updated_time: Utc::now().to_rfc3339(),
            pause_reason: Some("awaiting_approval".to_string()),
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
    fn test_approved_resumes() {
        let mut rs = paused_run();
        let result = resolve_approval(
            Some(&Approval::Approved {
                reason: "Looks good".to_string(),
            }),
            &mut rs,
            "phase_1",
        );
        assert_eq!(result.unwrap(), ApprovalResolution::Approved);
        assert_eq!(rs.phase_status, PhaseStatus::InProgress);
        assert!(rs.pause_reason.is_none());
        assert_eq!(rs.approval_records.len(), 1);
    }

    #[test]
    fn test_rejected_fails() {
        let mut rs = paused_run();
        let result = resolve_approval(
            Some(&Approval::Rejected {
                reason: "Broken".to_string(),
            }),
            &mut rs,
            "phase_1",
        );
        assert_eq!(result.unwrap(), ApprovalResolution::Rejected);
        assert_eq!(rs.phase_status, PhaseStatus::Failed);
        assert_eq!(rs.stop_reason, Some("operator_rejected".to_string()));
    }

    #[test]
    fn test_deferred_remains_paused() {
        let mut rs = paused_run();
        let result = resolve_approval(
            Some(&Approval::Deferred {
                reason: "Need more data".to_string(),
            }),
            &mut rs,
            "phase_1",
        );
        assert_eq!(result.unwrap(), ApprovalResolution::Deferred);
        assert_eq!(rs.phase_status, PhaseStatus::Paused);
        assert_eq!(rs.pause_reason, Some("Need more data".to_string()));
    }

    #[test]
    fn test_no_approval_returns_error() {
        let mut rs = paused_run();
        let result = resolve_approval(None, &mut rs, "phase_1");
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ControllerError::WorkflowPaused { .. })
        );
    }
}
