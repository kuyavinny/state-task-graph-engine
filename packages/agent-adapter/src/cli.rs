use crate::config::{AdapterConfig, default_config};
use crate::error::AdapterError;
use crate::graph_client::GraphEngineClient;
use crate::graph_runner::{RealRunner, RealRunnerConfig};
use crate::logger::AdapterLogger;
use crate::response::{self, SuccessEnvelope};
use crate::result_packet::CanonicalResultPacket;
use crate::task_packet::CanonicalTaskPacket;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agent-adapter",
    about = "agent-adapter: universal adapter boundary for agent-graph",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize adapter configuration with default profiles
    InitProfile,
    /// Validate the adapter configuration file
    ValidateProfile,
    /// List all profile names and their identities
    ListProfiles,
    /// Acquire the next available task from the graph engine
    GetWork {
        /// Profile name to use for actor resolution and permissions
        #[arg(long)]
        profile: String,
    },
    /// Submit a result for a completed or terminated task
    SubmitResult {
        /// Profile name to use for actor resolution and permissions
        #[arg(long)]
        profile: String,
        /// Path to a JSON result packet file (optional if using convenience flags)
        #[arg(long)]
        result_file: Option<String>,
        /// Task ID (convenience override, mutually exclusive with result-file)
        #[arg(long)]
        task_id: Option<String>,
        /// Graph revision (convenience override)
        #[arg(long)]
        revision: Option<u64>,
        /// Result status: success | fail | blocked | skipped | cancelled (convenience override)
        #[arg(long)]
        status: Option<String>,
        /// Result summary (convenience override)
        #[arg(long)]
        summary: Option<String>,
        /// Failure/reason text for non-success statuses (convenience override)
        #[arg(long)]
        reason: Option<String>,
    },
    /// Extend the lease on a claimed task
    Heartbeat {
        /// Profile name to use for actor resolution
        #[arg(long)]
        profile: String,
        /// Task ID to heartbeat
        #[arg(long)]
        task_id: String,
        /// Graph revision for optimistic concurrency
        #[arg(long)]
        revision: u64,
        /// Additional TTL in seconds
        #[arg(long, default_value = "300")]
        ttl_seconds: u64,
    },
    /// Release a claimed task back to READY
    ReleaseWork {
        /// Profile name to use for actor resolution and permissions
        #[arg(long)]
        profile: String,
        /// Task ID to release
        #[arg(long)]
        task_id: String,
        /// Graph revision for optimistic concurrency
        #[arg(long)]
        revision: u64,
        /// Reason for releasing work
        #[arg(long)]
        reason: Option<String>,
    },
}

/// Directory for adapter-owned files relative to the project root.
const AGENT_DIR: &str = ".agent";
/// Filename for the adapter configuration file.
const CONFIG_FILE: &str = "adapter.config.yaml";
/// Directory for adapter-owned artifacts.
const ARTIFACTS_DIR: &str = "adapter_artifacts";

impl Cli {
    pub fn run(self) -> Result<(), AdapterError> {
        match self.command {
            Commands::InitProfile => init_profile(),
            Commands::ValidateProfile => validate_profile(),
            Commands::ListProfiles => list_profiles(),
            Commands::GetWork { profile } => get_work(&profile),
            Commands::SubmitResult {
                profile,
                result_file,
                task_id,
                revision,
                status,
                summary,
                reason,
            } => submit_result(
                &profile,
                result_file.as_deref(),
                task_id.as_deref(),
                revision,
                status.as_deref(),
                summary.as_deref(),
                reason.as_deref(),
            ),
            Commands::Heartbeat {
                profile,
                task_id,
                revision,
                ttl_seconds,
            } => heartbeat(&profile, &task_id, revision, ttl_seconds),
            Commands::ReleaseWork {
                profile,
                task_id,
                revision,
                reason,
            } => release_work(&profile, &task_id, revision, reason.as_deref()),
        }
    }
}

/// Path to the adapter configuration file.
/// Propagates errors from `current_dir()` rather than silently defaulting.
fn config_path() -> Result<std::path::PathBuf, AdapterError> {
    Ok(std::env::current_dir()
        .map_err(|e| AdapterError::Io {
            message: format!("cannot determine current directory: {}", e),
        })?
        .join(AGENT_DIR)
        .join(CONFIG_FILE))
}

/// Path to the adapter artifacts directory.
/// Propagates errors from `current_dir()` rather than silently defaulting.
fn artifacts_path() -> Result<std::path::PathBuf, AdapterError> {
    Ok(std::env::current_dir()
        .map_err(|e| AdapterError::Io {
            message: format!("cannot determine current directory: {}", e),
        })?
        .join(AGENT_DIR)
        .join(ARTIFACTS_DIR))
}

