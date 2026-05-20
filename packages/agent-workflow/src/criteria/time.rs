//! Time criterion evaluation.
//!
//! Checks elapsed time since phase start or workflow start.
//! Accepts an injectable `now` for deterministic testing.

use crate::model::TimeCriterion;
use super::CriterionResult;
use chrono::{DateTime, Utc};

/// Evaluate a time criterion.
///
/// `since` must be `"phase_start"` or `"workflow_start"`.
/// Compares elapsed minutes against `elapsed_minutes` threshold.
pub fn evaluate(
    c: &TimeCriterion,
    phase_started_at: DateTime<Utc>,
    workflow_started_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> CriterionResult {
    let start = match c.since.as_str() {
        "phase_start" => phase_started_at,
        "workflow_start" => workflow_started_at,
        other => {
            return CriterionResult::Invalid {
                reason: format!("Unknown time.since value: '{}'", other),
            };
        }
    };

    let elapsed = now.signed_duration_since(start);
    let elapsed_minutes = elapsed.num_minutes().max(0) as u64;

    if elapsed_minutes >= c.elapsed_minutes {
        // Threshold exceeded — criterion met (action is carried out by caller)
        CriterionResult::Met
    } else {
        CriterionResult::NotMet {
            reason: format!(
                "Elapsed {} min (< required {} min since {})",
                elapsed_minutes, c.elapsed_minutes, c.since
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TimeCriterion;
    use chrono::TimeZone;

    #[test]
    fn test_time_phase_start_met() {
        let c = TimeCriterion {
            since: "phase_start".to_string(),
            elapsed_minutes: 30,
            action: "fail".to_string(),
        };
        let phase_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2024, 1, 1, 0, 45, 0).unwrap(); // 45 min elapsed
        let workflow_start = phase_start - chrono::Duration::hours(2);

        assert_eq!(
            evaluate(&c, phase_start, workflow_start, now),
            CriterionResult::Met
        );
    }

    #[test]
    fn test_time_phase_start_not_met() {
        let c = TimeCriterion {
            since: "phase_start".to_string(),
            elapsed_minutes: 60,
            action: "fail".to_string(),
        };
        let phase_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap(); // 30 min elapsed
        let workflow_start = phase_start - chrono::Duration::hours(2);

        assert!(matches!(
            evaluate(&c, phase_start, workflow_start, now),
            CriterionResult::NotMet { .. }
        ));
    }

    #[test]
    fn test_time_workflow_start_met() {
        let c = TimeCriterion {
            since: "workflow_start".to_string(),
            elapsed_minutes: 120,
            action: "fail".to_string(),
        };
        let workflow_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2024, 1, 1, 3, 0, 0).unwrap(); // 180 min
        let phase_start = Utc.with_ymd_and_hms(2024, 1, 1, 2, 0, 0).unwrap();

        assert_eq!(
            evaluate(&c, phase_start, workflow_start, now),
            CriterionResult::Met
        );
    }

    #[test]
    fn test_time_invalid_since() {
        let c = TimeCriterion {
            since: "random".to_string(),
            elapsed_minutes: 10,
            action: "fail".to_string(),
        };
        let now = Utc::now();
        assert!(matches!(
            evaluate(&c, now, now, now),
            CriterionResult::Invalid { .. }
        ));
    }
}
