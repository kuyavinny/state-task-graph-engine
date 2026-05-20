//! Criteria context derived from `agent-graph status` (via `stage status` binary).
//!
//! Contains normalized graph state used for phase entry/exit criteria evaluation.
//! Must NOT expose raw Module 1 internals to prevent coupling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Normalized graph state summary for criteria evaluation.
///
/// Produced by `GraphStatusClient::status()` by parsing the JSON envelope
/// returned by `stage status`. The fields exposed here are the ONLY
/// fields workflow criteria can reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriteriaContext {
    pub graph_revision: u64,
    pub node_count: usize,
    pub status_counts: HashMap<String, usize>,
    pub warnings: Vec<String>,
}

impl CriteriaContext {
    /// Returns the count for a given status string (e.g. "READY", "COMPLETED").
    pub fn count_by_status(&self, status: &str) -> usize {
        self.status_counts
            .get(status)
            .copied()
            .unwrap_or(0)
    }

    /// True if any warning was present in the status response.
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_by_status() {
        let ctx = CriteriaContext {
            graph_revision: 42,
            node_count: 10,
            status_counts: [
                ("READY".to_string(), 3),
                ("IN_PROGRESS".to_string(), 1),
                ("COMPLETED".to_string(), 4),
            ]
            .into_iter()
            .collect(),
            warnings: vec![],
        };
        assert_eq!(ctx.count_by_status("READY"), 3);
        assert_eq!(ctx.count_by_status("COMPLETED"), 4);
        assert_eq!(ctx.count_by_status("MISSING"), 0);
    }

    #[test]
    fn test_has_warnings() {
        let nowarn = CriteriaContext {
            graph_revision: 1,
            node_count: 5,
            status_counts: HashMap::new(),
            warnings: vec![],
        };
        let warn = CriteriaContext {
            graph_revision: 1,
            node_count: 5,
            status_counts: HashMap::new(),
            warnings: vec!["desync".to_string()],
        };
        assert!(!nowarn.has_warnings());
        assert!(warn.has_warnings());
    }
}
