use thiserror::Error;

/// The source of an error — either the adapter itself or the underlying graph engine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(non_camel_case_types)]
pub enum AdapterErrorCode {
    NO_WORK_AVAILABLE,
    PROFILE_NOT_FOUND,
    INVALID_PROFILE,
    PROFILE_PERMISSION_DENIED,
    GRAPH_ENGINE_UNAVAILABLE,
    GRAPH_ENGINE_NONZERO_EXIT,
    GRAPH_ENGINE_MALFORMED_JSON,
    GRAPH_ENGINE_FAILURE,
    CONTEXT_STALE_REFETCH_REQUIRED,
    CLAIM_FAILED,
    SUMMARIZE_FAILED_AFTER_CLAIM,
    INVALID_RESULT_PACKET,
    ARTIFACT_POLICY_VIOLATION,
    LEASE_NOT_OWNED,
    TASK_MAY_REMAIN_LEASED,
    UNKNOWN_ADAPTER_ERROR,
}

/// The source of an error — either the adapter itself or the underlying graph engine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSource {
    Adapter,
    GraphEngine,
}

/// All application errors map to spec-defined adapter error codes.
#[allow(dead_code)]
#[derive(Debug, Clone, Error)]
pub enum AdapterError {
    #[error("No work available")]
    NoWorkAvailable,

    #[error("Profile not found: {name}")]
    ProfileNotFound { name: String },

    #[error("Invalid profile configuration: {message}")]
    InvalidProfile { message: String },

    #[error("Profile permission denied: {message}")]
    ProfilePermissionDenied { message: String },

    #[error("Graph engine unavailable")]
    GraphEngineUnavailable,

    #[error("Graph engine exited with non-zero code: {message}")]
    GraphEngineNonzeroExit { message: String },

    #[error("Graph engine returned malformed JSON: {message}")]
    GraphEngineMalformedJson { message: String },

    #[error("Graph engine failure: {message}")]
    GraphEngineFailure { message: String },

    #[error("Context stale: revision mismatch. {message}")]
    ContextStaleRefetchRequired { message: String },

    #[error("Claim failed: {message}")]
    ClaimFailed { message: String },

    #[error("Summarize failed after claim: {message}")]
    SummarizeFailedAfterClaim { message: String },

    #[error("Invalid result packet: {message}")]
    InvalidResultPacket { message: String },

    #[error("Artifact policy violation: {message}")]
    ArtifactPolicyViolation { message: String },

    #[error("Lease not owned by actor")]
    LeaseNotOwned,

    #[error("Task may remain leased")]
    TaskMayRemainLeased,

    #[error("Unknown adapter error: {message}")]
    Unknown { message: String },

    #[error("IO error: {message}")]
    Io { message: String },

    #[error("YAML error: {message}")]
    Yaml { message: String },

    #[error("JSON error: {message}")]
    Json { message: String },
}

impl From<std::io::Error> for AdapterError {
    fn from(e: std::io::Error) -> Self {
        AdapterError::Io {
            message: e.to_string(),
        }
    }
}

