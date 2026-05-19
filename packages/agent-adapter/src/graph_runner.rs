// PR2: RealRunner/RealRunnerConfig/MockRunner not yet wired to CLI; dead_code allowed until PR3
#![allow(dead_code)]
use crate::error::AdapterError;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Trait for abstracting subprocess execution. Separates real subprocess
/// invocations from test doubles so the adapter can be tested without a
/// real graph engine binary.
pub trait GraphRunner: Send + Sync {
    /// Execute the graph engine binary with the given argument array.
    /// Returns the raw stdout on success, or an error describing what went
    /// wrong (nonzero exit, binary not found, malformed JSON, timeout, etc.).
    fn execute(&self, args: &[&str]) -> Result<String, AdapterError>;
}

/// Configuration for [`RealRunner`].
#[derive(Debug, Clone)]
pub struct RealRunnerConfig {
    /// Path to the graph engine binary (e.g., "agent-graph").
    pub binary_path: String,
    /// Optional timeout for subprocess execution. Defaults to 30 seconds.
    pub timeout: Option<Duration>,
    /// Environment variables to inject into the subprocess. Keys are
    /// variable names; values are set on the child process environment
    /// (inheriting the parent environment for unset keys).
    pub env: HashMap<String, String>,
    /// Optional working directory for the subprocess. When `None`,
    /// inherits the current directory.
    pub working_dir: Option<String>,
}

impl Default for RealRunnerConfig {
    fn default() -> Self {
        Self {
            binary_path: "agent-graph".to_string(),
            timeout: Some(Duration::from_secs(30)),
            env: HashMap::new(),
            working_dir: None,
        }
    }
}

/// Runs the graph engine binary via `std::process::Command`.
/// Must never use shell interpolation — arguments are passed as-is.
/// Supports optional timeout and environment injection.
pub struct RealRunner {
    config: RealRunnerConfig,
}

impl RealRunner {
    pub fn new(config: RealRunnerConfig) -> Self {
        Self { config }
    }
}

impl GraphRunner for RealRunner {
    fn execute(&self, args: &[&str]) -> Result<String, AdapterError> {
        let mut cmd = std::process::Command::new(&self.config.binary_path);
        cmd.args(args);

        // Inject environment variables (child inherits parent env, then overrides)
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Set working directory if specified
        if let Some(ref dir) = self.config.working_dir {
            cmd.current_dir(dir);
        }

        let start = Instant::now();
        let timeout = self.config.timeout.unwrap_or(Duration::from_secs(30));

        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AdapterError::GraphEngineUnavailable
                } else {
                    AdapterError::GraphEngineNonzeroExit {
                        message: format!("failed to spawn graph engine: {}", e),
                    }
                }
            })?;

        // Wait with timeout
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = child.wait_with_output().map_err(|_| {
                        AdapterError::GraphEngineNonzeroExit {
                            message: "graph engine process disappeared".to_string(),
                        }
                    })?;

                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

                    if !status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(AdapterError::GraphEngineNonzeroExit {
                            message: format!(
                                "exit code {:?} – stderr: {}",
                                status.code(),
                                stderr.trim()
                            ),
                        });
                    }

                    return Ok(stdout);
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        return Err(AdapterError::GraphEngineNonzeroExit {
                            message: format!("graph engine timed out after {}s", timeout.as_secs()),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(AdapterError::GraphEngineNonzeroExit {
                        message: format!("failed to wait on graph engine: {}", e),
                    });
                }
            }
        }
    }
}

/// Test double that returns pre-programmed responses instead of spawning a
/// real subprocess. Simulates graph engine crashes, malformed JSON,
/// nonzero exits, and `STALE_REVISION`.
pub struct MockRunner {
    responses: HashMap<String, String>,
    force_crash: bool,
    force_malformed: bool,
    force_stale: bool,
    force_timeout: bool,
}

impl MockRunner {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            force_crash: false,
            force_malformed: false,
            force_stale: false,
            force_timeout: false,
        }
    }

    /// Pre-load a response for a given command string.
    pub fn set_response(&mut self, command: &str, response: &str) {
        self.responses
            .insert(command.to_string(), response.to_string());
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

    /// Simulate a subprocess timeout.
    pub fn set_force_timeout(&mut self) {
        self.force_timeout = true;
    }
}

impl GraphRunner for MockRunner {
    fn execute(&self, args: &[&str]) -> Result<String, AdapterError> {
        if self.force_crash {
            return Err(AdapterError::GraphEngineNonzeroExit {
                message: "simulated nonzero exit".to_string(),
            });
        }

        if self.force_timeout {
            return Err(AdapterError::GraphEngineNonzeroExit {
                message: "simulated timeout".to_string(),
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
    fn mock_runner_simulates_timeout() {
        let mut runner = MockRunner::new();
        runner.set_force_timeout();
        let result = runner.execute(&["next"]);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_NONZERO_EXIT
        );
    }

    #[test]
    fn real_runner_config_default() {
        let config = RealRunnerConfig::default();
        assert_eq!(config.binary_path, "agent-graph");
        assert_eq!(config.timeout, Some(Duration::from_secs(30)));
        assert!(config.env.is_empty());
        assert!(config.working_dir.is_none());
    }

    #[test]
    fn real_runner_inherits_trait() {
        let runner = RealRunner::new(RealRunnerConfig::default());
        let _: &dyn GraphRunner = &runner;
    }

    /// RealRunner must pass arguments as-is without shell interpolation.
    /// Creates a temp script that echoes all arguments on one line, then
    /// calls it with `task; exit 99` which contains shell metacharacters.
    /// If RealRunner used shell interpolation, the `; exit 99` would
    /// terminate the process with code 99 or `exit` would be a separate command.
    /// Instead we assert: (1) execute() is Ok, (2) the output is exactly
    /// the literal argument `task; exit 99`.
    #[cfg(unix)]
    #[test]
    fn real_runner_passes_args_without_shell_interpolation() {
        use std::io::Write;

        let dir = tempfile::tempdir().expect("temp dir");
        let script_path = dir.path().join("echo_args.sh");
        let mut script = std::fs::File::create(&script_path).expect("create script");
        writeln!(script, "#!/bin/sh").expect("write shebang");
        writeln!(script, "printf '%s' \"$*\"").expect("write printf");
        drop(script);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }

        let config = RealRunnerConfig {
            binary_path: script_path.to_str().unwrap().to_string(),
            timeout: Some(Duration::from_secs(5)),
            env: HashMap::new(),
            working_dir: None,
        };
        let runner = RealRunner::new(config);

        // This argument contains shell metacharacters.
        // If RealRunner interpolated via shell, `; exit 99` would be a separate
        // command causing a nonzero exit, or the output would be just "task".
        let result = runner.execute(&["task; exit 99"]);
        assert!(result.is_ok(), "Expected OK, got: {:?}", result);
        assert_eq!(result.unwrap().trim(), "task; exit 99");
    }
}
