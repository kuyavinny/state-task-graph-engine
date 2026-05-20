//! Criteria Evaluation Engine
//!
//! Evaluates structured phase entry/exit criteria against the current
//! workflow run context. Returns the first unmet criterion or `AllMet`.
//!
//! All evaluators are pure functions — no side effects, no subprocess calls.
//! Graph criteria evaluate against `CriteriaContext` (fetched separately).
//! Time evaluation accepts an injectable `now` for deterministic testing.

use crate::criteria_context::CriteriaContext;
use crate::model::Criterion;
use crate::paths::ProjectPaths;
use crate::run_state::ApprovalRecord;
use chrono::{DateTime, Utc};

pub mod artifact;
pub mod graph_state;
pub mod operator;
pub mod result;
pub mod time;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Context provided to criteria evaluation.
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    /// Normalized graph state (from `stage status` via `GraphStatusClient`).
    pub graph: CriteriaContext,
    /// Project paths for artifact resolution.
    pub paths: ProjectPaths,
    /// Approval records for the current run.
    pub approval_records: Vec<ApprovalRecord>,
    /// Phase ID being evaluated.
    pub phase_id: String,
    /// When the current phase started.
    pub phase_started_at: DateTime<Utc>,
    /// When the workflow run started.
    pub workflow_started_at: DateTime<Utc>,
    /// Current time — injected for deterministic testing.
    pub now: DateTime<Utc>,
    /// Optional result packet JSON for exit criteria evaluation.
    pub result_packet: Option<serde_json::Value>,
}

/// Result of evaluating a single criterion.
#[derive(Debug, Clone, PartialEq)]
pub enum CriterionResult {
    Met,
    NotMet { reason: String },
    Invalid { reason: String },
}

/// Result of evaluating all criteria for a phase.
#[derive(Debug, Clone, PartialEq)]
pub enum EvaluationResult {
    /// All criteria satisfied.
    AllMet,
    /// First unmet criterion and reason.
    NotMet {
        criterion_index: usize,
        reason: String,
    },
    /// A criterion is structurally invalid.
    Invalid {
        criterion_index: usize,
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Top-level evaluator
// ---------------------------------------------------------------------------

/// Evaluate a list of criteria against the given context.
/// Returns `AllMet` if every criterion passes, or the first unmet/invalid.
pub fn evaluate_criteria(
    criteria: &[Criterion],
    ctx: &EvaluationContext,
) -> EvaluationResult {
    for (i, criterion) in criteria.iter().enumerate() {
        let result = evaluate_one(criterion, ctx);
        match result {
            CriterionResult::Met => continue,
            CriterionResult::NotMet { reason } => {
                return EvaluationResult::NotMet {
                    criterion_index: i,
                    reason,
                };
            }
            CriterionResult::Invalid { reason } => {
                return EvaluationResult::Invalid {
                    criterion_index: i,
                    reason,
                };
            }
        }
    }
    EvaluationResult::AllMet
}

/// Evaluate a single criterion.
pub fn evaluate_one(criterion: &Criterion, ctx: &EvaluationContext) -> CriterionResult {
    match criterion {
        Criterion::GraphState(c) => graph_state::evaluate(c, &ctx.graph),
        Criterion::Artifact(c) => artifact::evaluate(c, &ctx.paths),
        Criterion::Result(c) => result::evaluate(c, ctx.result_packet.as_ref()),
        Criterion::OperatorApproval(c) => {
            operator::evaluate(c, &ctx.phase_id, &ctx.approval_records)
        }
        Criterion::Time(c) => time::evaluate(c, ctx.phase_started_at, ctx.workflow_started_at, ctx.now),
        Criterion::FutureHook => CriterionResult::Invalid {
            reason: "future_hook criteria are not supported".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::criteria_context::CriteriaContext;
    use crate::model::{Criterion, GraphStateCriterion, OperatorApprovalCriterion, TimeCriterion};
    use crate::paths::ProjectPaths;
    use crate::run_state::ApprovalRecord;
    use chrono::TimeZone;

    fn make_ctx() -> EvaluationContext {
        let tmp = tempfile::tempdir().expect("temp dir");
        let now = Utc::now();
        EvaluationContext {
            graph: CriteriaContext {
                graph_revision: 1,
                node_count: 5,
                status_counts: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("READY".to_string(), 2);
                    m.insert("COMPLETED".to_string(), 3);
                    m
                },
                warnings: vec![],
            },
            paths: ProjectPaths::from_root(tmp.path().to_path_buf()),
            approval_records: vec![],
            phase_id: "phase_1".to_string(),
            phase_started_at: now - chrono::Duration::minutes(10),
            workflow_started_at: now - chrono::Duration::hours(1),
            now,
            result_packet: None,
        }
    }

    #[test]
    fn test_evaluate_criteria_all_met() {
        let ctx = make_ctx();
        let criteria = vec![
            Criterion::GraphState(GraphStateCriterion {
                key: "status_counts.READY".to_string(),
                op: ">=".to_string(),
                value: 1_i64,
            }),
        ];
        assert_eq!(evaluate_criteria(&criteria, &ctx), EvaluationResult::AllMet);
    }

    #[test]
    fn test_evaluate_criteria_first_unmet() {
        let ctx = make_ctx();
        let criteria = vec![
            Criterion::GraphState(GraphStateCriterion {
                key: "status_counts.READY".to_string(),
                op: ">=".to_string(),
                value: 10_i64, // Only 2 READY
            }),
            Criterion::GraphState(GraphStateCriterion {
                key: "status_counts.COMPLETED".to_string(),
                op: ">=".to_string(),
                value: 1_i64, // Would pass, but never reached
            }),
        ];
        match evaluate_criteria(&criteria, &ctx) {
            EvaluationResult::NotMet { criterion_index, .. } => {
                assert_eq!(criterion_index, 0);
            }
            other => panic!("Expected NotMet, got {:?}", other),
        }
    }

    #[test]
    fn test_evaluate_criteria_future_hook_rejected() {
        let ctx = make_ctx();
        let criteria = vec![Criterion::FutureHook];
        match evaluate_criteria(&criteria, &ctx) {
            EvaluationResult::Invalid { reason, .. } => {
                assert!(reason.contains("future_hook"));
            }
            other => panic!("Expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn test_evaluate_criteria_short_circuit() {
        // Second criterion is invalid but first is not met — should stop at first
        let ctx = make_ctx();
        let criteria = vec![
            Criterion::GraphState(GraphStateCriterion {
                key: "status_counts.NONEXISTENT".to_string(),
                op: ">=".to_string(),
                value: 1_i64,
            }),
            Criterion::FutureHook,
        ];
        match evaluate_criteria(&criteria, &ctx) {
            EvaluationResult::NotMet { criterion_index, .. } => {
                assert_eq!(criterion_index, 0);
            }
            other => panic!("Expected NotMet at index 0, got {:?}", other),
        }
    }
}