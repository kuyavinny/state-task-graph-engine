use crate::error::{AdapterError, AdapterErrorCode, ErrorSource};
use serde::Serialize;

/// Adapter version constant.
pub const ADAPTER_VERSION: &str = "1.0.0";

/// Standard success JSON envelope for all adapter CLI output.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SuccessEnvelope<T: Serialize> {
    pub ok: bool,
    pub adapter_version: String,
    pub profile: String,
    pub actor: String,
    pub data: T,
    pub warnings: Vec<String>,
}

/// Standard failure JSON envelope for all adapter CLI output.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FailureEnvelope {
    pub ok: bool,
    pub adapter_version: String,
    pub profile: String,
    pub actor: String,
    pub error: ErrorBody,
}

/// Structured error body in a failure envelope.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorBody {
    pub code: AdapterErrorCode,
    pub source: ErrorSource,
    pub message: String,
    pub retryable: bool,
    pub agent_action: String,
    pub human_action: String,
    pub details: serde_json::Value,
}

impl<T: Serialize> SuccessEnvelope<T> {
    /// Create a success envelope.
    pub fn new(profile: &str, actor: &str, data: T) -> Self {
        Self {
            ok: true,
            adapter_version: ADAPTER_VERSION.to_string(),
            profile: profile.to_string(),
            actor: actor.to_string(),
            data,
            warnings: Vec::new(),
        }
    }

    #[allow(dead_code)]
    /// Create a success envelope with warnings.
    pub fn with_warnings(profile: &str, actor: &str, data: T, warnings: Vec<String>) -> Self {
        Self {
            ok: true,
            adapter_version: ADAPTER_VERSION.to_string(),
            profile: profile.to_string(),
            actor: actor.to_string(),
            data,
            warnings,
        }
    }
}

impl FailureEnvelope {
    /// Create a failure envelope from an AdapterError.
    pub fn from_error(profile: &str, actor: &str, err: &AdapterError) -> Self {
        Self {
            ok: false,
            adapter_version: ADAPTER_VERSION.to_string(),
            profile: profile.to_string(),
            actor: actor.to_string(),
            error: ErrorBody {
                code: err.error_code(),
                source: err.source_tag(),
                message: err.to_string(),
                retryable: err.retryable(),
                agent_action: err.agent_action().to_string(),
                human_action: err.human_action().to_string(),
                details: err.details(),
            },
        }
    }
}

/// Output a success envelope as pretty-printed JSON to stdout.
pub fn output_success<T: Serialize>(envelope: &SuccessEnvelope<T>) -> Result<(), AdapterError> {
    let json = serde_json::to_string_pretty(envelope).map_err(|e| AdapterError::Json {
        message: e.to_string(),
    })?;
    println!("{}", json);
    Ok(())
}

/// Output a failure envelope as pretty-printed JSON to stderr and return the error.
pub fn output_failure(profile: &str, actor: &str, err: &AdapterError) -> Result<(), AdapterError> {
    let envelope = FailureEnvelope::from_error(profile, actor, err);
    let json = serde_json::to_string_pretty(&envelope).map_err(|e| AdapterError::Json {
        message: e.to_string(),
    })?;
    eprintln!("{}", json);
    Err(err.clone())
}

/// Resolve the actor string for the given profile from the config.
/// Returns "unknown" if the profile is not found.
pub fn resolve_actor(config: &crate::config::AdapterConfig, profile_name: &str) -> String {
    config
        .profiles
        .iter()
        .find(|p| p.name == profile_name)
        .map(|p| format!("agent_{}", p.agent_identity.runtime))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_envelope_formats_correctly() {
        let env = SuccessEnvelope::new(
            "read_only_agent",
            "agent_claude_code",
            serde_json::json!({"initialized": true}),
        );
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["adapter_version"], "1.0.0");
        assert_eq!(json["profile"], "read_only_agent");
        assert_eq!(json["actor"], "agent_claude_code");
        assert_eq!(json["data"]["initialized"], true);
    }

    #[test]
    fn failure_envelope_formats_correctly() {
        let err = AdapterError::ProfileNotFound {
            name: "missing".to_string(),
        };
        let env = FailureEnvelope::from_error("missing", "unknown", &err);
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"]["code"], "PROFILE_NOT_FOUND");
        assert_eq!(json["error"]["source"], "adapter");
        assert_eq!(json["error"]["retryable"], false);
        assert_eq!(json["error"]["agent_action"], "FIX_PROFILE_CONFIG");
    }

    #[test]
    fn failure_envelope_contains_all_fields() {
        let err = AdapterError::InvalidProfile {
            message: "bad yaml".to_string(),
        };
        let env = FailureEnvelope::from_error("test", "agent_test", &err);
        assert_eq!(env.error.code, AdapterErrorCode::INVALID_PROFILE);
        assert_eq!(env.error.source, ErrorSource::Adapter);
        assert!(!env.error.retryable);
        assert_eq!(env.error.agent_action, "FIX_PROFILE_CONFIG");
        assert_eq!(env.error.details["message"], "bad yaml");
    }

    #[test]
    fn success_envelope_with_warnings() {
        let env = SuccessEnvelope::with_warnings(
            "full_exec_agent",
            "agent_openhands",
            serde_json::json!({"x": 1}),
            vec!["desync warning".to_string()],
        );
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["warnings"][0], "desync warning");
    }

    #[test]
    fn resolve_actor_from_config() {
        let config = crate::config::default_config();
        let actor = resolve_actor(&config, "read_only_agent");
        assert_eq!(actor, "agent_claude_code");
        let actor = resolve_actor(&config, "full_exec_agent");
        assert_eq!(actor, "agent_openhands");
    }

    #[test]
    fn resolve_actor_unknown_profile() {
        let config = crate::config::default_config();
        let actor = resolve_actor(&config, "nonexistent");
        assert_eq!(actor, "unknown");
    }
}
