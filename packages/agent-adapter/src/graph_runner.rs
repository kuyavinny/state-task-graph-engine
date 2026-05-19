use crate::error::AdapterError;

/// Trait for abstracting subprocess execution. Separates real subprocess
/// invocations from test doubles so the adapter can be tested without a
/// real graph engine binary.
#[allow(dead_code)] // Will be used by GraphEngineClient in next commit
pub trait GraphRunner: Send + Sync {
    /// Execute the graph engine binary with the given argument array.
    /// Returns the raw stdout on success, or an error describing what went
    /// wrong (nonzero exit, binary not found, malformed JSON, etc.).
    fn execute(&self,
        args: &[&str],
    ) -> Result<String, AdapterError>;
}

/// Runs the graph engine binary via `std::process::Command`.
/// Must never use shell interpolation — arguments are passed as-is.
#[allow(dead_code)] // Will be used by GraphEngineClient in next commit
pub struct RealRunner {
    binary_path: String,
}

impl RealRunner {
    pub fn new(binary_path: String) -> Self {
        Self { binary_path }
    }
}

impl GraphRunner for RealRunner {
    fn execute(&self, args: &[&str]) -> Result<String, AdapterError> {
        let output = std::process::Command::new(&self.binary_path)
            .args(args)
            .output()
            .map_err(|_| AdapterError::GraphEngineUnavailable)?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AdapterError::GraphEngineNonzeroExit {
                message: format!(
                    "exit code {:?} – stderr: {}",
                    output.status.code(),
                    stderr.trim()
                ),
            });
        }

        Ok(stdout)
    }
}

/// Test double that returns pre-programmed responses instead of spawning a
/// real subprocess. Simulates graph engine crashes, malformed JSON,
/// nonzero exits, and `STALE_REVISION`.
#[allow(dead_code)] // Will be used by GraphEngineClient in next commit
pub struct MockRunner {
    responses: std::collections::HashMap<String, String>,
    force_crash: bool,
    force_malformed: bool,
    force_stale: bool,
}

impl MockRunner {
    pub fn new() -> Self {
        Self {
            responses: std::collections::HashMap::new(),
            force_crash: false,
            force_malformed: false,
            force_stale: false,
        }
    }

    /// Pre-load a response for a given command string.
    pub fn set_response(&mut self, command: &str, response: &str) {
        self.responses.insert(command.to_string(), response.to_string());
    }

    /// Simulate a graph engine crash (nonzero exit).
    pub fn set_force_crash(&mut self) {
        self.force_crash = true;
    }

    /// Simulate a malformed JSON response.
    pub fn set_force_malformed(&mut self) {
        self.force_malformed = true;
    }

    /// Simulate a `STALE_REVISION` graph response.
    pub fn set_force_stale(&mut self) {
        self.force_stale = true;
    }
}

impl GraphRunner for MockRunner {
    fn execute(&self, args: &[&str]) -> Result<String, AdapterError> {
        if self.force_crash {
            return Err(AdapterError::GraphEngineNonzeroExit {
                message: "simulated nonzero exit".to_string(),
            });
        }

        let command = args.join(" ");
        let mut body = self.responses.get(&command).cloned().unwrap_or_else(|| {
            // Default: simulate "no work available"
            serde_json::json!({
                "status": "failure",
                "code": "NO_WORK_AVAILABLE",
                "message": "No tasks available",
            })
            .to_string()
        });

        if self.force_malformed {
            body = "{not json".to_string();
        }

        if self.force_stale {
            body = serde_json::json!({
                "status": "failure",
                "code": "STALE_REVISION",
                "message": "revision mismatch",
            })
            .to_string();
        }

        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_runner_returns_programmed_response() {
        let mut runner = MockRunner::new();
        runner.set_response("next", r#"{"status":"success","data":{"task_id":"t1"}}"#);

        let result = runner.execute(&["next"]);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("t1"));
    }

    #[test]
    fn mock_runner_returns_default_no_work() {
        let runner = MockRunner::new();
        let result = runner.execute(&["next"]);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("NO_WORK_AVAILABLE"));
    }

    #[test]
    fn mock_runner_simulates_crash() {
        let mut runner = MockRunner::new();
        runner.set_force_crash();
        let result = runner.execute(&["next"]);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_NONZERO_EXIT
        );
    }

    #[test]
    fn mock_runner_simulates_malformed_json() {
        let mut runner = MockRunner::new();
        runner.set_force_malformed();
        let result = runner.execute(&["next"]);
        assert!(result.is_ok());
        let raw = result.unwrap();
        // Raw response is malformed; parsing would fail downstream
        assert!(raw.starts_with("{not json"));
    }

    #[test]
    fn mock_runner_simulates_stale_revision() {
        let mut runner = MockRunner::new();
        runner.set_force_stale();
        let result = runner.execute(&["claim", "t1", "actor"]);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("STALE_REVISION"));
    }

    #[test]
    fn real_runner_is_not_tested_without_binary() {
        // RealRunner requires the agent-graph binary to exist.
        // We create the struct without executing it in unit tests.
        let runner = RealRunner::new("/nonexistent/bin".to_string());
        // Just verify it compiles and has the trait.
        let _: &dyn GraphRunner = &runner;
    }
}
