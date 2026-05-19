// PR2: GraphEngineClient not yet wired to CLI; dead_code allowed until PR3 get-work command
#![allow(dead_code)]
use crate::error::AdapterError;
use crate::graph_runner::GraphRunner;
use crate::graph_types::{
    GraphClaimPayload, GraphFailureEnvelope, GraphNextPayload, GraphSuccessEnvelope,
    GraphSummarizePayload, is_graph_failure, parse_graph_failure, parse_graph_success,
};
use crate::logger::AdapterLogger;

/// High-level client that wraps a [`GraphRunner`] to provide typed methods
/// for graph engine interactions: `next`, `claim`, `summarize`.
///
/// All calls go through the trait, so production code uses [`RealRunner`]
/// and tests use [`MockRunner`].
///
/// If a logger is provided, each command is logged as a structured JSONL entry.
///
/// [`RealRunner`]: crate::graph_runner::RealRunner
/// [`MockRunner`]: crate::graph_runner::MockRunner
pub struct GraphEngineClient {
    runner: Box<dyn GraphRunner>,
    logger: Option<AdapterLogger>,
    actor: String,
}

impl GraphEngineClient {
    /// Create a new client with the given runner implementation and no logging.
    pub fn new(runner: Box<dyn GraphRunner>) -> Self {
        Self {
            runner,
            logger: None,
            actor: String::new(),
        }
    }

    /// Create a new client with runner, logger, and actor identity.
    pub fn with_logger(runner: Box<dyn GraphRunner>, logger: AdapterLogger, actor: &str) -> Self {
        Self {
            runner,
            logger: Some(logger),
            actor: actor.to_string(),
        }
    }

    /// Call `graph-engine next` and return the next available task.
    ///
    /// Returns `Ok(GraphSuccessEnvelope<GraphNextPayload>)` when work is available.
    /// Returns `Err(AdapterError::NoWorkAvailable)` when the graph reports no tasks.
    /// Returns other errors for subprocess failures, malformed JSON, etc.
    pub fn next(&self) -> Result<GraphSuccessEnvelope<GraphNextPayload>, AdapterError> {
        let raw = self.runner.execute(&["next"])?;

        if is_graph_failure(&raw) {
            let failure = parse_graph_failure(&raw)?;
            let err = self.normalize_failure(failure, "next");
            self.log_failure("next", &err);
            return Err(err);
        }

        let envelope: GraphSuccessEnvelope<GraphNextPayload> = parse_graph_success(&raw)?;
        self.log_success("next");
        Ok(envelope)
    }

    /// Call `graph-engine claim <task_id> <actor> --revision <rev>` and return the result.
    ///
    /// Returns `Ok(GraphSuccessEnvelope<GraphClaimPayload>)` on successful claim.
    /// Returns appropriate error on failure, including normalizing `STALE_REVISION`
    /// to `AdapterError::ContextStaleRefetchRequired`.
    pub fn claim(
        &self,
        task_id: &str,
        actor: &str,
        revision: u64,
    ) -> Result<GraphSuccessEnvelope<GraphClaimPayload>, AdapterError> {
        let args = ["claim", task_id, actor, "--revision", &revision.to_string()];
        let raw = self.runner.execute(&args)?;

        if is_graph_failure(&raw) {
            let failure = parse_graph_failure(&raw)?;
            let err = self.normalize_failure(failure, "claim");
            self.log_failure("claim", &err);
            return Err(err);
        }

        let envelope: GraphSuccessEnvelope<GraphClaimPayload> = parse_graph_success(&raw)?;
        self.log_success("claim");
        Ok(envelope)
    }

    /// Call `graph-engine summarize <task_id>` and return the result.
    ///
    /// Returns `Ok(GraphSuccessEnvelope<GraphSummarizePayload>)` on success.
    /// Returns appropriate error on failure.
    pub fn summarize(
        &self,
        task_id: &str,
    ) -> Result<GraphSuccessEnvelope<GraphSummarizePayload>, AdapterError> {
        let args = ["summarize", task_id];
        let raw = self.runner.execute(&args)?;

        if is_graph_failure(&raw) {
            let failure = parse_graph_failure(&raw)?;
            let err = self.normalize_failure(failure, "summarize");
            self.log_failure("summarize", &err);
            return Err(err);
        }

        let envelope: GraphSuccessEnvelope<GraphSummarizePayload> = parse_graph_success(&raw)?;
        self.log_success("summarize");
        Ok(envelope)
    }