/// Path to the .agent/ directory.
fn agent_dir() -> Result<std::path::PathBuf, AdapterError> {
    Ok(std::env::current_dir()
        .map_err(|e| AdapterError::Io {
            message: format!("cannot determine current directory: {}", e),
        })?
        .join(AGENT_DIR))
}

/// Resolve actor name from config for a given profile.
fn resolve_profile_actor(config: &AdapterConfig, profile_name: &str) -> String {
    response::resolve_actor(config, profile_name)
}

/// Initialize adapter configuration: write default config and create artifacts directory.
/// Uses atomic write (temp file + rename) so partial state is not left on failure.
fn init_profile() -> Result<(), AdapterError> {
    let config = default_config();
    let agent_dir = agent_dir()?;
    let artifacts_dir = artifacts_path()?;
    let config_path = config_path()?;

    // Create .agent/ directory if it doesn't exist
    std::fs::create_dir_all(&agent_dir)?;

    // Create .agent/adapter_artifacts/ directory if it doesn't exist
    std::fs::create_dir_all(&artifacts_dir)?;

    // Write config to a temp file then rename for atomicity
    let yaml = serde_yaml::to_string(&config)?;
    let temp_path = config_path.with_extension("yaml.tmp");
    std::fs::write(&temp_path, &yaml)?;
    std::fs::rename(&temp_path, &config_path)?;

    // Output success envelope
    let actor = resolve_profile_actor(&config, &config.default_profile);
    let data = serde_json::json!({
        "initialized": true,
        "config_path": config_path.to_string_lossy(),
        "artifacts_dir": artifacts_dir.to_string_lossy(),
    });
    let envelope = SuccessEnvelope::new(&config.default_profile, &actor, data);
    response::output_success(&envelope)
}

/// Validate the adapter configuration file by parsing it.
fn validate_profile() -> Result<(), AdapterError> {
    let config_path = config_path()?;

    if !config_path.exists() {
        return Err(AdapterError::ProfileNotFound {
            name: config_path.to_string_lossy().to_string(),
        });
    }

    let content = std::fs::read_to_string(&config_path)?;
    let config: AdapterConfig = serde_yaml::from_str(&content)?;
    config.validate()?;

    let actor = resolve_profile_actor(&config, &config.default_profile);
    let data = serde_json::json!({
        "valid": true,
        "profile_count": config.profiles.len(),
        "default_profile": config.default_profile,
    });

    let envelope = SuccessEnvelope::new(&config.default_profile, &actor, data);
    response::output_success(&envelope)
}

/// List all profile names and their identities.
fn list_profiles() -> Result<(), AdapterError> {
    let config_path = config_path()?;

    if !config_path.exists() {
        return Err(AdapterError::ProfileNotFound {
            name: config_path.to_string_lossy().to_string(),
        });
    }

    let content = std::fs::read_to_string(&config_path)?;
    let config: AdapterConfig = serde_yaml::from_str(&content)?;

    let actor = resolve_profile_actor(&config, &config.default_profile);

    let profiles: Vec<serde_json::Value> = config
        .profiles
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "runtime": p.agent_identity.runtime,
                "version": p.agent_identity.version,
                "preferred_format": p.capabilities.preferred_format,
            })
        })
        .collect();

    let data = serde_json::json!({
        "profiles": profiles,
    });

    let envelope = SuccessEnvelope::new(&config.default_profile, &actor, data);
    response::output_success(&envelope)
}

/// Acquire the next available task using the graph engine.
///
/// 1. Loads config and validates the named profile exists.
/// 2. Checks `permissions.allow_claim`.
/// 3. Resolves actor from profile.
/// 4. Builds a `GraphEngineClient` with `RealRunner`.
/// 5. Orchestrates `next → claim → summarize` via [`GraphEngineClient::get_work`].
/// 6. Outputs a `SuccessEnvelope<CanonicalTaskPacket>`.
fn get_work(profile_name: &str) -> Result<(), AdapterError> {
    let (config, profile, actor) = load_profile(profile_name)?;

    if !profile.permissions.allow_claim {
        return Err(AdapterError::ProfilePermissionDenied {
            message: format!(
                "profile '{}' does not have allow_claim permission",
                profile_name
            ),
        });
    }

    let client = build_client(&config, &actor)?;
    let packet: CanonicalTaskPacket = client.get_work(&actor)?;

    let envelope = SuccessEnvelope::new(profile_name, &actor, serde_json::to_value(packet)?);
    response::output_success(&envelope)
}

