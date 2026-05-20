use std::fmt;

/// Controller-specific error codes.
///
/// Each variant maps to a stable string code used in JSON error envelopes.
/// See the Module 3 Technical Specification §10.2 for full descriptions.
#[derive(Debug, Clone, PartialEq)]
pub enum ControllerError {
    // ── Workflow definition errors ──────────────────────────────────
    /// Specified workflow_id has no matching .yml/.json file.
    WorkflowDefinitionNotFound { workflow_id: String },

    /// Schema or data validation failure in definition.
    InvalidWorkflowDefinition { message: String },

    /// Workflow definition contains a `future_hook` or unknown criterion type.
    UnsupportedCriterion { phase_id: String, criterion_type: String },

    // ── Run state errors ──────────────────────────────────────────
    /// Cannot step a run that is COMPLETED, FAILED, or CANCELLED.
    WorkflowAlreadyStopped { run_id: String, phase_status: String },

    /// Current phase is paused awaiting operator approval.
    WorkflowPaused {
        run_id: String,
        phase_id: String,
        pause_reason: String,
    },

    /// Entry criteria for current phase are not satisfied.
    PhaseEntryCriteriaNotMet {
        run_id: String,
        phase_id: String,
        unmet_criterion: String,
    },

    /// Criteria expression syntax error or unknown key path.
    PhaseEntryCriteriaInvalid {
        run_id: String,
        phase_id: String,
        criterion: String,
        reason: String,
    },

    /// Phase requires verification but Module 5 is not installed.
    VerifierUnavailable { phase_id: String },

    // ── Timeout / retry errors ────────────────────────────────────
    /// Phase or total workflow duration exceeded limit.
    TimeoutExpired {
        run_id: String,
        phase_id: String,
        elapsed_minutes: u64,
        limit_minutes: u64,
    },

    /// Workflow-level retry threshold exceeded.
    MaxRetryExceeded {
        run_id: String,
        attempts: u64,
        max_attempts: u64,
    },

    // ── Result / submission errors ────────────────────────────────
    /// Workflow checks prevented adapter submit-result.
    ResultSubmissionBlocked {
        run_id: String,
        phase_id: String,
        reason: String,
    },

    /// Adapter submit-result returned an error (passed through).
    AdapterSubmitFailed {
        run_id: String,
        adapter_error: String,
    },

    // ── Lease / cancel errors ─────────────────────────────────────
    /// release-work call during pause or cancel failed.
    CannotReleaseTask {
        run_id: String,
        task_id: String,
        reason: String,
    },

    // ── Catch-all ─────────────────────────────────────────────────
    /// Uncategorized internal error.
    UnknownWorkflowError { message: String },
}

