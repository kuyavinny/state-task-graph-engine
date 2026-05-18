use crate::model::ErrorCode;
use thiserror::Error;

// Error variants are defined for all 8 PRs; many are unused in PR#1.
#[allow(dead_code)]
/// All application errors map to spec-defined error codes.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid YAML: {message}")]
    InvalidYaml { message: String },

    #[error("Schema validation failed: {message}")]
    InvalidSchema { message: String },

    #[error("Duplicate node ID: {id}")]
    DuplicateNodeId { id: String },

    #[error("Unknown dependency: {id}")]
    UnknownDependency { id: String },

    #[error("Cycle detected in dependencies")]
    CycleDetected,

    #[error("Invalid argument: {message}")]
    InvalidArgument { message: String },

    #[error("Invalid state transition: cannot {action} on {current_status}")]
    InvalidTransition {
        action: String,
        current_status: String,
    },

    #[error("Stale revision: expected {expected}, got {provided}")]
    StaleRevision { expected: u64, provided: u64 },

    #[error("Lease not owned by actor")]
    LeaseNotOwned,

    #[error("Task not ready: {id}")]
    TaskNotReady { id: String },

    #[error("Task not found: {id}")]
    TaskNotFound { id: String },

    #[error("Max attempts exceeded for task: {id}")]
    MaxAttemptsExceeded { id: String },

    #[error("Event log desync detected")]
    EventLogDesync,

    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("Atomic write failed: {message}")]
    AtomicWriteFailed { message: String },

    #[error("Command not implemented: {0}")]
    NotImplemented(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Graph validation failed with {count} error(s)")]
    GraphValidationFailed {
        count: usize,
        errors: Vec<crate::model::ValidationError>,
    },
}

impl AppError {
    /// Map an AppError to its spec-defined ErrorCode.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            AppError::InvalidYaml { .. } => ErrorCode::InvalidYaml,
            AppError::InvalidSchema { .. } => ErrorCode::InvalidSchema,
            AppError::DuplicateNodeId { .. } => ErrorCode::DuplicateNodeId,
            AppError::UnknownDependency { .. } => ErrorCode::UnknownDependency,
            AppError::CycleDetected => ErrorCode::CycleDetected,
            AppError::InvalidTransition { .. } => ErrorCode::InvalidTransition,
            AppError::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            AppError::StaleRevision { .. } => ErrorCode::StaleRevision,
            AppError::LeaseNotOwned => ErrorCode::LeaseNotOwned,
            AppError::TaskNotReady { .. } => ErrorCode::TaskNotReady,
            AppError::TaskNotFound { .. } => ErrorCode::TaskNotFound,
            AppError::MaxAttemptsExceeded { .. } => ErrorCode::MaxAttemptsExceeded,
            AppError::EventLogDesync => ErrorCode::EventLogDesync,
            AppError::FileNotFound { .. } => ErrorCode::FileNotFound,
            AppError::AtomicWriteFailed { .. } => ErrorCode::AtomicWriteFailed,
            AppError::NotImplemented(_) => ErrorCode::NotImplemented,
            AppError::Io(_) => ErrorCode::IoError,
            AppError::Serialization(_) => ErrorCode::SerializationError,
            AppError::GraphValidationFailed { .. } => ErrorCode::ValidationFailed,
        }
    }

    /// Convert to a structured details map for the error envelope.
    pub fn details(&self) -> serde_json::Value {
        match self {
            AppError::StaleRevision { expected, provided } => serde_json::json!({
                "expected": expected,
                "provided": provided,
            }),
            AppError::InvalidTransition {
                action,
                current_status,
            } => serde_json::json!({
                "action": action,
                "current_status": current_status,
            }),
            AppError::DuplicateNodeId { id } => serde_json::json!({
                "id": id,
            }),
            AppError::UnknownDependency { id } => serde_json::json!({
                "id": id,
            }),
            AppError::TaskNotReady { id } => serde_json::json!({
                "id": id,
            }),
            AppError::TaskNotFound { id } => serde_json::json!({
                "id": id,
            }),
            AppError::MaxAttemptsExceeded { id } => serde_json::json!({
                "id": id,
            }),
            AppError::FileNotFound { path } => serde_json::json!({
                "path": path,
            }),
            AppError::AtomicWriteFailed { message } => serde_json::json!({
                "message": message,
            }),
            AppError::GraphValidationFailed { count, errors } => serde_json::json!({
                "count": count,
                "errors": errors,
            }),
            AppError::InvalidArgument { message } => serde_json::json!({
                "message": message,
            }),
            _ => serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

impl From<serde_yaml::Error> for AppError {
    fn from(e: serde_yaml::Error) -> Self {
        AppError::InvalidYaml {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serialization(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_maps_to_correct_error_codes() {
        assert_eq!(
            AppError::CycleDetected.error_code(),
            ErrorCode::CycleDetected
        );
        assert_eq!(
            AppError::StaleRevision {
                expected: 5,
                provided: 3
            }
            .error_code(),
            ErrorCode::StaleRevision
        );
        assert_eq!(
            AppError::LeaseNotOwned.error_code(),
            ErrorCode::LeaseNotOwned
        );
        assert_eq!(
            AppError::NotImplemented("test".into()).error_code(),
            ErrorCode::NotImplemented
        );
    }

    #[test]
    fn stale_revision_details() {
        let err = AppError::StaleRevision {
            expected: 5,
            provided: 3,
        };
        let details = err.details();
        assert_eq!(details["expected"], 5);
        assert_eq!(details["provided"], 3);
    }

    #[test]
    fn serde_yaml_error_converts() {
        let result: Result<crate::model::Graph, _> = serde_yaml::from_str("{{invalid yaml");
        match result {
            Err(e) => {
                let app_err: AppError = e.into();
                assert!(matches!(app_err, AppError::InvalidYaml { .. }));
            }
            Ok(_) => panic!("Expected YAML parse error"),
        }
    }
}
