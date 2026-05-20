//! Verifier placeholder — Module 5 not yet implemented.
//!
//! verification_required: true is valid in definitions.
//! Entering/completing a verification-required phase returns VERIFIER_UNAVAILABLE.
//! Does NOT call any nonexistent verifier binary.
//! Does NOT submit success through adapter.

use crate::error::ControllerError;
use crate::model::Phase;
use crate::run_state::WorkflowRunState;

/// Check if the current phase requires verification.
/// Returns `VERIFIER_UNAVAILABLE` if `verification_required` is true.
pub fn check_verification(phase: &Phase, _run_state: &WorkflowRunState) -> Result<(), ControllerError> {
    if phase.verification_required {
        return Err(ControllerError::VerifierUnavailable {
            phase_id: phase.phase_id.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Phase;
    use crate::run_state::{
        ApprovalRecord, PhaseHistoryItem, PhaseStatus, WorkflowRunState,
        WorkflowRetryCounters,
    };
    use chrono::Utc;

    fn phase_with_verification() -> Phase {
        Phase {
            phase_id: "v1".to_string(),
            name: "V".to_string(),
            description: "".to_string(),
            entry_criteria: vec![],
            exit_criteria: vec![],
            operator_approval_required: false,
            verification_required: true,
            allowed_task_types: vec![],
            max_phase_duration_minutes: None,
        }
    }

    fn minimal_run() -> WorkflowRunState {
        WorkflowRunState {
            workflow_run_id: "r".to_string(),
            workflow_id: "w".to_string(),
            workflow_version: "1".to_string(),
            adapter_profile: "default".to_string(),
            current_phase: Some("v1".to_string()),
            phase_status: PhaseStatus::InProgress,
            active_task_id: None,
            active_task_graph_revision: None,
            active_task_lease_expires_at: None,
            active_task_packet_ref: None,
            start_time: Utc::now().to_rfc3339(),
            updated_time: Utc::now().to_rfc3339(),
            pause_reason: None,
            stop_reason: None,
            workflow_retry_counters: WorkflowRetryCounters::default(),
            approval_records: vec![],
            phase_history: vec![],
            run_artifacts: vec![],
        }
    }

    #[test]
    fn test_verification_required_returns_error() {
        let phase = phase_with_verification();
        let run = minimal_run();
        assert!(
            matches!(
                check_verification(&phase, &run),
                Err(ControllerError::VerifierUnavailable { .. })
            )
        );
    }

    #[test]
    fn test_no_verification_passes() {
        let mut phase = phase_with_verification();
        phase.verification_required = false;
        let run = minimal_run();
        assert!(check_verification(&phase, &run).is_ok());
    }
}
