use serde::{Deserialize, Serialize};

/// Valid status values for result submission.
pub const RESULT_STATUSES: &[&str] = &["success", "fail", "blocked", "skipped", "cancelled"];

/// Canonical result packet for `submit-result`.
///
/// Represents an agent's output after completing a task.
/// The `status` field determines which graph-engine mutation command is invoked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalResultPacket {
    #[serde(default = "default_adapter_version")]
    pub adapter_version: String,
    pub profile: String,
    pub actor: String,
    pub task_id: String,
    pub graph_revision: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_agent_output_path: Option<String>,
}

fn default_adapter_version() -> String {
    crate::response::ADAPTER_VERSION.to_string()
}

/// A single piece of evidence supporting the result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub kind: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl CanonicalResultPacket {
    /// Returns true if the status requires a summary field.
    pub fn requires_summary(&self) -> bool {
        self.status == "success"
    }

    /// Returns true if the status requires a reason field.
    pub fn requires_reason(&self) -> bool {
        matches!(
            self.status.as_str(),
            "fail" | "blocked" | "skipped" | "cancelled"
        )
    }

    /// Validate the packet against the rules before submitting to the graph engine.
    ///
    /// Returns Ok(()) on valid packets; Err(AdapterError::InvalidResultPacket) otherwise.
    pub fn validate(&self) -> Result<(), crate::error::AdapterError> {
        if self.task_id.is_empty() {
            return Err(crate::error::AdapterError::InvalidResultPacket {
                message: "task_id must be non-empty".to_string(),
            });
        }
        if !RESULT_STATUSES.contains(&self.status.as_str()) {
            return Err(crate::error::AdapterError::InvalidResultPacket {
                message: format!(
                    "invalid status '{}'. Expected one of: {}",
                    self.status,
                    RESULT_STATUSES.join(", ")
                ),
            });
        }
        if self.requires_summary() && self.summary.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            return Err(crate::error::AdapterError::InvalidResultPacket {
                message: format!("status '{}' requires a non-empty summary", self.status),
            });
        }
        if self.requires_reason() && self.reason.as_ref().map(|r| r.is_empty()).unwrap_or(true) {
            return Err(crate::error::AdapterError::InvalidResultPacket {
                message: format!("status '{}' requires a non-empty reason", self.status),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_success_packet() -> CanonicalResultPacket {
        CanonicalResultPacket {
            adapter_version: "1.0.0".to_string(),
            profile: "test".to_string(),
            actor: "agent".to_string(),
            task_id: "t1".to_string(),
            graph_revision: 1,
            status: "success".to_string(),
            summary: Some("Done".to_string()),
            reason: None,
            artifacts: vec![],
            evidence: vec![],
            raw_agent_output_path: None,
        }
    }

    #[test]
    fn valid_success_passes() {
        let p = valid_success_packet();
        assert!(p.validate().is_ok());
    }

    #[test]
    fn invalid_status_fails() {
        let mut p = valid_success_packet();
        p.status = "unknown".to_string();
        assert!(p.validate().is_err());
    }

    #[test]
    fn empty_task_id_fails() {
        let mut p = valid_success_packet();
        p.task_id = String::new();
        assert!(p.validate().is_err());
    }

    #[test]
    fn success_without_summary_fails() {
        let mut p = valid_success_packet();
        p.summary = None;
        assert!(p.validate().is_err());
    }

    #[test]
    fn success_with_empty_summary_fails() {
        let mut p = valid_success_packet();
        p.summary = Some("".to_string());
        assert!(p.validate().is_err());
    }

    #[test]
    fn fail_without_reason_fails() {
        let mut p = valid_success_packet();
        p.status = "fail".to_string();
        p.summary = None;
        p.reason = None;
        assert!(p.validate().is_err());
    }

    #[test]
    fn fail_with_reason_passes() {
        let mut p = valid_success_packet();
        p.status = "fail".to_string();
        p.summary = None;
        p.reason = Some("it broke".to_string());
        assert!(p.validate().is_ok());
    }

    #[test]
    fn blocked_with_reason_passes() {
        let mut p = valid_success_packet();
        p.status = "blocked".to_string();
        p.summary = None;
        p.reason = Some("waiting".to_string());
        assert!(p.validate().is_ok());
    }

    #[test]
    fn skipped_with_reason_passes() {
        let mut p = valid_success_packet();
        p.status = "skipped".to_string();
        p.summary = None;
        p.reason = Some("not needed".to_string());
        assert!(p.validate().is_ok());
    }

    #[test]
    fn cancelled_with_reason_passes() {
        let mut p = valid_success_packet();
        p.status = "cancelled".to_string();
        p.summary = None;
        p.reason = Some("user asked".to_string());
        assert!(p.validate().is_ok());
    }

    #[test]
    fn roundtrip_json_serde() {
        let p = valid_success_packet();
        let json = serde_json::to_string(&p).unwrap();
        let p2: CanonicalResultPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn deserialize_omitted_optional_fields() {
        let json = r#"{"task_id":"t1","graph_revision":1,"status":"success","summary":"Done","profile":"test","actor":"agent"}"#;
        let p: CanonicalResultPacket = serde_json::from_str(json).unwrap();
        assert_eq!(p.status, "success");
        assert_eq!(p.reason, None);
        assert!(p.validate().is_ok());
    }
}
