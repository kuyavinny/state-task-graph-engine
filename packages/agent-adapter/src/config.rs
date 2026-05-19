use serde::{Deserialize, Serialize};

/// Top-level adapter configuration loaded from `.agent/adapter.config.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdapterConfig {
    pub schema_version: String,
    pub graph_engine_binary_path: String,
    pub default_profile: String,
    pub profiles: Vec<Profile>,
}

/// A single capability profile within the adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    pub agent_identity: AgentIdentity,
    pub capabilities: Capabilities,
    pub permissions: Permissions,
    pub policies: Policies,
}

/// Identity metadata for the runtime associated with a profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentIdentity {
    pub runtime: String,
    pub version: String,
}

/// Capability declaration — what the runtime can do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capabilities {
    pub read_files: bool,
    pub write_files: bool,
    pub execute_shell: bool,
    pub run_tests: bool,
    pub network_access: bool,
    pub browser_access: bool,
    pub long_running_tasks: bool,
    pub max_task_minutes: u64,
    pub preferred_format: String,
    pub max_context_chars: u64,
}

/// Permission declaration — what the adapter allows the profile to report or request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Permissions {
    pub allow_claim: bool,
    pub allow_submit_success: bool,
    pub allow_submit_fail: bool,
    pub allow_submit_blocked: bool,
    pub allow_skip: bool,
    pub allow_cancel: bool,
    pub allow_release: bool,
}

/// Policy configuration for result handling, retries, logging, and artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Policies {
    pub result_policy: String,
    pub retry_policy: String,
    pub logging_policy: String,
    pub artifact_policy: ArtifactPolicy,
}

/// Artifact size and storage policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactPolicy {
    pub max_copied_artifact_bytes: u64,
    pub max_total_copied_bytes: u64,
}

/// Default configuration template used by `init-profile`.
pub fn default_config() -> AdapterConfig {
    AdapterConfig {
        schema_version: "1.0".to_string(),
        graph_engine_binary_path: "./target/release/stage".to_string(),
        default_profile: "read_only_agent".to_string(),
        profiles: vec![
            Profile {
                name: "read_only_agent".to_string(),
                agent_identity: AgentIdentity {
                    runtime: "claude_code".to_string(),
                    version: "1.0.0".to_string(),
                },
                capabilities: Capabilities {
                    read_files: true,
                    write_files: false,
                    execute_shell: false,
                    run_tests: false,
                    network_access: false,
                    browser_access: false,
                    long_running_tasks: false,
                    max_task_minutes: 10,
                    preferred_format: "markdown".to_string(),
                    max_context_chars: 16000,
                },
                permissions: Permissions {
                    allow_claim: true,
                    allow_submit_success: true,
                    allow_submit_fail: true,
                    allow_submit_blocked: true,
                    allow_skip: false,
                    allow_cancel: false,
                    allow_release: true,
                },
                policies: Policies {
                    result_policy: "strict_validation".to_string(),
                    retry_policy: "fail_fast".to_string(),
                    logging_policy: "debug".to_string(),
                    artifact_policy: ArtifactPolicy {
                        max_copied_artifact_bytes: 1048576,
                        max_total_copied_bytes: 5242880,
                    },
                },
            },
            Profile {
                name: "full_exec_agent".to_string(),
                agent_identity: AgentIdentity {
                    runtime: "openhands".to_string(),
                    version: "1.5.0".to_string(),
                },
                capabilities: Capabilities {
                    read_files: true,
                    write_files: true,
                    execute_shell: true,
                    run_tests: true,
                    network_access: true,
                    browser_access: false,
                    long_running_tasks: true,
                    max_task_minutes: 120,
                    preferred_format: "json".to_string(),
                    max_context_chars: 64000,
                },
                permissions: Permissions {
                    allow_claim: true,
                    allow_submit_success: true,
                    allow_submit_fail: true,
                    allow_submit_blocked: true,
                    allow_skip: true,
                    allow_cancel: true,
                    allow_release: true,
                },
                policies: Policies {
                    result_policy: "allow_artifacts".to_string(),
                    retry_policy: "fail_fast".to_string(),
                    logging_policy: "standard".to_string(),
                    artifact_policy: ArtifactPolicy {
                        max_copied_artifact_bytes: 1048576,
                        max_total_copied_bytes: 5242880,
                    },
                },
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_two_profiles() {
        let config = default_config();
        assert_eq!(config.profiles.len(), 2);
        assert_eq!(config.profiles[0].name, "read_only_agent");
        assert_eq!(config.profiles[1].name, "full_exec_agent");
    }

    #[test]
    fn config_roundtrips_through_yaml() {
        let config = default_config();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: AdapterConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn config_roundtrips_through_json() {
        let config = default_config();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AdapterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }
}