    /// Log a successful command if logger is present.
    fn log_success(&self, command: &str) {
        if let Some(ref logger) = self.logger {
            let _ = logger.log_success(command, &self.actor);
        }
    }

    /// Log a failed command if logger is present.
    fn log_failure(&self, command: &str, err: &AdapterError) {
        if let Some(ref logger) = self.logger {
            let _ = logger.log_failure(
                command,
                &self.actor,
                &format!("{:?}", err.error_code()),
                &format!("{}", err),
            );
        }
    }

    /// Normalize a graph failure envelope into the appropriate adapter error,
    /// given the command context that produced the failure.
    ///
    /// Maps known graph error codes to adapter-specific errors:
    /// - `NO_WORK_AVAILABLE` → `NoWorkAvailable`
    /// - `STALE_REVISION` → `ContextStaleRefetchRequired`
    /// - Claim unknowns → `ClaimFailed`
    /// - Next/Summarize unknowns → `Unknown { message }`
    fn normalize_failure(&self, failure: GraphFailureEnvelope, command: &str) -> AdapterError {
        match failure.code.as_str() {
            "NO_WORK_AVAILABLE" => AdapterError::NoWorkAvailable,
            "STALE_REVISION" => AdapterError::ContextStaleRefetchRequired {
                message: failure.message,
            },
            _ => match command {
                "claim" => AdapterError::ClaimFailed {
                    message: format!("{}: {}", failure.code, failure.message),
                },
                _ => AdapterError::Unknown {
                    message: format!("{}: {}", failure.code, failure.message),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_runner::MockRunner;

    // --- next() ---

    #[test]
    fn next_returns_task_when_available() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t1","title":"Do it","description":"desc","graph_revision":42,"lease_expiration":"2026-01-01T00:00:00Z","dependencies":[]}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.next().unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.data.task_id, Some("t1".to_string()));
        assert_eq!(result.data.graph_revision, 42);
    }

    #[test]
    fn next_returns_no_work_available() {
        let runner = MockRunner::new(); // default: NO_WORK_AVAILABLE
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.next();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.error_code(),
            crate::error::AdapterErrorCode::NO_WORK_AVAILABLE
        );
    }

    #[test]
    fn next_handles_malformed_json() {
        let mut runner = MockRunner::new();
        runner.set_response("next", "{not json");
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.next();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON
        );
    }

    #[test]
    fn next_propagates_crash() {
        let mut runner = MockRunner::new();
        runner.set_force_crash();
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.next();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_NONZERO_EXIT
        );
    }