/// Submit a result for a completed or terminated task.
///
/// 1. Load config and profile.
/// 2. Build packet from `--result-file` OR convenience flags.
/// 3. Validate packet (required fields per status).
/// 4. Check permissions per status.
/// 5. Map status to graph mutation command.
/// 6. Call graph engine with revision.
fn submit_result(
    profile_name: &str,
    result_file: Option<&str>,
    task_id: Option<&str>,
    revision: Option<u64>,
    status: Option<&str>,
    summary: Option<&str>,
    reason: Option<&str>,
) -> Result<(), AdapterError> {
    let (config, profile, actor) = load_profile(profile_name)?;

    // Build packet from file or convenience flags
    let mut packet = if let Some(path) = result_file {
        // Normalize path to prevent traversal outside the project directory
        let canonical =
            std::fs::canonicalize(path).map_err(|e| AdapterError::InvalidResultPacket {
                message: format!("result-file path '{}' could not be resolved: {}", path, e),
            })?;
        let cwd = std::env::current_dir().map_err(|e| AdapterError::InvalidResultPacket {
            message: format!("failed to get working directory: {}", e),
        })?;
        if !canonical.starts_with(&cwd) {
            return Err(AdapterError::InvalidResultPacket {
                message: format!(
                    "result-file path '{}' resolves outside the project directory",
                    path
                ),
            });
        }
        let content = std::fs::read_to_string(&canonical)?;
        serde_yaml::from_str::<CanonicalResultPacket>(&content)?
    } else {
        CanonicalResultPacket {
            adapter_version: crate::response::ADAPTER_VERSION.to_string(),
            profile: profile_name.to_string(),
            actor: actor.clone(),
            task_id: task_id.unwrap_or_default().to_string(),
            graph_revision: revision.unwrap_or(0),
            status: status.unwrap_or_default().to_string(),
            summary: summary.map(|s| s.to_string()),
            reason: reason.map(|r| r.to_string()),
            artifacts: Vec::new(),
            evidence: Vec::new(),
            raw_agent_output_path: None,
        }
    };

    // Enforce consistency between file and CLI flags
    if let Some(cli_task_id) = task_id {
        if !packet.task_id.is_empty() && packet.task_id != cli_task_id {
            return Err(AdapterError::InvalidResultPacket {
                message: format!(
                    "task_id mismatch: result-file has '{}', CLI has '{}'",
                    packet.task_id, cli_task_id
                ),
            });
        }
        if packet.task_id.is_empty() {
            packet.task_id = cli_task_id.to_string();
        }
    }
    if !packet.actor.is_empty() && packet.actor != actor {
        return Err(AdapterError::InvalidResultPacket {
            message: format!(
                "actor mismatch: result-file has '{}', profile has '{}'",
                packet.actor, actor
            ),
        });
    }

    // Validate packet
    packet.validate()?;

    // Enforce revision
    if packet.graph_revision == 0 {
        return Err(AdapterError::InvalidResultPacket {
            message: "graph_revision is required".to_string(),
        });
    }

    // Validate artifact paths and sizes against policy
    let project_root =
        std::env::current_dir().map_err(|e| AdapterError::ArtifactPolicyViolation {
            message: format!("failed to get working directory: {}", e),
        })?;
    let evidence_paths: Vec<Option<String>> = packet
        .evidence
        .iter()
        .map(|e| e.artifact_path.clone())
        .collect();
    crate::artifact::validate_artifacts(
        &packet.artifacts,
        &evidence_paths,
        &packet.raw_agent_output_path,
        &project_root,
        &profile.policies.artifact_policy,
    )?;

    // Check permissions per status
    match packet.status.as_str() {
        "success" if !profile.permissions.allow_submit_success => {
            return Err(AdapterError::ProfilePermissionDenied {
                message: "profile does not have allow_submit_success".to_string(),
            });
        }
        "fail" if !profile.permissions.allow_submit_fail => {
            return Err(AdapterError::ProfilePermissionDenied {
                message: "profile does not have allow_submit_fail".to_string(),
            });
        }
        "blocked" if !profile.permissions.allow_submit_blocked => {
            return Err(AdapterError::ProfilePermissionDenied {
                message: "profile does not have allow_submit_blocked".to_string(),
            });
        }
        "skipped" if !profile.permissions.allow_skip => {
            return Err(AdapterError::ProfilePermissionDenied {
                message: "profile does not have allow_skip".to_string(),
            });
        }
        "cancelled" if !profile.permissions.allow_cancel => {
            return Err(AdapterError::ProfilePermissionDenied {
                message: "profile does not have allow_cancel".to_string(),
            });
        }
        _ => {}
    }

    let client = build_client(&config, &actor)?;

    // Map status to graph mutation command
    let result = match packet.status.as_str() {
        "success" => client.complete(
            &packet.task_id,
            &packet.actor,
            packet.graph_revision,
            packet.summary.as_deref().unwrap_or(""),
        ),
        "fail" => client.fail(
            &packet.task_id,
            &packet.actor,
            packet.graph_revision,
            packet.reason.as_deref().unwrap_or(""),
        ),
        "blocked" => client.block(
            &packet.task_id,
            &packet.actor,
            packet.graph_revision,
            packet.reason.as_deref().unwrap_or(""),
        ),
        "skipped" => client.skip(
            &packet.task_id,
            &packet.actor,
            packet.graph_revision,
            packet.reason.as_deref().unwrap_or(""),
        ),
        "cancelled" => client.cancel(
            &packet.task_id,
            &packet.actor,
            packet.graph_revision,
            packet.reason.as_deref().unwrap_or(""),
        ),
        _ => unreachable!("status validated above"),
    };

    let payload = result?;
    let data = serde_json::json!({
        "node_id": payload.data.node_id,
        "status": payload.data.status,
    });
    let envelope = SuccessEnvelope::new(profile_name, &actor, data);
    response::output_success(&envelope)
}

