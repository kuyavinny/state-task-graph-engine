use crate::error::AdapterError;
use crate::graph_runner::GraphRunner;
use crate::graph_types::{
    parse_graph_failure, parse_graph_success, is_graph_failure, GraphClaimPayload, GraphFailureEnvelope, GraphNextPayload, GraphSuccessEnvelope, GraphSummarizePayload,
};

/// High-level client that wraps a [`GraphRunner`] to provide typed methods
/// for graph engine interactions: `next`, `claim`, `summarize`.
///
/// All calls go through the trait, so production code uses [`RealRunner`]
/// and tests use [`MockRunner`].
///
/// [`RealRunner`]: crate::graph_runner::RealRunner
/// [`MockRunner`]: crate::graph_runner::MockRunner
pub struct GraphEngineClient {
    runner: Box<dyn GraphRunner>,
    /// Working directory passed to the graph engine subprocess.
    /// When `None`, the subprocess inherits the current directory.
    working_dir: Option<String>,
}

impl GraphEngineClient {
    /// Create a new client with the given runner implementation.
    pub fn new(runner: Box<dyn GraphRunner>, working_dir: Option<String>) -> Self {
        Self { runner, working_dir }
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
            return Err(self.normalize_failure(failure));
        }

        let envelope: GraphSuccessEnvelope<GraphNextPayload> = parse_graph_success(&raw)?;
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
        let args = [
            "claim",
            task_id,
            actor,
            "--revision",
            &revision.to_string(),
        ];
        let raw = self.runner.execute(&args)?;

        if is_graph_failure(&raw) {
            let failure = parse_graph_failure(&raw)?;
            return Err(self.normalize_failure(failure));
        }

        let envelope: GraphSuccessEnvelope<GraphClaimPayload> = parse_graph_success(&raw)?;
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
            return Err(self.normalize_failure(failure));
        }

        let envelope: GraphSuccessEnvelope<GraphSummarizePayload> = parse_graph_success(&raw)?;
        Ok(envelope)
    }

    /// Normalize a graph failure envelope into the appropriate adapter error.
    ///
    /// Maps known graph error codes to adapter-specific errors:
    /// - `NO_WORK_AVAILABLE` → `NoWorkAvailable`
    /// - `STALE_REVISION` → `ContextStaleRefetchRequired`
    /// - Other codes → `ClaimFailed` (generic graph-side failure)
    fn normalize_failure(&self, failure: GraphFailureEnvelope) -> AdapterError {
        match failure.code.as_str() {
            "NO_WORK_AVAILABLE" => AdapterError::NoWorkAvailable,
            "STALE_REVISION" => AdapterError::ContextStaleRefetchRequired {
                message: failure.message,
            },
            _ => AdapterError::ClaimFailed {
                message: format!("{}: {}", failure.code, failure.message),
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

        let client = GraphEngineClient::new(Box::new(runner), None);
        let result = client.next().unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.data.task_id, Some("t1".to_string()));
        assert_eq!(result.data.graph_revision, 42);
    }

    #[test]
    fn next_returns_no_work_available() {
        let runner = MockRunner::new(); // default: NO_WORK_AVAILABLE
        let client = GraphEngineClient::new(Box::new(runner), None);
        let result = client.next();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.error_code(), crate::error::AdapterErrorCode::NO_WORK_AVAILABLE);
    }

    #[test]
    fn next_handles_malformed_json() {
        let mut runner = MockRunner::new();
        runner.set_response("next", "{not json");
        let client = GraphEngineClient::new(Box::new(runner), None);
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
        let client = GraphEngineClient::new(Box::new(runner), None);
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
        let client = GraphEngineClient::new(Box::new(runner), None);
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

        let client = GraphEngineClient::new(Box::new(runner), None);
        let result = client.claim("t1", "claude", 7).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.data.claimed);
        assert_eq!(result.data.graph_revision, 8);
    }

    #[test]
    fn claim_normalizes_stale_revision() {
        let mut runner = MockRunner::new();
        runner.set_force_stale();
        let client = GraphEngineClient::new(Box::new(runner), None);
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
        let client = GraphEngineClient::new(Box::new(runner), None);
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

        let client = GraphEngineClient::new(Box::new(runner), None);
        let result = client.summarize("t1").unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.data.task_id, "t1");
        assert_eq!(result.data.summary, "task summary");
    }

    #[test]
    fn summarize_handles_malformed_json() {
        let mut runner = MockRunner::new();
        runner.set_response("summarize t1", "{bad");
        let client = GraphEngineClient::new(Box::new(runner), None);
        let result = client.summarize("t1");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON
        );
    }

    #[test]
    fn summarize_propagates_graph_failure() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "summarize t1",
            r#"{"status":"failure","code":"NOT_FOUND","message":"task t1 not found"}"#,
        );
        let client = GraphEngineClient::new(Box::new(runner), None);
        let result = client.summarize("t1");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::CLAIM_FAILED
        );
    }
}