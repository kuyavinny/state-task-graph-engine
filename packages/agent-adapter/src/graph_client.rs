// PR2-3: GraphEngineClient used by get-work command; dead_code suppressed for test-only
#![allow(dead_code)]
use crate::error::AdapterError;
use crate::graph_runner::GraphRunner;
use crate::graph_types::{
    GraphClaimPayload, GraphFailureEnvelope, GraphHeartbeatPayload, GraphMutationPayload,
    GraphNextPayload, GraphReleasePayload, GraphSuccessEnvelope, GraphSummarizePayload,
    is_graph_failure, parse_graph_failure, parse_graph_success,
};
use crate::logger::AdapterLogger;
use crate::task_packet::{
    BoundedContext, CanonicalTaskPacket, Constraints, DependencyInfo, HeartbeatRequirements,
    TaskInfo,
};

/// High-level client that wraps a [`GraphRunner`] to provide typed methods
/// for graph engine interactions: `next`, `claim`, `summarize`, `release`.
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
        match self.runner.execute(&["next"]) {
            Ok(raw) => {
                if is_graph_failure(&raw) {
                    let failure = parse_graph_failure(&raw)?;
                    let err = self.normalize_failure(failure, "next");
                    self.log_failure("next", &err);
                    return Err(err);
                }

                match parse_graph_success::<GraphNextPayload>(&raw) {
                    Ok(envelope) => {
                        self.log_success("next");
                        Ok(envelope)
                    }
                    Err(e) => {
                        self.log_failure("next", &e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.log_failure("next", &e);
                Err(e)
            }
        }
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
        match self.runner.execute(&args) {
            Ok(raw) => {
                if is_graph_failure(&raw) {
                    let failure = parse_graph_failure(&raw)?;
                    let err = self.normalize_failure(failure, "claim");
                    self.log_failure("claim", &err);
                    return Err(err);
                }

                match parse_graph_success::<GraphClaimPayload>(&raw) {
                    Ok(envelope) => {
                        self.log_success("claim");
                        Ok(envelope)
                    }
                    Err(e) => {
                        self.log_failure("claim", &e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.log_failure("claim", &e);
                Err(e)
            }
        }
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
        match self.runner.execute(&args) {
            Ok(raw) => {
                if is_graph_failure(&raw) {
                    let failure = parse_graph_failure(&raw)?;
                    let err = self.normalize_failure(failure, "summarize");
                    self.log_failure("summarize", &err);
                    return Err(err);
                }

                match parse_graph_success::<GraphSummarizePayload>(&raw) {
                    Ok(envelope) => {
                        self.log_success("summarize");
                        Ok(envelope)
                    }
                    Err(e) => {
                        self.log_failure("summarize", &e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.log_failure("summarize", &e);
                Err(e)
            }
        }
    }

    /// Call `graph-engine release <task_id> <actor>` and return the result.
    ///
    /// **Note:** The spec prescribes `--revision`, but the current `agent-graph` CLI
    /// does not accept it for `release`.  The argument is kept in the signature
    /// for forward-compatibility but is intentionally **not** forwarded to the
    /// subprocess until the graph engine adds support.
    pub fn release(
        &self,
        task_id: &str,
        actor: &str,
        _revision: u64,
    ) -> Result<GraphSuccessEnvelope<GraphReleasePayload>, AdapterError> {
        let args = ["release", task_id, actor];
        match self.runner.execute(&args) {
            Ok(raw) => {
                if is_graph_failure(&raw) {
                    let failure = parse_graph_failure(&raw)?;
                    let err = self.normalize_failure(failure, "release");
                    self.log_failure("release", &err);
                    return Err(err);
                }

                match parse_graph_success::<GraphReleasePayload>(&raw) {
                    Ok(envelope) => {
                        self.log_success("release");
                        Ok(envelope)
                    }
                    Err(e) => {
                        self.log_failure("release", &e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.log_failure("release", &e);
                Err(e)
            }
        }
    }

    /// Call `graph-engine heartbeat <task_id> <actor> --ttl-seconds <ttl>`.
    pub fn heartbeat(
        &self,
        task_id: &str,
        actor: &str,
        ttl_seconds: u64,
    ) -> Result<GraphSuccessEnvelope<GraphHeartbeatPayload>, AdapterError> {
        let args = [
            "heartbeat",
            task_id,
            actor,
            "--ttl-seconds",
            &ttl_seconds.to_string(),
        ];
        match self.runner.execute(&args) {
            Ok(raw) => {
                if is_graph_failure(&raw) {
                    let failure = parse_graph_failure(&raw)?;
                    let err = self.normalize_failure(failure, "heartbeat");
                    self.log_failure("heartbeat", &err);
                    return Err(err);
                }

                match parse_graph_success::<GraphHeartbeatPayload>(&raw) {
                    Ok(envelope) => {
                        self.log_success("heartbeat");
                        Ok(envelope)
                    }
                    Err(e) => {
                        self.log_failure("heartbeat", &e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.log_failure("heartbeat", &e);
                Err(e)
            }
        }
    }

    /// Call `graph-engine complete <task_id> <actor> --revision <rev> --result-summary <txt>`.
    pub fn complete(
        &self,
        task_id: &str,
        actor: &str,
        revision: u64,
        summary: &str,
    ) -> Result<GraphSuccessEnvelope<GraphMutationPayload>, AdapterError> {
        let args = [
            "complete",
            task_id,
            actor,
            "--revision",
            &revision.to_string(),
            "--result-summary",
            summary,
        ];
        self.execute_mutation("complete", &args)
    }

    /// Call `graph-engine fail <task_id> <actor> --revision <rev> --failure-reason <txt>`.
    pub fn fail(
        &self,
        task_id: &str,
        actor: &str,
        revision: u64,
        reason: &str,
    ) -> Result<GraphSuccessEnvelope<GraphMutationPayload>, AdapterError> {
        let args = [
            "fail",
            task_id,
            actor,
            "--revision",
            &revision.to_string(),
            "--failure-reason",
            reason,
        ];
        self.execute_mutation("fail", &args)
    }

    /// Call `graph-engine block <task_id> <actor> --revision <rev> --blocked-reason <txt>`.
    pub fn block(
        &self,
        task_id: &str,
        actor: &str,
        revision: u64,
        reason: &str,
    ) -> Result<GraphSuccessEnvelope<GraphMutationPayload>, AdapterError> {
        let args = [
            "block",
            task_id,
            actor,
            "--revision",
            &revision.to_string(),
            "--blocked-reason",
            reason,
        ];
        self.execute_mutation("block", &args)
    }

    /// Call `graph-engine skip <task_id> <actor> --revision <rev> --skip-reason <txt>`.
    pub fn skip(
        &self,
        task_id: &str,
        actor: &str,
        revision: u64,
        reason: &str,
    ) -> Result<GraphSuccessEnvelope<GraphMutationPayload>, AdapterError> {
        let args = [
            "skip",
            task_id,
            actor,
            "--revision",
            &revision.to_string(),
            "--skip-reason",
            reason,
        ];
        self.execute_mutation("skip", &args)
    }

    /// Call `graph-engine cancel <task_id> <actor> --revision <rev> --cancel-reason <txt>`.
    pub fn cancel(
        &self,
        task_id: &str,
        actor: &str,
        revision: u64,
        reason: &str,
    ) -> Result<GraphSuccessEnvelope<GraphMutationPayload>, AdapterError> {
        let args = [
            "cancel",
            task_id,
            actor,
            "--revision",
            &revision.to_string(),
            "--cancel-reason",
            reason,
        ];
        self.execute_mutation("cancel", &args)
    }

    /// Generic mutation executor that handles success/failure/logging for all mutation commands.
    fn execute_mutation(
        &self,
        command: &str,
        args: &[&str],
    ) -> Result<GraphSuccessEnvelope<GraphMutationPayload>, AdapterError> {
        match self.runner.execute(args) {
            Ok(raw) => {
                if is_graph_failure(&raw) {
                    let failure = parse_graph_failure(&raw)?;
                    let err = self.normalize_failure(failure, command);
                    self.log_failure(command, &err);
                    return Err(err);
                }

                match parse_graph_success::<GraphMutationPayload>(&raw) {
                    Ok(envelope) => {
                        self.log_success(command);
                        Ok(envelope)
                    }
                    Err(e) => {
                        self.log_failure(command, &e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.log_failure(command, &e);
                Err(e)
            }
        }
    }

    /// Orchestrate the full `get-work` composition: next → claim → summarize.
    ///
    /// Returns a [`CanonicalTaskPacket`] containing the post-claim revision,
    /// task metadata, bounded context, and constraints.
    ///
    /// Composite failure behaviour:
    /// * If `claim` succeeds but `summarize` fails, a best-effort `release` is
    ///   attempted **only** when a valid post-claim revision was returned.
    /// * If release also fails (or no revision exists), the error is
    ///   [`AdapterError::SummarizeFailedAfterClaim`] with
    ///   [`AdapterErrorCode::TASK_MAY_REMAIN_LEASED`] in the details.
    pub fn get_work(&self, actor: &str) -> Result<CanonicalTaskPacket, AdapterError> {
        // 1. Discover next available task
        let next_env = self.next()?;
        let next_data = next_env.data;

        let task_id = match next_data.task_id {
            Some(id) => id,
            None => return Err(AdapterError::NoWorkAvailable),
        };
        let pre_claim_revision = next_data.graph_revision;

        // 2. Claim the task
        let claim_env = self.claim(&task_id, actor, pre_claim_revision)?;
        let claim_data = claim_env.data;
        let post_claim_revision = claim_data.graph_revision;

        // 3. Summarize for bounded context
        let summarize_result = self.summarize(&task_id);
        let summarize_data = match summarize_result {
            Ok(env) => env.data,
            Err(_summarize_err) => {
                // Best-effort release only when we have a valid post-claim revision
                if post_claim_revision > 0 {
                    let _ = self.release(&task_id, actor, post_claim_revision);
                }
                return Err(AdapterError::SummarizeFailedAfterClaim {
                    message: format!(
                        "summarize failed after successful claim for task {} — task may remain leased",
                        task_id
                    ),
                });
            }
        };

        // 4. Assemble canonical task packet
        let packet = CanonicalTaskPacket {
            adapter_version: crate::response::ADAPTER_VERSION.to_string(),
            profile: String::new(), // filled by CLI layer
            actor: actor.to_string(),
            graph_revision: post_claim_revision,
            task: TaskInfo {
                id: task_id.clone(),
                title: next_data.title.unwrap_or_else(|| task_id.clone()),
                description: next_data.description.unwrap_or_default(),
                status: "IN_PROGRESS".to_string(),
                lease_expires_at: claim_data.lease_expiration.clone(),
            },
            bounded_context: BoundedContext {
                parent_chain: Vec::new(),
                immediate_dependencies: summarize_data
                    .dependencies
                    .iter()
                    .map(|dep| {
                        // Each dep is a serde_json::Value; attempt to extract id/status
                        let id = dep
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let status = dep
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("UNKNOWN")
                            .to_string();
                        DependencyInfo { id, status }
                    })
                    .collect(),
                dependent_tasks: Vec::new(),
                recent_events: summarize_data.recent_events.clone(),
                completed_summaries: Vec::new(),
            },
            instructions: summarize_data.summary.clone(),
            reporting_requirements: vec!["summary".to_string()],
            heartbeat_requirements: HeartbeatRequirements {
                interval_seconds: 300,
            },
            constraints: Constraints {
                read_files: true,
                write_files: true,
                execute_shell: true,
                run_tests: false,
                network_access: false,
                browser_access: false,
            },
        };

        Ok(packet)
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
    /// - Next/Summarize/Release unknowns → `GraphEngineFailure`
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
                _ => AdapterError::GraphEngineFailure {
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
    fn summarize_maps_unknown_failure_to_graph_engine_failure() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "summarize t1",
            r#"{"status":"failure","code":"NOT_FOUND","message":"task t1 not found"}"#,
        );
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.summarize("t1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_FAILURE
        );
        assert_eq!(err.source_tag(), crate::error::ErrorSource::GraphEngine);
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

    #[test]
    fn crash_is_logged() {
        use crate::logger::AdapterLogger;
        let dir = tempfile::tempdir().expect("temp dir");
        let logger = AdapterLogger::new(dir.path().join("test_log.jsonl"));

        let mut runner = MockRunner::new();
        runner.set_force_crash();

        let client = GraphEngineClient::with_logger(Box::new(runner), logger, "test-actor");
        let _ = client.next(); // will fail with GRAPH_ENGINE_NONZERO_EXIT

        let content = std::fs::read_to_string(dir.path().join("test_log.jsonl")).unwrap();
        let entry: crate::logger::LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry.command, "next");
        assert!(!entry.success);
        assert_eq!(
            entry.error_code.as_deref(),
            Some("GRAPH_ENGINE_NONZERO_EXIT")
        );
    }

    #[test]
    fn malformed_json_is_now_logged() {
        // PR3 fix: malformed JSON in the Ok(raw) branch is now logged.
        use crate::logger::AdapterLogger;
        let dir = tempfile::tempdir().expect("temp dir");
        let logger = AdapterLogger::new(dir.path().join("test_log.jsonl"));

        let mut runner = MockRunner::new();
        runner.set_response("next", "{not json");

        let client = GraphEngineClient::with_logger(Box::new(runner), logger, "test-actor");
        let result = client.next();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::GRAPH_ENGINE_MALFORMED_JSON
        );

        let content = std::fs::read_to_string(dir.path().join("test_log.jsonl")).unwrap();
        let entry: crate::logger::LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry.command, "next");
        assert!(!entry.success);
        assert_eq!(
            entry.error_code.as_deref(),
            Some("GRAPH_ENGINE_MALFORMED_JSON")
        );
    }

    // --- release() ---

    #[test]
    fn release_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "release t1 test-agent",
            r#"{"status":"success","data":{"released":true,"task_id":"t1","graph_revision":44}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.release("t1", "test-agent", 43).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.data.released);
        assert_eq!(result.data.graph_revision, 44);
    }

    #[test]
    fn get_work_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t1","title":"Do it","description":"desc","graph_revision":42,"lease_expiration":"2026-01-01T00:00:00Z","dependencies":[]}}"#,
        );
        runner.set_response(
            "claim t1 test-agent --revision 42",
            r#"{"status":"success","data":{"claimed":true,"task_id":"t1","actor":"test-agent","graph_revision":43}}"#,
        );
        runner.set_response(
            "summarize t1",
            r#"{"status":"success","data":{"task_id":"t1","summary":"Task summary","graph_revision":43,"dependencies":[{"id":"d1","status":"COMPLETED"}],"recent_events":[]}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let packet = client.get_work("test-agent").unwrap();

        assert_eq!(packet.actor, "test-agent");
        assert_eq!(packet.graph_revision, 43); // post-claim revision
        assert_eq!(packet.task.id, "t1");
        assert_eq!(packet.task.status, "IN_PROGRESS");
        assert_eq!(packet.bounded_context.immediate_dependencies.len(), 1);
        assert_eq!(packet.bounded_context.immediate_dependencies[0].id, "d1");
    }

    #[test]
    fn get_work_no_task_available() {
        let runner = MockRunner::new(); // default: NO_WORK_AVAILABLE
        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.get_work("test-agent");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::NO_WORK_AVAILABLE
        );
    }

    #[test]
    fn get_work_claim_fails_after_next() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t1","title":"Do it","description":"desc","graph_revision":42,"dependencies":[]}}"#,
        );
        runner.set_response(
            "claim t1 test-agent --revision 42",
            r#"{"status":"failure","code":"ALREADY_CLAIMED","message":"task t1 is already claimed"}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.get_work("test-agent");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().error_code(),
            crate::error::AdapterErrorCode::CLAIM_FAILED
        );
    }

    #[test]
    fn get_work_summarize_fails_release_attempted() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t1","title":"Do it","description":"desc","graph_revision":42,"dependencies":[]}}"#,
        );
        runner.set_response(
            "claim t1 test-agent --revision 42",
            r#"{"status":"success","data":{"claimed":true,"task_id":"t1","actor":"test-agent","graph_revision":43}}"#,
        );
        runner.set_response(
            "summarize t1",
            r#"{"status":"failure","code":"NOT_FOUND","message":"task t1 not found"}"#,
        );
        runner.set_response(
            "release t1 test-agent",
            r#"{"status":"success","data":{"released":true,"task_id":"t1","graph_revision":44}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.get_work("test-agent");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.error_code(),
            crate::error::AdapterErrorCode::SUMMARIZE_FAILED_AFTER_CLAIM
        );
    }

    #[test]
    fn get_work_summarize_fails_no_post_claim_revision() {
        // If claim returns graph_revision == 0, release should NOT be attempted
        let mut runner = MockRunner::new();
        runner.set_response(
            "next",
            r#"{"status":"success","data":{"task_id":"t1","title":"Do it","description":"desc","graph_revision":42,"dependencies":[]}}"#,
        );
        runner.set_response(
            "claim t1 test-agent --revision 42",
            r#"{"status":"success","data":{"claimed":true,"task_id":"t1","actor":"test-agent","graph_revision":0}}"#,
        );
        runner.set_response(
            "summarize t1",
            r#"{"status":"failure","code":"NOT_FOUND","message":"task t1 not found"}"#,
        );
        // No release response set — if release were called, MockRunner would return
        // NO_WORK_AVAILABLE (its default), causing a different error path.

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.get_work("test-agent");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.error_code(),
            crate::error::AdapterErrorCode::SUMMARIZE_FAILED_AFTER_CLAIM
        );
    }

    #[test]
    fn complete_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "complete t1 agent --revision 5 --result-summary Done",
            r#"{"status":"success","data":{"node_id":"t1","status":"COMPLETED"}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.complete("t1", "agent", 5, "Done");
        assert!(result.is_ok());
        let env = result.unwrap();
        assert_eq!(env.data.node_id, "t1");
        assert_eq!(env.data.status, "COMPLETED");
    }

    #[test]
    fn fail_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "fail t1 agent --revision 5 --failure-reason broke",
            r#"{"status":"success","data":{"node_id":"t1","status":"FAILED"}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.fail("t1", "agent", 5, "broke");
        assert!(result.is_ok());
        let env = result.unwrap();
        assert_eq!(env.data.status, "FAILED");
    }

    #[test]
    fn block_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "block t1 agent --revision 5 --blocked-reason waiting",
            r#"{"status":"success","data":{"node_id":"t1","status":"BLOCKED"}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.block("t1", "agent", 5, "waiting");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.status, "BLOCKED");
    }

    #[test]
    fn skip_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "skip t1 agent --revision 5 --skip-reason obsolete",
            r#"{"status":"success","data":{"node_id":"t1","status":"SKIPPED"}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.skip("t1", "agent", 5, "obsolete");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.status, "SKIPPED");
    }

    #[test]
    fn cancel_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "cancel t1 agent --revision 5 --cancel-reason user",
            r#"{"status":"success","data":{"node_id":"t1","status":"CANCELLED"}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.cancel("t1", "agent", 5, "user");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.status, "CANCELLED");
    }

    #[test]
    fn heartbeat_returns_success() {
        let mut runner = MockRunner::new();
        runner.set_response(
            "heartbeat t1 agent --ttl-seconds 600",
            r#"{"status":"success","data":{"node_id":"t1","status":"IN_PROGRESS","actor":"agent","lease_expires_at":"2026-12-31T23:59:59Z"}}"#,
        );

        let client = GraphEngineClient::new(Box::new(runner));
        let result = client.heartbeat("t1", "agent", 600);
        assert!(result.is_ok());
        let env = result.unwrap();
        assert_eq!(env.data.node_id, "t1");
        assert_eq!(env.data.status, "IN_PROGRESS");
        assert_eq!(
            env.data.lease_expires_at,
            Some("2026-12-31T23:59:59Z".to_string())
        );
    }
}