impl ControllerError {
    /// Returns the stable error code string for this error.
    ///
    /// These codes MUST match the technical specification exactly.
    pub fn code(&self) -> &'static str {
        match self {
            ControllerError::WorkflowDefinitionNotFound { .. } => "WORKFLOW_DEFINITION_NOT_FOUND",
            ControllerError::InvalidWorkflowDefinition { .. } => "INVALID_WORKFLOW_DEFINITION",
            ControllerError::UnsupportedCriterion { .. } => "UNSUPPORTED_CRITERION",
            ControllerError::WorkflowAlreadyStopped { .. } => "WORKFLOW_ALREADY_STOPPED",
            ControllerError::WorkflowPaused { .. } => "WORKFLOW_PAUSED",
            ControllerError::PhaseEntryCriteriaNotMet { .. } => "PHASE_ENTRY_CRITERIA_NOT_MET",
            ControllerError::PhaseEntryCriteriaInvalid { .. } => "PHASE_ENTRY_CRITERIA_INVALID",
            ControllerError::VerifierUnavailable { .. } => "VERIFIER_UNAVAILABLE",
            ControllerError::TimeoutExpired { .. } => "TIMEOUT_EXPIRED",
            ControllerError::MaxRetryExceeded { .. } => "MAX_RETRY_EXCEEDED",
            ControllerError::ResultSubmissionBlocked { .. } => "RESULT_SUBMISSION_BLOCKED",
            ControllerError::AdapterSubmitFailed { .. } => "ADAPTER_SUBMIT_FAILED",
            ControllerError::CannotReleaseTask { .. } => "CANNOT_RELEASE_TASK",
            ControllerError::UnknownWorkflowError { .. } => "UNKNOWN_WORKFLOW_ERROR",
        }
    }

    /// Returns true if the error is retryable.
    pub fn retryable(&self) -> bool {
        matches!(self, ControllerError::UnknownWorkflowError { .. })
    }

    /// Returns a human-readable message for this error.
    pub fn message(&self) -> String {
        match self {
            ControllerError::WorkflowDefinitionNotFound { workflow_id } => {
                format!("Workflow definition not found for ID: {}", workflow_id)
            }
            ControllerError::InvalidWorkflowDefinition { message } => {
                format!("Invalid workflow definition: {}", message)
            }
            ControllerError::UnsupportedCriterion { phase_id, criterion_type } => {
                format!(
                    "Unsupported criterion '{}' in phase '{}'",
                    criterion_type, phase_id
                )
            }
            ControllerError::WorkflowAlreadyStopped { run_id, phase_status } => {
                format!(
                    "Workflow run '{}' is already stopped (status: {})",
                    run_id, phase_status
                )
            }
            ControllerError::WorkflowPaused {
                run_id,
                phase_id,
                pause_reason,
            } => {
                format!(
                    "Workflow '{}' paused in phase '{}'. Reason: {}",
                    run_id, phase_id, pause_reason
                )
            }
            ControllerError::PhaseEntryCriteriaNotMet {
                run_id,
                phase_id,
                unmet_criterion,
            } => {
                format!(
                    "Phase entry criteria not met for run '{}', phase '{}'. Unmet: {}",
                    run_id, phase_id, unmet_criterion
                )
            }
            ControllerError::PhaseEntryCriteriaInvalid {
                run_id,
                phase_id,
                criterion,
                reason,
            } => {
                format!(
                    "Invalid phase entry criterion for run '{}', phase '{}'. Criterion: {}. Reason: {}",
                    run_id, phase_id, criterion, reason
                )
            }
            ControllerError::VerifierUnavailable { phase_id } => {
                format!(
                    "Phase '{}' requires verification, but verifier (Module 5) is not available",
                    phase_id
                )
            }
            ControllerError::TimeoutExpired {
                run_id,
                phase_id,
                elapsed_minutes,
                limit_minutes,
            } => {
                format!(
                    "Timeout expired for run '{}', phase '{}'. Elapsed: {} min, limit: {} min",
                    run_id, phase_id, elapsed_minutes, limit_minutes
                )
            }
            ControllerError::MaxRetryExceeded {
                run_id,
                attempts,
                max_attempts,
            } => {
                format!(
                    "Max retries exceeded for run '{}'. Attempts: {}, limit: {}",
                    run_id, attempts, max_attempts
                )
            }
            ControllerError::ResultSubmissionBlocked {
                run_id,
                phase_id,
                reason,
            } => {
                format!(
                    "Result submission blocked for run '{}', phase '{}'. {}",
                    run_id, phase_id, reason
                )
            }
            ControllerError::AdapterSubmitFailed {
                run_id,
                adapter_error,
            } => {
                format!(
                    "Adapter submit-result failed for run '{}'. Error: {}",
                    run_id, adapter_error
                )
            }
            ControllerError::CannotReleaseTask {
                run_id,
                task_id,
                reason,
            } => {
                format!(
                    "Cannot release task '{}' for run '{}'. {}",
                    task_id, run_id, reason
                )
            }
            ControllerError::UnknownWorkflowError { message } => {
                format!("Unknown workflow error: {}", message)
            }
        }
    }

    /// Returns the "source" field for the error envelope.
    pub fn source(&self) -> &'static str {
        "workflow_controller"
    }

    /// Returns the recommended agent action for this error.
    pub fn agent_action(&self) -> &'static str {
        match self {
            ControllerError::WorkflowDefinitionNotFound { .. } => "CREATE_WORKFLOW_DEFINITION",
            ControllerError::InvalidWorkflowDefinition { .. } => "FIX_DEFINITION",
            ControllerError::UnsupportedCriterion { .. } => "DOWNGRADE_DEFINITION",
            ControllerError::WorkflowAlreadyStopped { .. } => "START_NEW_RUN",
            ControllerError::WorkflowPaused { .. } => "AWAIT_OPERATOR_APPROVAL",
            ControllerError::PhaseEntryCriteriaNotMet { .. } => "WAIT_OR_INVESTIGATE",
            ControllerError::PhaseEntryCriteriaInvalid { .. } => "FIX_DEFINITION",
            ControllerError::VerifierUnavailable { .. } => "INSTALL_VERIFIER",
            ControllerError::TimeoutExpired { .. } => "REVIEW_AND_RESTART",
            ControllerError::MaxRetryExceeded { .. } => "REVIEW_AND_RESTART",
            ControllerError::ResultSubmissionBlocked { .. } => "FIX_RESULT_PACKET",
            ControllerError::AdapterSubmitFailed { .. } => "RETRY",
            ControllerError::CannotReleaseTask { .. } => "INVESTIGATE_ADAPTER",
            ControllerError::UnknownWorkflowError { .. } => "RETRY",
        }
    }

    /// Returns the recommended human action for this error.
    pub fn human_action(&self) -> String {
        match self {
            ControllerError::WorkflowDefinitionNotFound { .. } => {
                "Add workflow file to .agent/workflows/".to_string()
            }
            ControllerError::InvalidWorkflowDefinition { .. } => {
                "Review and correct YAML/JSON".to_string()
            }
            ControllerError::UnsupportedCriterion { .. } => {
                "Remove unsupported criteria".to_string()
            }
            ControllerError::WorkflowAlreadyStopped { .. } => {
                "Run 'agent-workflow init-run' to start a new run".to_string()
            }
            ControllerError::WorkflowPaused { run_id, .. } => {
                format!(
                    "Run: agent-workflow step --run-id {} --approve [APPROVED|REJECTED|DEFERRED] --reason \"...\"",
                    run_id
                )
            }
            ControllerError::PhaseEntryCriteriaNotMet { .. } => {
                "Check graph state manually".to_string()
            }
            ControllerError::PhaseEntryCriteriaInvalid { .. } => {
                "Correct criteria expression".to_string()
            }
            ControllerError::VerifierUnavailable { .. } => {
                "Remove verification_required or install Module 5".to_string()
            }
            ControllerError::TimeoutExpired { .. } => {
                "Extend timeout or investigate cause".to_string()
            }
            ControllerError::MaxRetryExceeded { .. } => {
                "Assess failures before retrying".to_string()
            }
            ControllerError::ResultSubmissionBlocked { .. } => {
                "Fix result file or criteria".to_string()
            }
            ControllerError::AdapterSubmitFailed { .. } => {
                "Review adapter error and retry".to_string()
            }
            ControllerError::CannotReleaseTask { .. } => {
                "Check adapter_logs.jsonl".to_string()
            }
            ControllerError::UnknownWorkflowError { .. } => {
                "Report issue and retry".to_string()
            }
        }
    }
}

