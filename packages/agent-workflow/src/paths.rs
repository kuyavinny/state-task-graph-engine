use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Project root discovery
// ---------------------------------------------------------------------------

/// Walk upward from `start_dir` to find a directory containing `.git/`
/// or root `Cargo.toml` with a `[workspace]` section.
fn discover_project_root(start_dir: &std::path::Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        // Check for workspace Cargo.toml
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(contents) = std::fs::read_to_string(&cargo_toml) {
                if contents.contains("[workspace]") {
                    return Some(dir);
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Resolved project paths for the agent-workflow module.
///
/// All paths are relative to the discovered project root.
/// In tests, use `ProjectPaths::from_root(temp_dir)` to override.
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    root: PathBuf,
}

impl ProjectPaths {
    /// Discover project root from the current working directory.
    pub fn discover() -> Result<Self, crate::error::ControllerError> {
        let cwd = std::env::current_dir().map_err(|e| {
            crate::error::ControllerError::UnknownWorkflowError {
                message: format!("Cannot determine current directory: {}", e),
            }
        })?;

        let root = discover_project_root(&cwd).ok_or_else(|| {
            crate::error::ControllerError::UnknownWorkflowError {
                message: "Cannot find project root (no .git/ or workspace Cargo.toml found)".to_string(),
            }
        })?;

        Ok(Self { root })
    }

    /// Create from an explicit root path (for testing).
    pub fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// The project root directory.
    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    /// `.agent/` directory.
    pub fn agent_dir(&self) -> PathBuf {
        self.root.join(".agent")
    }

    /// `.agent/workflows/` directory.
    pub fn workflows_dir(&self) -> PathBuf {
        self.agent_dir().join("workflows")
    }

    /// `.agent/workflows/<workflow_id>.yml` path.
    pub fn workflow_yaml(&self, workflow_id: &str) -> PathBuf {
        self.workflows_dir().join(format!("{}.yml", workflow_id))
    }

    /// `.agent/workflows/<workflow_id>.json` path.
    pub fn workflow_json(&self, workflow_id: &str) -> PathBuf {
        self.workflows_dir().join(format!("{}.json", workflow_id))
    }

    /// `.agent/workflow_runs/` directory.
    pub fn workflow_runs_dir(&self) -> PathBuf {
        self.agent_dir().join("workflow_runs")
    }

    /// `.agent/workflow_runs/<run_id>/` directory.
    pub fn run_dir(&self, run_id: &str) -> PathBuf {
        self.workflow_runs_dir().join(run_id)
    }

    /// `.agent/workflow_runs/<run_id>/run_state.json`.
    pub fn run_state_file(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("run_state.json")
    }

    /// `.agent/workflow_runs/<run_id>/task_packets/`.
    pub fn task_packets_dir(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("task_packets")
    }

    /// `.agent/workflow_runs/<run_id>/result_packets/`.
    pub fn result_packets_dir(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("result_packets")
    }

    /// `.agent/workflow_runs/<run_id>/artifacts/`.
    pub fn artifacts_dir(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("artifacts")
    }

    /// `.agent/workflow_logs.jsonl`.
    pub fn workflow_logs_file(&self) -> PathBuf {
        self.agent_dir().join("workflow_logs.jsonl")
    }
}

// ---------------------------------------------------------------------------
// Path safety validation
// ---------------------------------------------------------------------------

/// Reject IDs containing path traversal or separator characters.
///
/// This prevents directory traversal attacks when constructing paths from
/// user-supplied `workflow_id` or `run_id` values.
pub fn validate_id(id: &str) -> Result<(), crate::error::ControllerError> {
    if id.is_empty() {
        return Err(crate::error::ControllerError::UnknownWorkflowError {
            message: "ID must not be empty".to_string(),
        });
    }

    // Reject absolute paths FIRST, before checking for separators
    if id.starts_with('/') || id.starts_with('\\') {
        return Err(crate::error::ControllerError::UnknownWorkflowError {
            message: format!("ID '{}' must not be an absolute path", id),
        });
    }

    // Reject absolute paths
    if id.starts_with('/') || id.starts_with('\\') {
        return Err(crate::error::ControllerError::UnknownWorkflowError {
            message: format!("ID '{}' must not be an absolute path", id),
        });
    }

    // Reject path separators and traversal patterns
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(crate::error::ControllerError::UnknownWorkflowError {
            message: format!("ID '{}' contains invalid path characters", id),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_paths_from_root() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        assert_eq!(paths.root(), tmp.path());
        assert_eq!(paths.agent_dir(), tmp.path().join(".agent"));
        assert_eq!(paths.workflows_dir(), tmp.path().join(".agent/workflows"));
        assert_eq!(
            paths.workflow_yaml("my_workflow"),
            tmp.path().join(".agent/workflows/my_workflow.yml")
        );
        assert_eq!(
            paths.workflow_json("my_workflow"),
            tmp.path().join(".agent/workflows/my_workflow.json")
        );
        assert_eq!(paths.workflow_runs_dir(), tmp.path().join(".agent/workflow_runs"));
        assert_eq!(
            paths.run_dir("run_123"),
            tmp.path().join(".agent/workflow_runs/run_123")
        );
        assert_eq!(
            paths.run_state_file("run_123"),
            tmp.path().join(".agent/workflow_runs/run_123/run_state.json")
        );
        assert_eq!(
            paths.task_packets_dir("run_123"),
            tmp.path().join(".agent/workflow_runs/run_123/task_packets")
        );
        assert_eq!(
            paths.result_packets_dir("run_123"),
            tmp.path().join(".agent/workflow_runs/run_123/result_packets")
        );
        assert_eq!(
            paths.artifacts_dir("run_123"),
            tmp.path().join(".agent/workflow_runs/run_123/artifacts")
        );
        assert_eq!(
            paths.workflow_logs_file(),
            tmp.path().join(".agent/workflow_logs.jsonl")
        );
    }

    #[test]
    fn test_validate_id_rejects_traversal() {
        assert!(validate_id("../etc/passwd").is_err());
        assert!(validate_id("foo/../../bar").is_err());
        assert!(validate_id("foo\\bar").is_err());
        assert!(validate_id("").is_err());
        assert!(validate_id("/absolute").is_err());
    }

    #[test]
    fn test_validate_id_accepts_valid() {
        assert!(validate_id("api_deployment_v1").is_ok());
        assert!(validate_id("run_2026-05-20T14-30-00_abc123").is_ok());
        assert!(validate_id("my-workflow").is_ok());
    }
}