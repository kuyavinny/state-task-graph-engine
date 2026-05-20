//! Operator approval criterion evaluation.
//!
//! Checks if an APPROVED approval record exists for the current phase.

use crate::model::OperatorApprovalCriterion;
use crate::run_state::{ApprovalDecision, ApprovalRecord};
use super::CriterionResult;

/// Evaluate operator approval: requires at least one APPROVED record for the phase.
pub fn evaluate(
    _c: &OperatorApprovalCriterion,
    phase_id: &str,
    records: &[ApprovalRecord],
) -> CriterionResult {
    let approved = records.iter().any(|r| {
        r.phase_id == phase_id && r.decision == ApprovalDecision::Approved
    });

    if approved {
        CriterionResult::Met
    } else {
        CriterionResult::NotMet {
            reason: format!(
                "Operator approval not yet received for phase '{}'",
                phase_id
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OperatorApprovalCriterion;
    use crate::run_state::ApprovalRecord;
    use chrono::Utc;

    #[test]
    fn test_approval_approved() {
        let c = OperatorApprovalCriterion { decision: None };
        let records = vec![ApprovalRecord {
            approval_id: "a1".to_string(),
            phase_id: "phase_1".to_string(),
            operator: "alice".to_string(),
            decision: ApprovalDecision::Approved,
            reason: "Looks good".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }];
        assert_eq!(evaluate(&c, "phase_1", &records), CriterionResult::Met);
    }

    #[test]
    fn test_approval_rejected() {
        let c = OperatorApprovalCriterion { decision: None };
        let records = vec![ApprovalRecord {
            approval_id: "a1".to_string(),
            phase_id: "phase_1".to_string(),
            operator: "alice".to_string(),
            decision: ApprovalDecision::Rejected,
            reason: "Broken".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }];
        assert!(matches!(
            evaluate(&c, "phase_1", &records),
            CriterionResult::NotMet { .. }
        ));
    }

    #[test]
    fn test_approval_none() {
        let c = OperatorApprovalCriterion { decision: None };
        let records: Vec<ApprovalRecord> = vec![];
        assert!(matches!(
            evaluate(&c, "phase_1", &records),
            CriterionResult::NotMet { .. }
        ));
    }

    #[test]
    fn test_approval_different_phase() {
        let c = OperatorApprovalCriterion { decision: None };
        let records = vec![ApprovalRecord {
            approval_id: "a1".to_string(),
            phase_id: "phase_2".to_string(),
            operator: "alice".to_string(),
            decision: ApprovalDecision::Approved,
            reason: "Good".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }];
        assert!(matches!(
            evaluate(&c, "phase_1", &records),
            CriterionResult::NotMet { .. }
        ));
    }
}
