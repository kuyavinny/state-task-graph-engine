use crate::error::ControllerError;
use crate::paths::ProjectPaths;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Interface to the adapter (Module 2) for all task-related operations.
///
/// **Invariant:** All Module 1 mutations MUST route through this trait.
/// No direct `stage` mutation calls are permitted.
pub trait AdapterClient: Send + Sync {
    /// Get the next available task via `agent-adapter get-work`.
    fn get_work(&self,
        paths: &ProjectPaths,
        profile: &str,
    ) -> Result<TaskPacket, ControllerError>;

    /// Submit a result via `agent-adapter submit-result`.
    fn submit_result(&self,
        paths: &ProjectPaths,
        profile: &str,
        result_file: &Path,
    ) -> Result<SubmitResult, ControllerError>;

    /// Release a task lease via `agent-adapter release-work`.
    fn release_work(
        &self,
        paths: &ProjectPaths,
        profile: &str,
        task_id: &str,
        revision: u64,
        reason: &str,
    ) -> Result<ReleaseResult, ControllerError>;

    /// Render task context via `agent-adapter render-context`.
    fn render_context(
        &self,
        paths: &ProjectPaths,
        profile: &str,
    ) -> Result<RenderResult, ControllerError>;

    /// Validate the adapter profile via `agent-adapter validate-profile`.
    fn validate_profile(
        &self,
        paths: &ProjectPaths,
    ) -> Result<(), ControllerError>;

    /// Heartbeat a leased task via `agent-adapter heartbeat`.
    fn heartbeat(
        &self,
        paths: &ProjectPaths,
        profile: &str,
        task_id: &str,
        revision: u64,
        ttl_seconds: u64,
    ) -> Result<(), ControllerError>;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Task packet returned by `get_work`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskPacket {
    pub task_id: String,
    pub title: String,
    pub description: String,
    pub graph_revision: u64,
    pub instructions: String,
    pub lease_expires_at: Option<String>,
}

/// Result of `submit_result`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubmitResult {
    pub node_id: String,
    pub status: String,
}

/// Result of `release_work`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReleaseResult {
    pub node_id: String,
    pub released: bool,
    pub graph_revision: u64,
}

/// Result of `render_context`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderResult {
    pub format: String,
    pub content: String,
    pub truncated: bool,
}

/// Generic adapter response envelope (agent-adapter JSON format).
#[derive(Debug, Clone, Deserialize)]
struct AdapterResponse {
    ok: bool,
    #[allow(dead_code)]
    adapter_version: Option<String>,
    #[allow(dead_code)]
    profile: Option<String>,
    #[allow(dead_code)]
    actor: Option<String>,
    data: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Real implementation — subprocess via `agent-adapter` binary
// ---------------------------------------------------------------------------

const ADAPTER_BINARY: &str = "agent-adapter";

pub struct RealAdapterClient;

impl Default for RealAdapterClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RealAdapterClient {
    pub fn new() -> Self {
        Self
    }

    fn resolve_binary(paths: &ProjectPaths) -> Result<PathBuf, ControllerError> {
        let _targets = [
            paths.root().join("target/debug").join(ADAPTER_BINARY),
            paths.root().join("target/release").join(ADAPTER_BINARY),
        ];
        for t in ["debug", "release"] {
            let path = paths.root().join("target").join(t).join(ADAPTER_BINARY);
            if path.exists() {
                return Ok(path);
            }
        }
        if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
            for t in ["debug", "release"] {
                let path = std::path::Path::new(&dir).join(t).join(ADAPTER_BINARY);
                if path.exists() {
                    return Ok(path);
                }
            }
        }
        if let Some(found) = super::graph_client::find_in_path(ADAPTER_BINARY) {
            return Ok(found);
        }
        Err(ControllerError::BinaryNotFound {
            binary: ADAPTER_BINARY.to_string(),
        })
    }