impl From<serde_yaml::Error> for AdapterError {
    fn from(e: serde_yaml::Error) -> Self {
        AdapterError::Yaml {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for AdapterError {
    fn from(e: serde_json::Error) -> Self {
        AdapterError::Json {
            message: e.to_string(),
        }
    }
}

impl AdapterError {
    /// Map an AdapterError to its spec-defined error code.
    pub fn error_code(&self) -> AdapterErrorCode {
        match self {
            AdapterError::NoWorkAvailable => AdapterErrorCode::NO_WORK_AVAILABLE,
            AdapterError::ProfileNotFound { .. } => AdapterErrorCode::PROFILE_NOT_FOUND,
            AdapterError::InvalidProfile { .. } => AdapterErrorCode::INVALID_PROFILE,
            AdapterError::ProfilePermissionDenied { .. } => {
                AdapterErrorCode::PROFILE_PERMISSION_DENIED
            }
            AdapterError::GraphEngineUnavailable => AdapterErrorCode::GRAPH_ENGINE_UNAVAILABLE,
            AdapterError::GraphEngineNonzeroExit { .. } => {
                AdapterErrorCode::GRAPH_ENGINE_NONZERO_EXIT
            }
            AdapterError::GraphEngineMalformedJson { .. } => {
                AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON
            }
            AdapterError::GraphEngineFailure { .. } => AdapterErrorCode::GRAPH_ENGINE_FAILURE,
            AdapterError::ContextStaleRefetchRequired { .. } => {
                AdapterErrorCode::CONTEXT_STALE_REFETCH_REQUIRED
            }
            AdapterError::ClaimFailed { .. } => AdapterErrorCode::CLAIM_FAILED,
            AdapterError::SummarizeFailedAfterClaim { .. } => {
                AdapterErrorCode::SUMMARIZE_FAILED_AFTER_CLAIM
            }
            AdapterError::InvalidResultPacket { .. } => AdapterErrorCode::INVALID_RESULT_PACKET,
            AdapterError::ArtifactPolicyViolation { .. } => {
                AdapterErrorCode::ARTIFACT_POLICY_VIOLATION
            }
            AdapterError::LeaseNotOwned => AdapterErrorCode::LEASE_NOT_OWNED,
            AdapterError::TaskMayRemainLeased => AdapterErrorCode::TASK_MAY_REMAIN_LEASED,
            AdapterError::Unknown { .. } => AdapterErrorCode::UNKNOWN_ADAPTER_ERROR,
            AdapterError::Io { .. } => AdapterErrorCode::UNKNOWN_ADAPTER_ERROR,
            AdapterError::Yaml { .. } => AdapterErrorCode::INVALID_PROFILE,
            AdapterError::Json { .. } => AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON,
        }
    }

    /// Map an AdapterError to its source.
    pub fn source_tag(&self) -> ErrorSource {
        match self {
            AdapterError::GraphEngineUnavailable
            | AdapterError::GraphEngineNonzeroExit { .. }
            | AdapterError::GraphEngineMalformedJson { .. }
            | AdapterError::GraphEngineFailure { .. }
            | AdapterError::ContextStaleRefetchRequired { .. } => ErrorSource::GraphEngine,
            _ => ErrorSource::Adapter,
        }
    }

    /// Whether the error is retryable.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            AdapterError::NoWorkAvailable
                | AdapterError::GraphEngineUnavailable
                | AdapterError::GraphEngineNonzeroExit { .. }
        )
    }

    /// Suggested action for the agent runtime.
    pub fn agent_action(&self) -> &'static str {
        match self {
            AdapterError::NoWorkAvailable => "POLL_LATER",
            AdapterError::ContextStaleRefetchRequired { .. } => "REFETCH_WORK",
            AdapterError::GraphEngineUnavailable => "RETRY",
            AdapterError::GraphEngineNonzeroExit { .. } => "RETRY",
            AdapterError::GraphEngineFailure { .. } => "INVESTIGATE",
            AdapterError::LeaseNotOwned => "RELEASE_AND_REFETCH",
            AdapterError::ProfileNotFound { .. } => "FIX_PROFILE_CONFIG",
            AdapterError::InvalidProfile { .. } => "FIX_PROFILE_CONFIG",
            AdapterError::ProfilePermissionDenied { .. } => "USE_PERMITTED_PROFILE",
            AdapterError::InvalidResultPacket { .. } => "FIX_RESULT_PACKET",
            AdapterError::ArtifactPolicyViolation { .. } => "FIX_ARTIFACT_PATHS",
            _ => "INVESTIGATE",
        }
    }