/// Extend the lease on a claimed task via heartbeat.
fn heartbeat(
    profile_name: &str,
    task_id: &str,
    _revision: u64,
    ttl_seconds: u64,
) -> Result<(), AdapterError> {
    let (config, _profile, actor) = load_profile(profile_name)?;

    let client = build_client(&config, &actor)?;
    let payload = client.heartbeat(task_id, &actor, ttl_seconds)?;

    let data = serde_json::json!({
        "node_id": payload.data.node_id,
        "status": payload.data.status,
        "actor": payload.data.actor,
        "lease_expires_at": payload.data.lease_expires_at,
    });
    let envelope = SuccessEnvelope::new(profile_name, &actor, data);
    response::output_success(&envelope)
}

/// Release a claimed task back to READY.
fn release_work(
    profile_name: &str,
    task_id: &str,
    revision: u64,
    _reason: Option<&str>,
) -> Result<(), AdapterError> {
    let (config, profile, actor) = load_profile(profile_name)?;

    if !profile.permissions.allow_release {
        return Err(AdapterError::ProfilePermissionDenied {
            message: format!(
                "profile '{}' does not have allow_release permission",
                profile_name
            ),
        });
    }

    let client = build_client(&config, &actor)?;
    let payload = client.release(task_id, &actor, revision)?;

    let data = serde_json::json!({
        "node_id": payload.data.task_id,
        "released": payload.data.released,
        "graph_revision": payload.data.graph_revision,
    });
    let envelope = SuccessEnvelope::new(profile_name, &actor, data);
    response::output_success(&envelope)
}

// --- helpers ---

/// Load config and resolve profile + actor.
fn load_profile(
    profile_name: &str,
) -> Result<(AdapterConfig, crate::config::Profile, String), AdapterError> {
    let config_path = config_path()?;
    if !config_path.exists() {
        return Err(AdapterError::ProfileNotFound {
            name: config_path.to_string_lossy().to_string(),
        });
    }
    let content = std::fs::read_to_string(&config_path)?;
    let config: AdapterConfig = serde_yaml::from_str(&content)?;
    config.validate()?;

    let profile = config
        .profiles
        .iter()
        .find(|p| p.name == profile_name)
        .cloned()
        .ok_or_else(|| AdapterError::ProfileNotFound {
            name: profile_name.to_string(),
        })?;

    let actor = resolve_profile_actor(&config, profile_name);
    Ok((config, profile, actor))
}

/// Build a `GraphEngineClient` with a `RealRunner` from config.
fn build_client(config: &AdapterConfig, actor: &str) -> Result<GraphEngineClient, AdapterError> {
    let runner_config = RealRunnerConfig {
        binary_path: config.graph_engine_binary_path.clone(),
        timeout: None,
        env: std::collections::HashMap::new(),
        working_dir: None,
    };
    let runner = RealRunner::new(runner_config);
    let base_dir = std::env::current_dir().map_err(|e| AdapterError::Io {
        message: format!("cannot determine current directory: {}", e),
    })?;
    let logger = AdapterLogger::default_path(&base_dir);
    Ok(GraphEngineClient::with_logger(
        Box::new(runner),
        logger,
        actor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // filesystem-dependent assertions for init-profile, validate-profile,
    // and list-profiles are covered by integration tests (tests/integration.rs)
    // because std::env::current_dir() is process-global and unit tests run
    // in parallel, leading to race conditions on directory state.

    #[test]
    fn resolve_profile_actor_from_default_config() {
        let config = default_config();
        let a = resolve_profile_actor(&config, "read_only_agent");
        assert_eq!(a, "agent_claude_code");

        let a = resolve_profile_actor(&config, "full_exec_agent");
        assert_eq!(a, "agent_openhands");

        let a = resolve_profile_actor(&config, "nonexistent");
        assert_eq!(a, "unknown");
    }
}