    #[test]
    fn next_normalizes_stale_revision() {
        let mut runner = MockRunner::new();
        runner.set_force_stale();
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.next();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::CONTEXT_STALE_REFETCH_REQUIRED
        );
    }

    // --- claim() ---

    #[test]
    fn claim_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "claim t1 claude --revision 7",
            r#"{"status":"success","data":{"claimed":true,"task_id":"t1","actor":"claude","graph_revision":8}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.claim("t1", "claude", 7).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.data.claimed);
        assert_eq!(result.data.graph_revision, 8);
    }

    #[test]
    fn claim_normalizes_stale_revision() {
        let mut runner = MockRunner::new();
        runner.set_force_stale();
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.claim("t1", "claude", 7);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::CONTEXT_STALE_REFETCH_REQUIRED
        );
    }

    #[test]
    fn claim_maps_unknown_failure_to_claim_failed() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "claim t1 claude --revision 7",
            r#"{"status":"failure","code":"ALREADY_CLAIMED","message":"Task t1 is already claimed"}"#,
        );
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.claim("t1", "claude", 7);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::CLAIM_FAILED
        );
    }

    // --- summarize() ---

    #[test]
    fn summarize_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "summarize t1",
            r#"{"status":"success","data":{"task_id":"t1","summary":"task summary","graph_revision":9,"dependencies":[],"recent_events":[]}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.summarize("t1").unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.data.task_id, "t1");
        assert_eq!(result.data.summary, "task summary");
    }

    #[test]
    fn summarize_handles_malformed_json() {
        let mut runner = MockRunner::new();
        runner.set_response("summarize t1", "{bad");
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.summarize("t1");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON
        );
    }

    #[test]
    fn summarize_maps_unknown_failure_to_unknown() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "summarize t1",
            r#"{"status":"failure","code":"NOT_FOUND","message":"task t1 not found"}"#,
        );
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.summarize("t1");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::UNKNOWN_ADAPTER_ERROR
        );
    }

    // --- Strict File Boundary Tests ---
    // The adapter must never depend on reading graph state files directly.
    // It must work correctly using only the GraphRunner trait, even when
    // .agent/task_graph.yaml and .agent/task_events.jsonl are absent.

    #[test]
    fn adapter_works_without_graph_state_files() {
        // Use MockRunner — no filesystem access, no graph state files
        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t42","title":"Test task","description":"desc","graph_revision":1,"lease_expiration":"2026-01-01T00:00:00Z","dependencies":[]}}"#,
        );
        runner.set_response(
            "claim t42 test-agent --revision 1",
            r#"{"status":"success","data":{"claimed":true,"task_id":"t42","actor":"test-agent","graph_revision":2}}"#,
        );
        runner.set_response(
            "summarize t42",
            r#"{"status":"success","data":{"task_id":"t42","summary":"Task summary","graph_revision":2,"dependencies":[],"recent_events":[]}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));

        // All three operations succeed without any filesystem access
        let next_result = client.next().unwrap();
        assert_eq!(next_result.data.task_id, Some("t42".to_string()));

        let claim_result = client.claim("t42", "test-agent", 1).unwrap();
        assert!(claim_result.data.claimed);

        let summarize_result = client.summarize("t42").unwrap();
        assert_eq!(summarize_result.data.task_id, "t42");
    }

    #[test]
    fn claim_with_special_characters_in_args() {
        // Verify that special characters in task IDs and actor names
        // are passed through correctly (no shell interpolation issues)
        let mut runner = MockRunner::new();
        runner.set_response(
            "claim task-1_v2.0 agent-name --revision 42",
            r#"{"status":"success","data":{"claimed":true,"task_id":"task-1_v2.0","actor":"agent-name","graph_revision":43}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.claim("task-1_v2.0", "agent-name", 42);
        assert!(result.is_ok());
    }

    // --- Logging Integration Tests ---

    #[test]
    fn next_success_is_logged() {
        use crate::logger::AdapterLogger;
        let dir = tempfile::tempdir().expect("temp dir");
        let logger = AdapterLogger::new(dir.path().join("test_log.jsonl"));

        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t1","title":"T","description":"D","graph_revision":1,"dependencies":[]}}"#,
        );

        let client = GraphEngineClient::with_logger(Box::new(runner), logger, "test-actor");
        let _ = client.next().unwrap();

        let content = std::fs::read_to_string(dir.path().join("test_log.jsonl")).unwrap();
        let entry: crate::logger::LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry.command, "next");
        assert_eq!(entry.actor, "test-actor");
        assert!(entry.success);
    }

    #[test]
    fn claim_failure_is_logged() {
        use crate::logger::AdapterLogger;
        let dir = tempfile::tempdir().expect("temp dir");
        let logger = AdapterLogger::new(dir.path().join("test_log.jsonl"));

        let mut runner = MockRunner::new();
        runner.set_force_stale();

        let client = GraphEngineClient::with_logger(Box::new(runner), logger, "test-actor");
        let _ = client.claim("t1", "agent", 1); // will fail with STALE_REVISION

        let content = std::fs::read_to_string(dir.path().join("test_log.jsonl")).unwrap();
        let entry: crate::logger::LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry.command, "claim");
        assert!(!entry.success);
        assert!(entry.error_code.is_some());
        assert!(entry.error_message.is_some());
    }
}
