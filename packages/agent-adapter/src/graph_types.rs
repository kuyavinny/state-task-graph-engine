use serde::{Deserialize, Serialize};

/// Typed graph engine success envelope with a generic payload.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphSuccessEnvelope<T> {
    pub status: String,
    pub data: T,
}

/// Typed graph engine failure envelope.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphFailureEnvelope {
    pub status: String,
    pub code: String,
    pub message: String,
}

/// Payload for the `graph-engine next` command.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphNextPayload {
    pub task_id: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub graph_revision: u64,
    #[serde(default)]
    pub lease_expiration: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Payload for the `graph-engine claim` command.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphClaimPayload {
    pub claimed: bool,
    pub task_id: String,
    pub actor: String,
    pub graph_revision: u64,
    #[serde(default)]
    pub lease_expiration: Option<String>,
}

/// Payload for the `graph-engine summarize` command.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphSummarizePayload {
    pub task_id: String,
    pub summary: String,
    pub graph_revision: u64,
    #[serde(default)]
    pub dependencies: Vec<serde_json::Value>,
    #[serde(default)]
    pub recent_events: Vec<serde_json::Value>,
}

/// Payload for the `graph-engine release` command.
#[allow(dead_code)] // PR2-3: used via GraphEngineClient, not yet wired to CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphReleasePayload {
    pub released: bool,
    pub task_id: String,
    pub graph_revision: u64,
}

/// Deserialize a raw JSON string into a typed graph success envelope.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
pub fn parse_graph_success<T: serde::de::DeserializeOwned>(
    raw: &str,
) -> Result<GraphSuccessEnvelope<T>, crate::error::AdapterError> {
    serde_json::from_str(raw).map_err(|e| crate::error::AdapterError::GraphEngineMalformedJson {
        message: format!("{}", e),
    })
}

/// Deserialize a raw JSON string into a typed graph failure envelope.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
pub fn parse_graph_failure(raw: &str) -> Result<GraphFailureEnvelope, crate::error::AdapterError> {
    serde_json::from_str(raw).map_err(|e| crate::error::AdapterError::GraphEngineMalformedJson {
        message: format!("{}", e),
    })
}

/// Determine whether a raw JSON string represents a graph failure.
#[allow(dead_code)] // PR2: used via GraphEngineClient, not yet wired to CLI
pub fn is_graph_failure(raw: &str) -> bool {
    // Try to parse as JSON and look for status == "failure"
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| {
            v.get("status")
                .and_then(|s| s.as_str())
                .map(|s| s == "failure")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_success_envelope() {
        let raw = r#"{"status":"success","data":{"task_id":"t1","graph_revision":42}}"#;
        let env = parse_graph_success::<serde_json::Value>(raw).unwrap();
        assert_eq!(env.status, "success");
        assert_eq!(env.data["task_id"], "t1");
    }

    #[test]
    fn parse_failure_envelope() {
        let raw = r#"{"status":"failure","code":"STALE_REVISION","message":"rev mismatch"}"#;
        let env = parse_graph_failure(raw).unwrap();
        assert_eq!(env.status, "failure");
        assert_eq!(env.code, "STALE_REVISION");
    }

    #[test]
    fn malformed_json_returns_error() {
        let raw = "{not json";
        let result = parse_graph_success::<serde_json::Value>(raw);
        assert!(result.is_err());
    }

    #[test]
    fn detect_failure_status() {
        assert!(is_graph_failure(r#"{"status":"failure","code":"X"}"#));
        assert!(!is_graph_failure(r#"{"status":"success","data":{}}"#));
        assert!(!is_graph_failure("not json at all"));
    }

    #[test]
    fn next_payload_fields() {
        let raw = r#"{"task_id":"t1","title":"Do it","description":"Desc","graph_revision":3,"lease_expiration":"2026-01-01T00:00:00Z","dependencies":["d1"]}"#;
        let p: GraphNextPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(p.task_id, Some("t1".to_string()));
        assert_eq!(p.title, Some("Do it".to_string()));
        assert_eq!(p.graph_revision, 3);
    }

    #[test]
    fn claim_payload_fields() {
        let raw = r#"{"claimed":true,"task_id":"t1","actor":"claude","graph_revision":7}"#;
        let p: GraphClaimPayload = serde_json::from_str(raw).unwrap();
        assert!(p.claimed);
        assert_eq!(p.task_id, "t1");
        assert_eq!(p.graph_revision, 7);
    }

    #[test]
    fn release_payload_fields() {
        let raw = r#"{"released":true,"task_id":"t1","graph_revision":10}"#;
        let p: GraphReleasePayload = serde_json::from_str(raw).unwrap();
        assert!(p.released);
        assert_eq!(p.task_id, "t1");
        assert_eq!(p.graph_revision, 10);
    }

    #[test]
    fn summarize_payload_fields() {
        let raw = r#"{"task_id":"t1","summary":"summary text","graph_revision":9,"dependencies":[],"recent_events":[]}"#;
        let p: GraphSummarizePayload = serde_json::from_str(raw).unwrap();
        assert_eq!(p.task_id, "t1");
        assert_eq!(p.summary, "summary text");
        assert_eq!(p.graph_revision, 9);
    }
}
