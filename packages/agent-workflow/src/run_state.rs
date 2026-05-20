use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Workflow Run State
// ---------------------------------------------------------------------------

/// Per-run state persisted in `.agent/workflow_runs/<run_id>/run_state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunState {
    pub workflow_run_id: String,
    pub workflow_id: String,
    pub workflow_version: String,
    pub adapter_profile: String,
    pub current_phase: Option<String>,
    pub phase_status: PhaseStatus,
    pub active_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_task_graph_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_task_lease_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_task_packet_ref: Option<String>,
    pub start_time: String,
    #[serde(default = "utc_now")]
    pub updated_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub workflow_retry_counters: WorkflowRetryCounters,
    #[serde(default)]
    pub approval_records: Vec<ApprovalRecord>,
    #[serde(default)]
    pub phase_history: Vec<PhaseHistoryItem>,
    #[serde(default)]
    pub run_artifacts: Vec<RunArtifact>,
}

/// Phase status lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhaseStatus {
    #[serde(rename = "WAITING")]
    Waiting,
    #[serde(rename = "IN_PROGRESS")]
    InProgress,
    #[serde(rename = "PAUSED")]
    Paused,
    #[serde(rename = "COMPLETED")]
    Completed,
    #[serde(rename = "FAILED")]
    Failed,
    #[serde(rename = "CANCELLED")]
    Cancelled,
}

/// Retry counters at the workflow level.
/// Independent from Module 1 task `attempts`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowRetryCounters {
    pub total_attempts: u64,
    pub sequential_task_failures: u64,
    pub max_workflow_retries: u64,
}

/// A single operator approval decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub phase_id: String,
    pub operator: String,
    pub decision: ApprovalDecision,
    pub reason: String,
    pub timestamp: String,
}

/// Approval decision values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    #[serde(rename = "APPROVED")]
    Approved,
    #[serde(rename = "REJECTED")]
    Rejected,
    #[serde(rename = "DEFERRED")]
    Deferred,
}

/// A single phase entry in the run history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseHistoryItem {
    pub phase_id: String,
    pub status: PhaseStatus,
    pub entered_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exited_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_packet_id: Option<String>,
}

