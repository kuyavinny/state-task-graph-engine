//! Graph state criterion evaluation.
//!
//! Evaluates structured conditions against `CriteriaContext` fields.
//! Supports:
//! - `graph_revision` — integer compare.
//! - `node_count` — integer compare.
//! - `status_counts.<STATUS>` — map lookup + integer compare.

use crate::criteria_context::CriteriaContext;
use super::CriterionResult;
use crate::model::GraphStateCriterion;

/// Evaluate a graph_state criterion against the provided context.
///
/// Supported keys:
/// - `"graph_revision"` | `"node_count"` — direct integer fields.
/// - `"status_counts.READY"` | `"status_counts.COMPLETED"` | etc. — map lookup.
///
/// Supported ops: `==`, `!=`, `>`, `>=`, `<`, `<=`
pub fn evaluate(c: &GraphStateCriterion, ctx: &CriteriaContext) -> CriterionResult {
    let left = match c.key.as_str() {
        "graph_revision" => ctx.graph_revision as i64,
        "node_count" => ctx.node_count as i64,
        key => {
            if let Some(suffix) = key.strip_prefix("status_counts.") {
                ctx.status_counts
                    .get(suffix)
                    .copied()
                    .unwrap_or(0) as i64
            } else {
                return CriterionResult::Invalid {
                    reason: format!("Unknown graph_state key: '{}'", c.key),
                };
            }
        }
    };

    let right = c.value;

    let met = match c.op.as_str() {
        "==" => left == right,
        "!=" => left != right,
        ">" => left > right,
        ">=" => left >= right,
        "<" => left < right,
        "<=" => left <= right,
        other => {
            return CriterionResult::Invalid {
                reason: format!("Unsupported graph_state operator: '{}'", other),
            };
        }
    };

    if met {
        CriterionResult::Met
    } else {
        CriterionResult::NotMet {
            reason: format!(
                "graph_state criterion: {} {} {} (actual: {})",
                c.key, c.op, right, left
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GraphStateCriterion;

    fn ctx(counts: std::collections::HashMap<String, usize>) -> CriteriaContext {
        CriteriaContext {
            graph_revision: 3,
            node_count: 5,
            status_counts: counts,
            warnings: vec![],
        }
    }

    #[test]
    fn test_graph_revision_eq() {
        let c = GraphStateCriterion {
            key: "graph_revision".to_string(),
            op: "==".to_string(),
            value: 3,
        };
        let ctx = ctx(Default::default());
        assert_eq!(evaluate(&c, &ctx), CriterionResult::Met);
    }

    #[test]
    fn test_status_counts_ge() {
        let mut counts = std::collections::HashMap::new();
        counts.insert("READY".to_string(), 2);
        let c = GraphStateCriterion {
            key: "status_counts.READY".to_string(),
            op: ">=".to_string(),
            value: 1,
        };
        let ctx = ctx(counts);
        assert_eq!(evaluate(&c, &ctx), CriterionResult::Met);
    }

    #[test]
    fn test_status_counts_lt_fails() {
        let mut counts = std::collections::HashMap::new();
        counts.insert("READY".to_string(), 0);
        let c = GraphStateCriterion {
            key: "status_counts.READY".to_string(),
            op: ">=".to_string(),
            value: 1,
        };
        let ctx = ctx(counts);
        assert!(matches!(evaluate(&c, &ctx), CriterionResult::NotMet { .. }));
    }

    #[test]
    fn test_unknown_key_invalid() {
        let c = GraphStateCriterion {
            key: "foo".to_string(),
            op: "==".to_string(),
            value: 1,
        };
        let ctx = ctx(Default::default());
        assert!(matches!(evaluate(&c, &ctx), CriterionResult::Invalid { .. }));
    }

    #[test]
    fn test_unsupported_op_invalid() {
        let c = GraphStateCriterion {
            key: "node_count".to_string(),
            op: "contains".to_string(),
            value: 1,
        };
        let ctx = ctx(Default::default());
        assert!(matches!(evaluate(&c, &ctx), CriterionResult::Invalid { .. }));
    }
}
