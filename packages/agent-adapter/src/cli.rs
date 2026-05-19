use crate::config::{AdapterConfig, default_config};
use crate::error::AdapterError;
use crate::response::{self, SuccessEnvelope};

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