impl fmt::Display for ControllerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ControllerError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let errors = vec![
            ControllerError::WorkflowDefinitionNotFound {
                workflow_id: "test".to_string(),
            },
            ControllerError::InvalidWorkflowDefinition {
                message: "bad".to_string(),
            },
            ControllerError::UnsupportedCriterion {
                phase_id: "p".to_string(),
                criterion_type: "future_hook".to_string(),
            },
            ControllerError::WorkflowAlreadyStopped {
                run_id: "r".to_string(),
                phase_status: "COMPLETED".to_string(),
            },
            ControllerError::WorkflowPaused {
                run_id: "r".to_string(),
                phase_id: "p".to_string(),
                pause_reason: "waiting".to_string(),
            },
            ControllerError::PhaseEntryCriteriaNotMet {
                run_id: "r".to_string(),
                phase_id: "p".to_string(),
                unmet_criterion: "graph_state".to_string(),
            },
            ControllerError::PhaseEntryCriteriaInvalid {
                run_id: "r".to_string(),
                phase_id: "p".to_string(),
                criterion: "bad_key".to_string(),
                reason: "unknown".to_string(),
            },
            ControllerError::VerifierUnavailable {
                phase_id: "p".to_string(),
            },
            ControllerError::TimeoutExpired {
                run_id: "r".to_string(),
                phase_id: "p".to_string(),
                elapsed_minutes: 60,
                limit_minutes: 30,
            },
            ControllerError::MaxRetryExceeded {
                run_id: "r".to_string(),
                attempts: 5,
                max_attempts: 3,
            },
            ControllerError::ResultSubmissionBlocked {
                run_id: "r".to_string(),
                phase_id: "p".to_string(),
                reason: "bad status".to_string(),
            },
            ControllerError::AdapterSubmitFailed {
                run_id: "r".to_string(),
                adapter_error: "timeout".to_string(),
            },
            ControllerError::CannotReleaseTask {
                run_id: "r".to_string(),
                task_id: "t".to_string(),
                reason: "lease expired".to_string(),
            },
            ControllerError::UnknownWorkflowError {
                message: "oops".to_string(),
            },
        ];

        // Verify each error maps to its expected code
        assert_eq!(errors[0].code(), "WORKFLOW_DEFINITION_NOT_FOUND");
        assert_eq!(errors[1].code(), "INVALID_WORKFLOW_DEFINITION");
        assert_eq!(errors[2].code(), "UNSUPPORTED_CRITERION");
        assert_eq!(errors[3].code(), "WORKFLOW_ALREADY_STOPPED");
        assert_eq!(errors[4].code(), "WORKFLOW_PAUSED");
        assert_eq!(errors[5].code(), "PHASE_ENTRY_CRITERIA_NOT_MET");
        assert_eq!(errors[6].code(), "PHASE_ENTRY_CRITERIA_INVALID");
        assert_eq!(errors[7].code(), "VERIFIER_UNAVAILABLE");
        assert_eq!(errors[8].code(), "TIMEOUT_EXPIRED");
        assert_eq!(errors[9].code(), "MAX_RETRY_EXCEEDED");
        assert_eq!(errors[10].code(), "RESULT_SUBMISSION_BLOCKED");
        assert_eq!(errors[11].code(), "ADAPTER_SUBMIT_FAILED");
        assert_eq!(errors[12].code(), "CANNOT_RELEASE_TASK");
        assert_eq!(errors[13].code(), "UNKNOWN_WORKFLOW_ERROR");
    }

    #[test]
    fn test_retryable() {
        let retryable = ControllerError::UnknownWorkflowError {
            message: "test".to_string(),
        };
        let not_retryable = ControllerError::WorkflowAlreadyStopped {
            run_id: "r".to_string(),
            phase_status: "COMPLETED".to_string(),
        };

        assert!(retryable.retryable());
        assert!(!not_retryable.retryable());
    }

    #[test]
    fn test_error_display() {
        let err = ControllerError::WorkflowDefinitionNotFound {
            workflow_id: "my_workflow".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Workflow definition not found for ID: my_workflow"
        );
    }
}
