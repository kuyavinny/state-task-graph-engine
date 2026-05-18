use serde::{Deserialize, Serialize};

// Types defined for all 8 PRs; several unused in PR#1.

/// The canonical v1 graph document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Graph {
    pub schema_version: String,
    pub graph_revision: u64,
    pub nodes: Vec<Node>,
}

impl Graph {
    /// Create an empty initial graph.
    pub fn new() -> Self {
        Self {
            schema_version: "1.0".to_string(),
            graph_revision: 0,
            nodes: Vec::new(),
        }
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

/// A single task node in the DAG.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: String,
    pub priority: i32,
    pub status: Status,
    pub dependencies: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub attempts: u32,
    pub max_attempts: u32,
    pub lease: Lease,
    pub result_summary: Option<String>,
    pub failure_reason: Option<String>,
    pub blocked_reason: Option<String>,
    pub skip_reason: Option<String>,
    pub cancel_reason: Option<String>,
    pub evidence: Vec<String>,
    pub artifacts: Vec<String>,
    pub data: serde_json::Value,
}

/// Task status in the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Pending,
    Ready,
    InProgress,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    Skipped,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Pending => write!(f, "PENDING"),
            Status::Ready => write!(f, "READY"),
            Status::InProgress => write!(f, "IN_PROGRESS"),
            Status::Blocked => write!(f, "BLOCKED"),
            Status::Completed => write!(f, "COMPLETED"),
            Status::Failed => write!(f, "FAILED"),
            Status::Cancelled => write!(f, "CANCELLED"),
            Status::Skipped => write!(f, "SKIPPED"),
        }
    }
}

/// Lease metadata for task claiming.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lease {
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub expires_at: Option<String>,
}

impl Lease {
    /// Create an empty (unclaimed) lease.
    pub fn empty() -> Self {
        Self {
            claimed_by: None,
            claimed_at: None,
            expires_at: None,
        }
    }
}

impl Default for Lease {
    fn default() -> Self {
        Self::empty()
    }
}

// Types defined for all 8 PRs; several unused in PR#1.

/// Event log entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(dead_code)]
pub struct Event {
    pub event_id: String,
    pub timestamp: String,
    pub graph_revision_before: u64,
    pub graph_revision_after: u64,
    pub node_id: String,
    pub actor: String,
    pub action: EventAction,
    pub from_status: Option<Status>,
    pub to_status: Option<Status>,
    pub reason: Option<String>,
    pub metadata: serde_json::Value,
}

/// Action types for the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum EventAction {
    Init,
    Claim,
    Heartbeat,
    Release,
    Complete,
    Fail,
    Block,
    Skip,
    Cancel,
    Reopen,
    AppendNodes,
    LeaseExpired,
    DependencyResolved,
    ValidationFailed,
    RejectedWrite,
}

/// Standard error codes for the response envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidSchema,
    InvalidYaml,
    DuplicateNodeId,
    UnknownDependency,
    CycleDetected,
    InvalidTransition,
    StaleRevision,
    LeaseNotOwned,
    TaskNotReady,
    TaskNotFound,
    MaxAttemptsExceeded,
    EventLogDesync,
    AtomicWriteFailed,
    NotImplemented,
    IoError,
    SerializationError,
    Internal,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCode::InvalidSchema => write!(f, "INVALID_SCHEMA"),
            ErrorCode::InvalidYaml => write!(f, "INVALID_YAML"),
            ErrorCode::DuplicateNodeId => write!(f, "DUPLICATE_NODE_ID"),
            ErrorCode::UnknownDependency => write!(f, "UNKNOWN_DEPENDENCY"),
            ErrorCode::CycleDetected => write!(f, "CYCLE_DETECTED"),
            ErrorCode::InvalidTransition => write!(f, "INVALID_TRANSITION"),
            ErrorCode::StaleRevision => write!(f, "STALE_REVISION"),
            ErrorCode::LeaseNotOwned => write!(f, "LEASE_NOT_OWNED"),
            ErrorCode::TaskNotReady => write!(f, "TASK_NOT_READY"),
            ErrorCode::TaskNotFound => write!(f, "TASK_NOT_FOUND"),
            ErrorCode::MaxAttemptsExceeded => write!(f, "MAX_ATTEMPTS_EXCEEDED"),
            ErrorCode::EventLogDesync => write!(f, "EVENT_LOG_DESYNC"),
            ErrorCode::AtomicWriteFailed => write!(f, "ATOMIC_WRITE_FAILED"),
            ErrorCode::NotImplemented => write!(f, "NOT_IMPLEMENTED"),
            ErrorCode::IoError => write!(f, "IO_ERROR"),
            ErrorCode::SerializationError => write!(f, "SERIALIZATION_ERROR"),
            ErrorCode::Internal => write!(f, "INTERNAL"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_new_is_valid_empty() {
        let g = Graph::new();
        assert_eq!(g.schema_version, "1.0");
        assert_eq!(g.graph_revision, 0);
        assert!(g.nodes.is_empty());
    }

    #[test]
    fn status_display_matches_spec() {
        assert_eq!(Status::Pending.to_string(), "PENDING");
        assert_eq!(Status::Ready.to_string(), "READY");
        assert_eq!(Status::InProgress.to_string(), "IN_PROGRESS");
        assert_eq!(Status::Blocked.to_string(), "BLOCKED");
        assert_eq!(Status::Completed.to_string(), "COMPLETED");
        assert_eq!(Status::Failed.to_string(), "FAILED");
        assert_eq!(Status::Cancelled.to_string(), "CANCELLED");
        assert_eq!(Status::Skipped.to_string(), "SKIPPED");
    }

    #[test]
    fn status_serde_roundtrip() {
        let json = serde_json::to_string(&Status::InProgress).unwrap();
        assert_eq!(json, "\"IN_PROGRESS\"");
        let back: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Status::InProgress);
    }

    #[test]
    fn lease_empty_is_null_fields() {
        let l = Lease::empty();
        assert!(l.claimed_by.is_none());
        assert!(l.claimed_at.is_none());
        assert!(l.expires_at.is_none());
    }

    #[test]
    fn error_code_display_matches_spec() {
        assert_eq!(ErrorCode::StaleRevision.to_string(), "STALE_REVISION");
        assert_eq!(ErrorCode::CycleDetected.to_string(), "CYCLE_DETECTED");
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let json = serde_json::to_string(&ErrorCode::LeaseNotOwned).unwrap();
        assert_eq!(json, "\"LEASE_NOT_OWNED\"");
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ErrorCode::LeaseNotOwned);
    }

    #[test]
    fn graph_yaml_roundtrip() {
        let g = Graph::new();
        let yaml = serde_yaml::to_string(&g).unwrap();
        let back: Graph = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(g, back);
    }
}