/// Reference to an artifact within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunArtifact {
    pub r#type: String,
    pub path: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn utc_now() -> String {
    Utc::now().to_rfc3339()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_run_state_json_roundtrip() {
        let state = WorkflowRunState {
            workflow_run_id: "run_2026-05-20T14-30-00_abc123".to_string(),
            workflow_id: "api_deployment_v1".to_string(),
            workflow_version: "1.0.0".to_string(),
            adapter_profile: "full_exec_agent".to_string(),
            current_phase: Some("deploy".to_string()),
            phase_status: PhaseStatus::InProgress,
            active_task_id: Some("build_and_push_image".to_string()),
            active_task_graph_revision: Some(42),
            active_task_lease_expires_at: Some("2026-05-20T15:00:00Z".to_string()),
            active_task_packet_ref: Some("task_packets/pkt_abc123.json".to_string()),
            start_time: "2026-05-20T14:00:00Z".to_string(),
            updated_time: "2026-05-20T14:30:00Z".to_string(),
            pause_reason: None,
            stop_reason: None,
            workflow_retry_counters: WorkflowRetryCounters {
                total_attempts: 2,
                sequential_task_failures: 0,
                max_workflow_retries: 3,
            },
            approval_records: vec![ApprovalRecord {
                approval_id: "a3f7c2d9-e1b2-4c5d-8f6a-123456789abc".to_string(),
                phase_id: "verification_gate".to_string(),
                operator: "alice".to_string(),
                decision: ApprovalDecision::Approved,
                reason: "Smoke tests passed.".to_string(),
                timestamp: "2026-05-20T14:15:00Z".to_string(),
            }],
            phase_history: vec![
                PhaseHistoryItem {
                    phase_id: "setup".to_string(),
                    status: PhaseStatus::Completed,
                    entered_at: "2026-05-20T14:00:00Z".to_string(),
                    exited_at: Some("2026-05-20T14:10:00Z".to_string()),
                    exit_reason: Some("criteria_met".to_string()),
                    result_packet_id: Some("res_abc123".to_string()),
                },
                PhaseHistoryItem {
                    phase_id: "deploy".to_string(),
                    status: PhaseStatus::InProgress,
                    entered_at: "2026-05-20T14:10:00Z".to_string(),
                    exited_at: None,
                    exit_reason: None,
                    result_packet_id: None,
                },
            ],
            run_artifacts: vec![RunArtifact {
                r#type: "result_packet_ref".to_string(),
                path: "result_packets/res_abc123.json".to_string(),
                timestamp: "2026-05-20T14:10:00Z".to_string(),
            }],
        };

        let json = serde_json::to_string_pretty(&state).expect("serialize state");
        let reparsed: WorkflowRunState = serde_json::from_str(&json).expect("deserialize state");

        assert_eq!(reparsed.workflow_run_id, state.workflow_run_id);
        assert_eq!(reparsed.workflow_id, state.workflow_id);
        assert_eq!(reparsed.current_phase, state.current_phase);
        assert_eq!(reparsed.phase_status, state.phase_status);
        assert_eq!(reparsed.active_task_id, state.active_task_id);
        assert_eq!(
            reparsed.active_task_graph_revision,
            state.active_task_graph_revision
        );
        assert_eq!(
            reparsed.active_task_lease_expires_at,
            state.active_task_lease_expires_at
        );
        assert_eq!(
            reparsed.active_task_packet_ref,
            state.active_task_packet_ref
        );
        assert_eq!(reparsed.workflow_retry_counters.total_attempts, 2);
        assert_eq!(reparsed.approval_records.len(), 1);
        assert_eq!(
            reparsed.approval_records[0].decision,
            ApprovalDecision::Approved
        );
        assert_eq!(reparsed.phase_history.len(), 2);
        assert_eq!(reparsed.phase_history[0].status, PhaseStatus::Completed);
        assert_eq!(reparsed.phase_history[1].status, PhaseStatus::InProgress);
        assert_eq!(reparsed.run_artifacts.len(), 1);
    }

    #[test]
    fn test_workflow_run_state_defaults() {
        // Test that default values are applied correctly for optional fields
        let json = r#"
        {
            "workflow_run_id": "run_test_123",
            "workflow_id": "test_workflow",
            "workflow_version": "1.0.0",
            "adapter_profile": "test_profile",
            "current_phase": null,
            "phase_status": "WAITING",
            "active_task_id": null,
            "start_time": "2026-05-20T14:00:00Z"
        }
        "#;

        let state: WorkflowRunState = serde_json::from_str(json).expect("parse state");
        assert_eq!(state.phase_status, PhaseStatus::Waiting);
        assert_eq!(state.workflow_retry_counters.total_attempts, 0);
        assert!(state.approval_records.is_empty());
        assert!(state.phase_history.is_empty());
        assert!(state.run_artifacts.is_empty());
        assert_eq!(state.pause_reason, None);
        assert_eq!(state.stop_reason, None);
    }

    #[test]
    fn test_approval_decision_serialization() {
        let decisions = vec![
            ApprovalDecision::Approved,
            ApprovalDecision::Rejected,
            ApprovalDecision::Deferred,
        ];

        for decision in decisions {
            let json = serde_json::to_string(&decision).expect("serialize decision");
            let reparsed: ApprovalDecision =
                serde_json::from_str(&json).expect("deserialize decision");
            assert_eq!(reparsed, decision);
        }
    }

    #[test]
    fn test_phase_status_serialization() {
        let statuses = vec![
            PhaseStatus::Waiting,
            PhaseStatus::InProgress,
            PhaseStatus::Paused,
            PhaseStatus::Completed,
            PhaseStatus::Failed,
            PhaseStatus::Cancelled,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).expect("serialize status");
            let reparsed: PhaseStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(reparsed, status);
        }
    }
}
