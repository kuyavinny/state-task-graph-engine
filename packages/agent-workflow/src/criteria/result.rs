//! Result criterion evaluation.
//!
//! Parses Canonical Result Packet JSON **only** for criteria evaluation.
//! Does NOT modify, re-normalize, or reserialize the result file.

use crate::model::ResultCriterion;
use super::CriterionResult;
/// Evaluate a result criterion against a Canonical Result Packet JSON value.
///
/// Parses the JSON only to extract status and task_id fields for comparison.
/// Does not modify or reserialize the result packet.
pub fn evaluate(c: &ResultCriterion, packet: Option<&serde_json::Value>) -> CriterionResult {
    let packet = match packet {
        Some(p) => p,
        None => {
            return CriterionResult::NotMet {
                reason: "No result packet available".to_string(),
            };
        }
    };

    // Extract status from Canonical Result Packet
    let status = packet
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if status != c.status {
        return CriterionResult::NotMet {
            reason: format!("Result status '{}' does not match expected '{}'", status, c.status),
        };
    }

    // Optional: verify last_task_completed matches
    if let Some(expected_task) = &c.last_task_completed {
        let actual_task = packet
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if actual_task != expected_task {
            return CriterionResult::NotMet {
                reason: format!(
                    "Result task_id '{}' does not match expected '{}'",
                    actual_task, expected_task
                ),
            };
        }
    }

    CriterionResult::Met
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ResultCriterion;

    #[test]
    fn test_result_status_matches() {
        let c = ResultCriterion {
            status: "success".to_string(),
            last_task_completed: None,
        };
        let packet = serde_json::json!({
            "status": "success",
            "task_id": "task_001"
        });
        assert_eq!(evaluate(&c, Some(&packet)), CriterionResult::Met);
    }

    #[test]
    fn test_result_status_mismatch() {
        let c = ResultCriterion {
            status: "success".to_string(),
            last_task_completed: None,
        };
        let packet = serde_json::json!({
            "status": "failure"
        });
        assert!(matches!(evaluate(&c, Some(&packet)), CriterionResult::NotMet { .. }));
    }

    #[test]
    fn test_result_task_id_matches() {
        let c = ResultCriterion {
            status: "success".to_string(),
            last_task_completed: Some("task_001".to_string()),
        };
        let packet = serde_json::json!({
            "status": "success",
            "task_id": "task_001"
        });
        assert_eq!(evaluate(&c, Some(&packet)), CriterionResult::Met);
    }

    #[test]
    fn test_result_task_id_mismatch() {
        let c = ResultCriterion {
            status: "success".to_string(),
            last_task_completed: Some("task_001".to_string()),
        };
        let packet = serde_json::json!({
            "status": "success",
            "task_id": "task_002"
        });
        assert!(matches!(evaluate(&c, Some(&packet)), CriterionResult::NotMet { .. }));
    }

    #[test]
    fn test_result_no_packet() {
        let c = ResultCriterion {
            status: "success".to_string(),
            last_task_completed: None,
        };
        assert!(matches!(evaluate(&c, None), CriterionResult::NotMet { .. }));
    }
}