    /// Suggested action for the human operator.
    pub fn human_action(&self) -> &'static str {
        match self {
            AdapterError::NoWorkAvailable => "None",
            AdapterError::ProfileNotFound { .. } => "Check adapter.config.yaml profiles",
            AdapterError::InvalidProfile { .. } => "Fix YAML syntax or schema errors",
            AdapterError::ProfilePermissionDenied { .. } => "Review profile permissions in config",
            AdapterError::GraphEngineUnavailable => "Ensure agent-graph binary is available",
            AdapterError::GraphEngineFailure { .. } => "Check graph engine logs",
            AdapterError::ArtifactPolicyViolation { .. } => "Review artifact policy limits",
            _ => "Investigate adapter logs",
        }
    }

    /// Convert to a structured details map for the failure envelope.
    pub fn details(&self) -> serde_json::Value {
        match self {
            AdapterError::ProfileNotFound { name } => serde_json::json!({ "profile": name }),
            AdapterError::ContextStaleRefetchRequired { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::GraphEngineNonzeroExit { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::GraphEngineMalformedJson { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::GraphEngineFailure { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::ClaimFailed { message } => serde_json::json!({ "message": message }),
            AdapterError::InvalidResultPacket { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::ArtifactPolicyViolation { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::ProfilePermissionDenied { message } => {
                serde_json::json!({ "message": message })
            }
            AdapterError::InvalidProfile { message } => serde_json::json!({ "message": message }),
            AdapterError::Unknown { message } => serde_json::json!({ "message": message }),
            AdapterError::SummarizeFailedAfterClaim { message } => {
                serde_json::json!({ "message": message, "code": "TASK_MAY_REMAIN_LEASED" })
            }
            _ => serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_match_spec() {
        assert_eq!(
            AdapterError::NoWorkAvailable.error_code(),
            AdapterErrorCode::NO_WORK_AVAILABLE
        );
        assert_eq!(
            AdapterError::ProfileNotFound { name: "x".into() }.error_code(),
            AdapterErrorCode::PROFILE_NOT_FOUND
        );
        assert_eq!(
            AdapterError::GraphEngineMalformedJson {
                message: "bad".into()
            }
            .error_code(),
            AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON
        );
        assert_eq!(
            AdapterError::ContextStaleRefetchRequired {
                message: "rev".into()
            }
            .error_code(),
            AdapterErrorCode::CONTEXT_STALE_REFETCH_REQUIRED
        );
    }

    #[test]
    fn error_sources_are_correct() {
        assert_eq!(
            AdapterError::GraphEngineUnavailable.source_tag(),
            ErrorSource::GraphEngine
        );
        assert_eq!(
            AdapterError::ProfileNotFound { name: "x".into() }.source_tag(),
            ErrorSource::Adapter
        );
    }

    #[test]
    fn retryable_flags() {
        assert!(AdapterError::NoWorkAvailable.retryable());
        assert!(AdapterError::GraphEngineUnavailable.retryable());
        assert!(!AdapterError::ProfileNotFound { name: "x".into() }.retryable());
    }

    #[test]
    fn agent_actions() {
        assert_eq!(AdapterError::NoWorkAvailable.agent_action(), "POLL_LATER");
        assert_eq!(
            AdapterError::ContextStaleRefetchRequired {
                message: "m".into()
            }
            .agent_action(),
            "REFETCH_WORK"
        );
    }

    #[test]
    fn pr2_error_sources_and_retryability() {
        // PR2 error codes must have correct source and retryability
        assert_eq!(
            AdapterError::GraphEngineNonzeroExit {
                message: "x".into()
            }
            .source_tag(),
            ErrorSource::GraphEngine
        );
        assert_eq!(
            AdapterError::GraphEngineMalformedJson {
                message: "x".into()
            }
            .source_tag(),
            ErrorSource::GraphEngine
        );
        assert_eq!(
            AdapterError::ContextStaleRefetchRequired {
                message: "x".into()
            }
            .source_tag(),
            ErrorSource::GraphEngine
        );
        // STALE_REVISION is NOT retryable — agent must REFETCH_WORK
        assert!(
            !AdapterError::ContextStaleRefetchRequired {
                message: "x".into()
            }
            .retryable()
        );
        // GRAPH_ENGINE_NONZERO_EXIT IS retryable
        assert!(
            AdapterError::GraphEngineNonzeroExit {
                message: "x".into()
            }
            .retryable()
        );
        // ClaimFailed is Adapter-sourced
        assert_eq!(
            AdapterError::ClaimFailed {
                message: "x".into()
            }
            .source_tag(),
            ErrorSource::Adapter
        );
    }
}
