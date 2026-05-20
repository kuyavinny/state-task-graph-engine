use crate::criteria_context::CriteriaContext;
use crate::error::ControllerError;
use crate::paths::ProjectPaths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Read-only interface to the graph engine (Module 1, `stage` binary).
///
/// **Invariant:** This trait only exposes `status()` and `validate()`.
/// Mutation commands (claim, complete, fail, etc.) are intentionally absent
/// — all mutations must route through [`AdapterClient`](crate::adapter_client::AdapterClient).
pub trait GraphStatusClient: Send + Sync {
    /// Call `stage status` and return normalized `CriteriaContext`.
    ///
    /// Parse the `stage status` JSON envelope, extract graph_revision,
    /// node_count, status counts, and warnings into a `CriteriaContext`.
    fn status(&self,
        paths: &ProjectPaths,
    ) -> Result<CriteriaContext, ControllerError>;

    /// Call `stage validate` and return whether the graph passes validation.
    fn validate(&self,
        paths: &ProjectPaths,
    ) -> Result<GraphValidationResult, ControllerError>;
}

/// Result of a `stage validate` call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphValidationResult {
    pub valid: bool,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Real implementation — subprocess via `stage` binary
// ---------------------------------------------------------------------------

const STAGE_BINARY: &str = "stage";

pub struct RealGraphStatusClient;

impl Default for RealGraphStatusClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RealGraphStatusClient {
    pub fn new() -> Self {
        Self
    }

    /// Resolve the absolute path to the `stage` binary.
    ///
    /// Strategy:
    /// 1. Check workspace `target/debug/stage`
    /// 2. Check `CARGO_TARGET_DIR` env
    /// 3. Try `which stage`
    fn resolve_binary(paths: &ProjectPaths) -> Result<PathBuf, ControllerError> {
        // 1. Workspace target
        let workspace_target = paths.root().join("target/debug").join(STAGE_BINARY);
        if workspace_target.exists() {
            return Ok(workspace_target);
        }
        let workspace_release = paths.root().join("target/release").join(STAGE_BINARY);
        if workspace_release.exists() {
            return Ok(workspace_release);
        }

        // 2. CARGO_TARGET_DIR
        if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
            let cargo_debug = std::path::Path::new(&target_dir)
                .join("debug")
                .join(STAGE_BINARY);
            if cargo_debug.exists() {
                return Ok(cargo_debug);
            }
            let cargo_release = std::path::Path::new(&target_dir)
                .join("release")
                .join(STAGE_BINARY);
            if cargo_release.exists() {
                return Ok(cargo_release);
            }
        }

        // 3. PATH lookup
        if let Some(found) = find_in_path(STAGE_BINARY) {
            return Ok(found);
        }

        Err(ControllerError::BinaryNotFound {
            binary: STAGE_BINARY.to_string(),
        })
    }

    /// Run a `stage` subcommand and capture stdout/stderr.
    fn run_command(
        paths: &ProjectPaths,
        subcommand: &str,
    ) -> Result<serde_json::Value, ControllerError> {
        let binary = Self::resolve_binary(paths)?;

        let output = std::process::Command::new(&binary)
            .arg(subcommand)
            .current_dir(paths.root())
            .output()
            .map_err(|e| ControllerError::GraphClientError {
                command: format!("stage {}", subcommand),
                message: format!("Failed to execute stage: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ControllerError::GraphClientError {
                command: format!("stage {}", subcommand),
                message: if stderr.is_empty() {
                    format!("stage {} exited with code {:?}", subcommand, output.status.code())
                } else {
                    format!("stage {} error: {}", subcommand, stderr.trim())
                },
            });
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ControllerError::GraphClientError {
                command: format!("stage {}", subcommand),
                message: "Empty response from stage".to_string(),
            });
        }

        serde_json::from_str(trimmed).map_err(|e| ControllerError::GraphClientError {
            command: format!("stage {}", subcommand),
            message: format!("Malformed JSON from stage: {}", e),
        })
    }
}

impl GraphStatusClient for RealGraphStatusClient {
    fn status(
        &self,
        paths: &ProjectPaths,
    ) -> Result<CriteriaContext, ControllerError> {
        let json = Self::run_command(paths, "status")?;

        // Extract envelope fields
        let data = json.get("data").ok_or_else(|| ControllerError::GraphClientError {
            command: "stage status".to_string(),
            message: "Missing 'data' field in stage status response".to_string(),
        })?;

        let revision = data
            .get("revision")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let node_count = data
            .get("node_count")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);

        let status_counts: std::collections::HashMap<String, usize> = data
            .get("status")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let warnings: Vec<String> = json
            .get("warnings")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        Ok(CriteriaContext {
            graph_revision: revision,
            node_count,
            status_counts,
            warnings,
        })
    }

    fn validate(
        &self,
        paths: &ProjectPaths,
    ) -> Result<GraphValidationResult, ControllerError> {
        let json = Self::run_command(paths, "validate")?;

        let ok = json.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let warnings: Vec<String> = json
            .get("warnings")
            .and_then(|v| v.as_array().cloned())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(GraphValidationResult {
            valid: ok,
            warnings,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Search PATH for a binary name.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|p| p.exists())
}

// ---------------------------------------------------------------------------
// Mock implementation for tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod mock {
    use super::*;

    pub struct MockGraphStatusClient {
        pub status_result: Result<CriteriaContext, ControllerError>,
        pub validate_result: Result<GraphValidationResult, ControllerError>,
    }

    impl MockGraphStatusClient {
        pub fn new() -> Self {
            Self {
                status_result: Ok(CriteriaContext {
                    graph_revision: 1,
                    node_count: 0,
                    status_counts: std::collections::HashMap::new(),
                    warnings: vec![],
                }),
                validate_result: Ok(GraphValidationResult {
                    valid: true,
                    warnings: vec![],
                }),
            }
        }
    }

    impl GraphStatusClient for MockGraphStatusClient {
        fn status(
            &self,
            _paths: &ProjectPaths,
        ) -> Result<CriteriaContext, ControllerError> {
            self.status_result.clone()
        }

        fn validate(
            &self,
            _paths: &ProjectPaths,
        ) -> Result<GraphValidationResult, ControllerError> {
            self.validate_result.clone()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_status_returns_default() {
        let client = mock::MockGraphStatusClient::new();
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let ctx = client.status(&paths).expect("status");
        assert_eq!(ctx.graph_revision, 1);
        assert_eq!(ctx.node_count, 0);
    }

    #[test]
    fn test_mock_validate_returns_default() {
        let client = mock::MockGraphStatusClient::new();
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let result = client.validate(&paths).expect("validate");
        assert!(result.valid);
    }
}