    fn run_command(
        paths: &ProjectPaths,
        args: &[&str],
    ) -> Result<AdapterResponse, ControllerError> {
        let binary = Self::resolve_binary(paths)?;

        let mut cmd = std::process::Command::new(&binary);
        for a in args {
            cmd.arg(a);
        }

        let output = cmd
            .current_dir(paths.root())
            .output()
            .map_err(|e| ControllerError::AdapterClientError {
                command: args.first().unwrap_or(&"agent-adapter").to_string(),
                message: format!("Failed to execute agent-adapter: {}", e),
            })?;

        let raw = String::from_utf8_lossy(&output.stdout);
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ControllerError::AdapterClientError {
                command: args.first().unwrap_or(&"").to_string(),
                message: format!(
                    "Empty response from agent-adapter (exit: {:?})",
                    output.status.code()
                ),
            });
        }

        let envelope: AdapterResponse = serde_json::from_str(trimmed)
            .map_err(|e| ControllerError::AdapterClientError {
                command: args.first().unwrap_or(&"").to_string(),
                message: format!("Malformed JSON from agent-adapter: {}", e),
            })?;

        if !envelope.ok {
            let msg = envelope
                .error
                .as_ref()
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown adapter error");
            let code = envelope
                .error
                .as_ref()
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN_ADAPTER_ERROR");
            return Err(ControllerError::AdapterClientError {
                command: args.first().unwrap_or(&"").to_string(),
                message: format!("{}: {}", code, msg),
            });
        }

        Ok(envelope)
    }
}

impl AdapterClient for RealAdapterClient {
    fn get_work(
        &self,
        paths: &ProjectPaths,
        profile: &str,
    ) -> Result<TaskPacket, ControllerError> {
        let envelope = Self::run_command(paths, &[
            "get-work",
            "--profile",
            profile,
        ])?;

        let data = envelope.data.ok_or_else(|| ControllerError::AdapterClientError {
            command: "get-work".to_string(),
            message: "Missing data field in get-work response".to_string(),
        })?;

        // Extract task fields from CanonicalTaskPacket
        let task_id = data
            .get("task")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ControllerError::AdapterClientError {
                command: "get-work".to_string(),
                message: "Missing task.id in get-work response".to_string(),
            })?;

