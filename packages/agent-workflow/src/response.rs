use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Success Envelope
// ---------------------------------------------------------------------------

/// Standard success response envelope.
///
/// Matches Module 2 envelope format. All `agent-workflow` commands output this
/// shape on success.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuccessEnvelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuccessEnvelope {
    pub fn new(message: &str) -> Self {
        Self {
            ok: true,
            workflow: None,
            run_id: None,
            current_phase: None,
            phase_status: None,
            message: Some(message.to_string()),
        }
    }

    pub fn with_run(run_id: &str, message: &str) -> Self {
        Self {
            ok: true,
            workflow: None,
            run_id: Some(run_id.to_string()),
            current_phase: None,
            phase_status: None,
            message: Some(message.to_string()),
        }
    }

    pub fn with_workflow(workflow: &str, run_id: &str, current_phase: &str, phase_status: &str, message: &str) -> Self {
        Self {
            ok: true,
            workflow: Some(workflow.to_string()),
            run_id: Some(run_id.to_string()),
            current_phase: Some(current_phase.to_string()),
            phase_status: Some(phase_status.to_string()),
            message: Some(message.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Failure Envelope
// ---------------------------------------------------------------------------

/// Standard failure response envelope.
///
/// Matches Module 2 envelope format with `error` object containing:
/// - `code`: stable error code string
/// - `source`: always `"workflow_controller"` for Module 3 errors
/// - `message`: human-readable error description
/// - `retryable`: boolean
/// - `agent_action`: recommended action for automated agents
/// - `human_action`: recommended action for human operators
/// - `details`: optional structured details
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureEnvelope {
    pub ok: bool,
    pub error: ErrorDetail,
}

/// Structured error detail within a failure envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorDetail {
    pub code: String,
    pub source: String,
    pub message: String,
    pub retryable: bool,
    pub agent_action: String,
    pub human_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl FailureEnvelope {
    pub fn from_controller_error(err: &crate::error::ControllerError) -> Self {
        Self {
            ok: false,
            error: ErrorDetail {
                code: err.code().to_string(),
                source: err.source().to_string(),
                message: err.message(),
                retryable: err.retryable(),
                agent_action: err.agent_action().to_string(),
                human_action: err.human_action(),
                details: None,
            },
        }
    }

    pub fn with_details(err: &crate::error::ControllerError, details: serde_json::Value) -> Self {
        Self {
            ok: false,
            error: ErrorDetail {
                code: err.code().to_string(),
                source: err.source().to_string(),
                message: err.message(),
                retryable: err.retryable(),
                agent_action: err.agent_action().to_string(),
                human_action: err.human_action(),
                details: Some(details),
            },
        }
    }

    /// Create a simple NOT_IMPLEMENTED envelope for stub commands.
    pub fn not_implemented(command: &str) -> Self {
        Self {
            ok: false,
            error: ErrorDetail {
                code: "NOT_IMPLEMENTED".to_string(),
                source: "workflow_controller".to_string(),
                message: format!("Command '{}' is not yet implemented.", command),
                retryable: false,
                agent_action: "WAIT_FOR_IMPLEMENTATION".to_string(),
                human_action: "This feature is not yet available.".to_string(),
                details: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ControllerError;

    #[test]
    fn test_success_envelope_serialization() {
        let envelope = SuccessEnvelope::with_workflow(
            "api_deployment_v1",
            "run_2026-05-20T14-30-00_abc123",
            "setup",
            "WAITING",
            "Workflow run initialized.",
        );

        let json = serde_json::to_string(&envelope).expect("serialize");
        let reparsed: SuccessEnvelope = serde_json::from_str(&json).expect("deserialize");

        assert!(reparsed.ok);
        assert_eq!(reparsed.workflow, Some("api_deployment_v1".to_string()));
        assert_eq!(reparsed.current_phase, Some("setup".to_string()));
    }

    #[test]
    fn test_failure_envelope_serialization() {
        let err = ControllerError::WorkflowPaused {
            run_id: "run_123".to_string(),
            phase_id: "verification_gate".to_string(),
            pause_reason: "awaiting_decision".to_string(),
        };

        let envelope = FailureEnvelope::from_controller_error(&err);
        let json = serde_json::to_string_pretty(&envelope).expect("serialize");
        let reparsed: FailureEnvelope = serde_json::from_str(&json).expect("deserialize");

        assert!(!reparsed.ok);
        assert_eq!(reparsed.error.code, "WORKFLOW_PAUSED");
        assert_eq!(reparsed.error.source, "workflow_controller");
        assert!(!reparsed.error.retryable);
    }

    #[test]
    fn test_failure_envelope_not_implemented() {
        let envelope = FailureEnvelope::not_implemented("step");
        assert!(!envelope.ok);
        assert_eq!(envelope.error.code, "NOT_IMPLEMENTED");
    }

    #[test]
    fn test_failure_envelope_with_details() {
        let err = ControllerError::WorkflowPaused {
            run_id: "run_123".to_string(),
            phase_id: "gate".to_string(),
            pause_reason: "approval".to_string(),
        };

        let details = serde_json::json!({
            "phase_id": "gate",
            "approval_id": "abc-123"
        });

        let envelope = FailureEnvelope::with_details(&err, details.clone());
        assert!(envelope.error.details.is_some());
        assert_eq!(envelope.error.details.unwrap()["approval_id"], "abc-123");
    }
}