        let title = data
            .get("task")
            .and_then(|t| t.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let description = data
            .get("task")
            .and_then(|t| t.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let graph_revision = data
            .get("graph_revision")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let instructions = data
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let lease_expires_at = data
            .get("task")
            .and_then(|t| t.get("lease_expires_at"))
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(TaskPacket {
            task_id: task_id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            graph_revision,
            instructions: instructions.to_string(),
            lease_expires_at,
        })
    }

    fn submit_result(
        &self,
        paths: &ProjectPaths,
        profile: &str,
        result_file: &Path,
    ) -> Result<SubmitResult, ControllerError> {
        let envelope = Self::run_command(paths, &[
            "submit-result",
            "--profile",
            profile,
            "--result-file",
            &result_file.to_string_lossy(),
        ])?;

        let data = envelope.data.ok_or_else(|| ControllerError::AdapterClientError {
            command: "submit-result".to_string(),
            message: "Missing data field in submit-result response".to_string(),
        })?;

        let node_id = data
            .get("node_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(SubmitResult { node_id, status })
    }

    fn release_work(
        &self,
        paths: &ProjectPaths,
        profile: &str,
        task_id: &str,
        revision: u64,
        reason: &str,
    ) -> Result<ReleaseResult, ControllerError> {
        let envelope = Self::run_command(paths, &[
            "release-work",
            "--profile",
            profile,
            "--task-id",
            task_id,
            "--revision",
            &revision.to_string(),
            "--reason",
            reason,
        ])?;

        let data = envelope.data.ok_or_else(|| ControllerError::AdapterClientError {
            command: "release-work".to_string(),
            message: "Missing data field in release-work response".to_string(),
        })?;

        let node_id = data
            .get("node_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let released = data
            .get("released")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let graph_revision = data
            .get("graph_revision")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        Ok(ReleaseResult {
            node_id,
            released,
            graph_revision,
        })
    }

    fn render_context(
        &self,
        paths: &ProjectPaths,
        profile: &str,
    ) -> Result<RenderResult, ControllerError> {
        let envelope = Self::run_command(paths, &[
            "render-context",
            "--profile",
            profile,
        ])?;

        let data = envelope.data.ok_or_else(|| ControllerError::AdapterClientError {
            command: "render-context".to_string(),
            message: "Missing data field in render-context response".to_string(),
        })?;

        let format = data
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("markdown")
            .to_string();
        let content = data
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let truncated = data
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Ok(RenderResult {
            format,
            content,
            truncated,
        })
    }

    fn validate_profile(
        &self,
        paths: &ProjectPaths,
    ) -> Result<(), ControllerError> {
        let envelope = Self::run_command(paths, &["validate-profile"])?;

        let data = envelope.data.ok_or_else(|| ControllerError::AdapterClientError {
            command: "validate-profile".to_string(),
            message: "Missing data field in validate-profile response".to_string(),
        })?;

        let valid = data
            .get("valid")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !valid {
            return Err(ControllerError::AdapterClientError {
                command: "validate-profile".to_string(),
                message: "Profile validation returned valid=false".to_string(),
            });
        }

        Ok(())
    }

    fn heartbeat(
        &self,
        paths: &ProjectPaths,
        profile: &str,
        task_id: &str,
        revision: u64,
        ttl_seconds: u64,
    ) -> Result<(), ControllerError> {
        let envelope = Self::run_command(paths, &[
            "heartbeat",
            "--task-id",
            task_id,
            "--revision",
            &revision.to_string(),
            "--ttl-seconds",
            &ttl_seconds.to_string(),
            "--profile",
            profile,
        ])?;

        // Success envelope.data may be null or contain fields; presence of ok=true is enough
        let _ = envelope.data;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock implementation for tests
// ---------------------------------------------------------------------------

pub mod mock {
    use super::*;

    pub struct MockAdapterClient {
        pub get_work_result: Result<TaskPacket, ControllerError>,
        pub submit_result_result: Result<SubmitResult, ControllerError>,
        pub release_work_result: Result<ReleaseResult, ControllerError>,
        pub render_context_result: Result<RenderResult, ControllerError>,
        pub validate_profile_result: Result<(), ControllerError>,
        pub heartbeat_result: Result<(), ControllerError>,
    }

    impl Default for MockAdapterClient {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockAdapterClient {
        pub fn new() -> Self {
            Self {
                get_work_result: Ok(TaskPacket {
                    task_id: "task_001".to_string(),
                    title: "Test Task".to_string(),
                    description: "Desc".to_string(),
                    graph_revision: 1,
                    instructions: "Do X".to_string(),
                    lease_expires_at: None,
                }),
                submit_result_result: Ok(SubmitResult {
                    node_id: "node_001".to_string(),
                    status: "COMPLETED".to_string(),
                }),
                release_work_result: Ok(ReleaseResult {
                    node_id: "node_001".to_string(),
                    released: true,
                    graph_revision: 2,
                }),
                render_context_result: Ok(RenderResult {
                    format: "markdown".to_string(),
                    content: "# Context".to_string(),
                    truncated: false,
                }),
                validate_profile_result: Ok(()),
                heartbeat_result: Ok(()),
            }
        }
    }

    impl AdapterClient for MockAdapterClient {
        fn get_work(
            &self,
            _paths: &ProjectPaths,
            _profile: &str,
        ) -> Result<TaskPacket, ControllerError> {
            self.get_work_result.clone()
        }

        fn submit_result(
            &self,
            _paths: &ProjectPaths,
            _profile: &str,
            _result_file: &Path,
        ) -> Result<SubmitResult, ControllerError> {
            self.submit_result_result.clone()
        }

        fn release_work(
            &self,
            _paths: &ProjectPaths,
            _profile: &str,
            _task_id: &str,
            _revision: u64,
            _reason: &str,
        ) -> Result<ReleaseResult, ControllerError> {
            self.release_work_result.clone()
        }

        fn render_context(
            &self,
            _paths: &ProjectPaths,
            _profile: &str,
        ) -> Result<RenderResult, ControllerError> {
            self.render_context_result.clone()
        }

        fn validate_profile(
            &self,
            _paths: &ProjectPaths,
        ) -> Result<(), ControllerError> {
            self.validate_profile_result.clone()
        }

        fn heartbeat(
            &self,
            _paths: &ProjectPaths,
            _profile: &str,
            _task_id: &str,
            _revision: u64,
            _ttl_seconds: u64,
        ) -> Result<(), ControllerError> {
            self.heartbeat_result.clone()
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
    fn test_mock_get_work() {
        let client = mock::MockAdapterClient::new();
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let task = client.get_work(&paths, "default").expect("get_work");
        assert_eq!(task.task_id, "task_001");
        assert_eq!(task.title, "Test Task");
    }

    #[test]
    fn test_mock_validate_profile() {
        let client = mock::MockAdapterClient::new();
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        assert!(client.validate_profile(&paths).is_ok());
    }
}
