use crate::model::{ErrorCode, Graph, Node, Status, ValidationError};

use std::collections::{HashMap, HashSet, VecDeque};

/// Validate the entire graph. Returns all errors found (empty vec = valid).
pub fn validate_graph(graph: &Graph) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if graph.nodes.is_empty() {
        return errors; // Empty graph is valid
    }

    // 1. Duplicate node IDs
    check_duplicate_ids(graph, &mut errors);

    // Build ID set for referential checks
    let id_set: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();

    // 2. Referential integrity (dependencies + parent_id)
    check_referential_integrity(graph, &id_set, &mut errors);

    // 3. Cycle detection (Kahn's algorithm handles unknown deps gracefully)
    check_cycles(graph, &id_set, &mut errors);

    // 4. Per-node field validation
    for node in &graph.nodes {
        check_node_fields(node, &mut errors);
    }

    errors
}

// ---------------------------------------------------------------------------
// Duplicate IDs
// ---------------------------------------------------------------------------

fn check_duplicate_ids(graph: &Graph, errors: &mut Vec<ValidationError>) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        if node.id.is_empty() {
            continue; // Empty-id caught by required-fields check
        }
        if let Some(&first) = seen.get(node.id.as_str()) {
            errors.push(ValidationError {
                code: ErrorCode::DuplicateNodeId,
                message: format!(
                    "Duplicate node ID '{}' at indices {} and {}",
                    node.id, first, i
                ),
                details: serde_json::json!({ "id": node.id }),
            });
        } else {
            seen.insert(&node.id, i);
        }
    }
}

// ---------------------------------------------------------------------------
// Referential Integrity
// ---------------------------------------------------------------------------

fn check_referential_integrity(
    graph: &Graph,
    id_set: &HashSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    for node in &graph.nodes {
        for dep_id in &node.dependencies {
            if !dep_id.is_empty() && !id_set.contains(dep_id.as_str()) {
                errors.push(ValidationError {
                    code: ErrorCode::UnknownDependency,
                    message: format!("Node '{}' depends on unknown node '{}'", node.id, dep_id),
                    details: serde_json::json!({
                        "id": node.id,
                        "dependency": dep_id,
                    }),
                });
            }
        }

        if let Some(ref parent_id) = node.parent_id
            && !parent_id.is_empty()
            && !id_set.contains(parent_id.as_str())
        {
            errors.push(ValidationError {
                code: ErrorCode::UnknownDependency,
                message: format!(
                    "Node '{}' references unknown parent '{}'",
                    node.id, parent_id
                ),
                details: serde_json::json!({
                    "id": node.id,
                    "parent_id": parent_id,
                }),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Cycle Detection (Kahn's Algorithm)
// ---------------------------------------------------------------------------

fn check_cycles(graph: &Graph, id_set: &HashSet<&str>, errors: &mut Vec<ValidationError>) {
    // in_degree[node] = number of its dependencies that are in the graph
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    // dependents[X] = nodes whose dependencies list includes X
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in &graph.nodes {
        in_degree.entry(node.id.as_str()).or_insert(0);
        for dep_id in &node.dependencies {
            if !dep_id.is_empty() && id_set.contains(dep_id.as_str()) {
                dependents
                    .entry(dep_id.as_str())
                    .or_default()
                    .push(node.id.as_str());
                *in_degree.entry(node.id.as_str()).or_insert(0) += 1;
            }
        }
    }

    // Start with nodes that have no unmet dependencies
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut sorted_count = 0;
    while let Some(id) = queue.pop_front() {
        sorted_count += 1;
        if let Some(deps) = dependents.get(id) {
            for &dep_id in deps {
                if let Some(deg) = in_degree.get_mut(dep_id) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dep_id);
                    }
                }
            }
        }
    }

    if sorted_count < graph.nodes.len() {
        errors.push(ValidationError {
            code: ErrorCode::CycleDetected,
            message: "Cycle detected in task dependencies".to_string(),
            details: serde_json::json!({
                "unprocessed_nodes": graph.nodes.len() - sorted_count,
            }),
        });
    }
}

// ---------------------------------------------------------------------------
// Per-Node Field Validation
// ---------------------------------------------------------------------------

fn check_node_fields(node: &Node, errors: &mut Vec<ValidationError>) {
    // Required fields
    if node.id.is_empty() {
        errors.push(ValidationError {
            code: ErrorCode::InvalidSchema,
            message: "Node has empty id".to_string(),
            details: serde_json::json!({ "field": "id" }),
        });
        // Skip further checks for this node — id is required for messages
        return;
    }

    if node.title.is_empty() {
        errors.push(validation_error(
            ErrorCode::InvalidSchema,
            format!("Node '{}' has empty title", node.id),
            serde_json::json!({ "id": node.id, "field": "title" }),
        ));
    }

    // Timestamp format (RFC 3339)
    check_timestamp(node, "created_at", &node.created_at, errors);
    check_timestamp(node, "updated_at", &node.updated_at, errors);

    // max_attempts >= 1
    if node.max_attempts == 0 {
        errors.push(validation_error(
            ErrorCode::InvalidSchema,
            format!("Node '{}' has max_attempts=0, must be >= 1", node.id),
            serde_json::json!({ "id": node.id, "field": "max_attempts", "value": 0 }),
        ));
    }

    // attempts <= max_attempts unless FAILED
    if node.attempts > node.max_attempts && node.status != Status::Failed {
        errors.push(validation_error(
            ErrorCode::InvalidSchema,
            format!(
                "Node '{}' has attempts({}) > max_attempts({})",
                node.id, node.attempts, node.max_attempts
            ),
            serde_json::json!({
                "id": node.id,
                "field": "attempts",
                "value": node.attempts,
                "max_attempts": node.max_attempts,
            }),
        ));
    }

    // Lease consistency
    check_lease_consistency(node, errors);

    // Terminal-state reason requirements
    check_terminal_reasons(node, errors);
}

// ---------------------------------------------------------------------------
// Timestamp Validation
// ---------------------------------------------------------------------------

fn check_timestamp(node: &Node, field: &str, value: &str, errors: &mut Vec<ValidationError>) {
    if value.is_empty() {
        errors.push(validation_error(
            ErrorCode::InvalidSchema,
            format!("Node '{}' has empty {}", node.id, field),
            serde_json::json!({ "id": node.id, "field": field }),
        ));
        return;
    }

    if chrono::DateTime::parse_from_rfc3339(value).is_err() {
        errors.push(validation_error(
            ErrorCode::InvalidSchema,
            format!("Node '{}' has invalid {}: '{}'", node.id, field, value),
            serde_json::json!({ "id": node.id, "field": field, "value": value }),
        ));
    }
}

// ---------------------------------------------------------------------------
// Lease Consistency
// ---------------------------------------------------------------------------

fn check_lease_consistency(node: &Node, errors: &mut Vec<ValidationError>) {
    match node.status {
        Status::InProgress => {
            if node.lease.claimed_by.is_none() {
                errors.push(missing_lease_field(node, "claimed_by"));
            }
            if node.lease.claimed_at.is_none() {
                errors.push(missing_lease_field(node, "claimed_at"));
            }
            if node.lease.expires_at.is_none() {
                errors.push(missing_lease_field(node, "expires_at"));
            }
        }
        _ => {
            // Non-IN_PROGRESS should not have active lease fields
            if node.lease.claimed_by.is_some() {
                errors.push(validation_error(
                    ErrorCode::InvalidSchema,
                    format!(
                        "Node '{}' is {} but has active lease.claimed_by",
                        node.id, node.status
                    ),
                    serde_json::json!({
                        "id": node.id,
                        "status": node.status.to_string(),
                        "field": "lease.claimed_by",
                    }),
                ));
            }
            if node.lease.claimed_at.is_some() {
                errors.push(validation_error(
                    ErrorCode::InvalidSchema,
                    format!(
                        "Node '{}' is {} but has active lease.claimed_at",
                        node.id, node.status
                    ),
                    serde_json::json!({
                        "id": node.id,
                        "status": node.status.to_string(),
                        "field": "lease.claimed_at",
                    }),
                ));
            }
            if node.lease.expires_at.is_some() {
                errors.push(validation_error(
                    ErrorCode::InvalidSchema,
                    format!(
                        "Node '{}' is {} but has active lease.expires_at",
                        node.id, node.status
                    ),
                    serde_json::json!({
                        "id": node.id,
                        "status": node.status.to_string(),
                        "field": "lease.expires_at",
                    }),
                ));
            }
        }
    }
}

fn missing_lease_field(node: &Node, field: &str) -> ValidationError {
    ValidationError {
        code: ErrorCode::InvalidSchema,
        message: format!(
            "Node '{}' is IN_PROGRESS but lease.{} is missing",
            node.id, field
        ),
        details: serde_json::json!({ "id": node.id, "field": format!("lease.{}", field) }),
    }
}

// ---------------------------------------------------------------------------
// Terminal-State Reason Requirements
// ---------------------------------------------------------------------------

fn check_terminal_reasons(node: &Node, errors: &mut Vec<ValidationError>) {
    match node.status {
        Status::Completed => {
            if node.result_summary.is_none() {
                errors.push(missing_reason(node, "result_summary"));
            }
            // Mutual exclusion: no contradictory reason fields
            check_contradictory_reason(node, "failure_reason", &node.failure_reason, errors);
            check_contradictory_reason(node, "blocked_reason", &node.blocked_reason, errors);
            check_contradictory_reason(node, "skip_reason", &node.skip_reason, errors);
            check_contradictory_reason(node, "cancel_reason", &node.cancel_reason, errors);
        }
        Status::Failed => {
            if node.failure_reason.is_none() {
                errors.push(missing_reason(node, "failure_reason"));
            }
            check_contradictory_reason(node, "result_summary", &node.result_summary, errors);
            check_contradictory_reason(node, "blocked_reason", &node.blocked_reason, errors);
            check_contradictory_reason(node, "skip_reason", &node.skip_reason, errors);
            check_contradictory_reason(node, "cancel_reason", &node.cancel_reason, errors);
        }
        Status::Blocked => {
            if node.blocked_reason.is_none() {
                errors.push(missing_reason(node, "blocked_reason"));
            }
            check_contradictory_reason(node, "result_summary", &node.result_summary, errors);
            check_contradictory_reason(node, "failure_reason", &node.failure_reason, errors);
            check_contradictory_reason(node, "skip_reason", &node.skip_reason, errors);
            check_contradictory_reason(node, "cancel_reason", &node.cancel_reason, errors);
        }
        Status::Skipped => {
            if node.skip_reason.is_none() {
                errors.push(missing_reason(node, "skip_reason"));
            }
            check_contradictory_reason(node, "result_summary", &node.result_summary, errors);
            check_contradictory_reason(node, "failure_reason", &node.failure_reason, errors);
            check_contradictory_reason(node, "blocked_reason", &node.blocked_reason, errors);
            check_contradictory_reason(node, "cancel_reason", &node.cancel_reason, errors);
        }
        Status::Cancelled => {
            if node.cancel_reason.is_none() {
                errors.push(missing_reason(node, "cancel_reason"));
            }
            check_contradictory_reason(node, "result_summary", &node.result_summary, errors);
            check_contradictory_reason(node, "failure_reason", &node.failure_reason, errors);
            check_contradictory_reason(node, "blocked_reason", &node.blocked_reason, errors);
            check_contradictory_reason(node, "skip_reason", &node.skip_reason, errors);
        }
        _ => {}
    }
}

fn check_contradictory_reason(
    node: &Node,
    field: &str,
    value: &Option<String>,
    errors: &mut Vec<ValidationError>,
) {
    if value.is_some() {
        errors.push(validation_error(
            ErrorCode::InvalidSchema,
            format!(
                "Node '{}' is {} but has contradictory field {}",
                node.id, node.status, field
            ),
            serde_json::json!({
                "id": node.id,
                "status": node.status.to_string(),
                "field": field,
            }),
        ));
    }
}

fn missing_reason(node: &Node, field: &str) -> ValidationError {
    ValidationError {
        code: ErrorCode::InvalidSchema,
        message: format!(
            "Node '{}' is {} but {} is missing",
            node.id, node.status, field
        ),
        details: serde_json::json!({ "id": node.id, "field": field }),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validation_error(
    code: ErrorCode,
    message: String,
    details: serde_json::Value,
) -> ValidationError {
    ValidationError {
        code,
        message,
        details,
    }
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lease, Node};

    fn valid_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            parent_id: None,
            title: format!("Task {}", id),
            description: "desc".to_string(),
            priority: 10,
            status: Status::Ready,
            dependencies: vec![],
            created_at: "2026-05-17T00:00:00Z".to_string(),
            updated_at: "2026-05-17T00:00:00Z".to_string(),
            attempts: 0,
            max_attempts: 3,
            lease: Lease::empty(),
            result_summary: None,
            failure_reason: None,
            blocked_reason: None,
            skip_reason: None,
            cancel_reason: None,
            evidence: vec![],
            artifacts: vec![],
            data: serde_json::Value::Null,
        }
    }

    fn graph_with_nodes(nodes: Vec<Node>) -> Graph {
        Graph {
            schema_version: "1.0".to_string(),
            graph_revision: 1,
            nodes,
        }
    }

    #[test]
    fn valid_dag_passes() {
        let a = valid_node("a");
        let mut b = valid_node("b");
        b.dependencies = vec!["a".to_string()];
        let graph = graph_with_nodes(vec![a, b]);
        assert!(validate_graph(&graph).is_empty());
    }

    #[test]
    fn empty_graph_is_valid() {
        let graph = graph_with_nodes(vec![]);
        assert!(validate_graph(&graph).is_empty());
    }

    #[test]
    fn duplicate_id_detected() {
        let a1 = valid_node("a");
        let a2 = valid_node("a");
        let graph = graph_with_nodes(vec![a1, a2]);
        let errors = validate_graph(&graph);
        assert!(errors.iter().any(|e| e.code == ErrorCode::DuplicateNodeId));
    }

    #[test]
    fn unknown_dependency_detected() {
        let mut a = valid_node("a");
        a.dependencies = vec!["nonexistent".to_string()];
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::UnknownDependency)
        );
    }

    #[test]
    fn unknown_parent_detected() {
        let mut a = valid_node("a");
        a.parent_id = Some("ghost".to_string());
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::UnknownDependency)
        );
    }

    #[test]
    fn cycle_detected() {
        let mut a = valid_node("a");
        a.dependencies = vec!["b".to_string()];
        let mut b = valid_node("b");
        b.dependencies = vec!["a".to_string()];
        let graph = graph_with_nodes(vec![a, b]);
        let errors = validate_graph(&graph);
        assert!(errors.iter().any(|e| e.code == ErrorCode::CycleDetected));
    }

    #[test]
    fn missing_title_detected() {
        let mut a = valid_node("a");
        a.title = "".to_string();
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::InvalidSchema && e.details["field"] == "title")
        );
    }

    #[test]
    fn invalid_timestamp_detected() {
        let mut a = valid_node("a");
        a.created_at = "not-a-date".to_string();
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::InvalidSchema && e.details["field"] == "created_at")
        );
    }

    #[test]
    fn max_attempts_zero_detected() {
        let mut a = valid_node("a");
        a.max_attempts = 0;
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(errors
            .iter()
            .any(|e| e.code == ErrorCode::InvalidSchema && e.details["field"] == "max_attempts"));
    }

    #[test]
    fn attempts_exceeds_max_unless_failed() {
        let mut a = valid_node("a");
        a.attempts = 5;
        a.max_attempts = 3;
        let graph = graph_with_nodes(vec![a.clone()]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::InvalidSchema && e.details["field"] == "attempts")
        );

        // FAILED node with attempts > max_attempts is allowed
        a.status = Status::Failed;
        a.failure_reason = Some("exhausted".to_string());
        let graph2 = graph_with_nodes(vec![a]);
        assert!(
            !validate_graph(&graph2)
                .iter()
                .any(|e| e.details["field"] == "attempts")
        );
    }

    #[test]
    fn in_progress_requires_lease() {
        let mut a = valid_node("a");
        a.status = Status::InProgress;
        a.lease = Lease::empty(); // missing all fields
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        let lease_errors: Vec<_> = errors
            .iter()
            .filter(|e| {
                e.details["field"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("lease.")
            })
            .collect();
        assert_eq!(lease_errors.len(), 3); // claimed_by, claimed_at, expires_at
    }

    #[test]
    fn non_in_progress_rejects_orphaned_lease_timestamps() {
        let mut a = valid_node("a");
        a.status = Status::Ready;
        a.lease = Lease {
            claimed_by: None,
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2026-05-18T00:00:00Z".to_string()),
        };
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.details["field"] == "lease.claimed_at")
        );
        assert!(
            errors
                .iter()
                .any(|e| e.details["field"] == "lease.expires_at")
        );
    }

    #[test]
    fn terminal_state_rejects_contradictory_reason_fields() {
        let mut a = valid_node("a");
        a.status = Status::Completed;
        a.result_summary = Some("done".to_string());
        a.failure_reason = Some("engine crashed".to_string());
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.details["field"] == "failure_reason")
        );
    }

    #[test]
    fn cycle_detected_even_with_unknown_deps() {
        let mut a = valid_node("a");
        a.dependencies = vec!["b".to_string()];
        let mut b = valid_node("b");
        b.dependencies = vec!["a".to_string()];
        let mut c = valid_node("c");
        c.dependencies = vec!["unknown".to_string()];
        let graph = graph_with_nodes(vec![a, b, c]);
        let errors = validate_graph(&graph);
        // Should detect BOTH unknown dep and the a↔b cycle
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::UnknownDependency)
        );
        assert!(errors.iter().any(|e| e.code == ErrorCode::CycleDetected));
    }

    #[test]
    fn non_in_progress_must_not_have_active_lease() {
        let mut a = valid_node("a");
        a.status = Status::Ready;
        a.lease = Lease {
            claimed_by: Some("worker".to_string()),
            claimed_at: None,
            expires_at: None,
        };
        let graph = graph_with_nodes(vec![a]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.details["field"] == "lease.claimed_by")
        );
    }

    #[test]
    fn terminal_state_requires_reason() {
        let mut a = valid_node("a");
        a.status = Status::Completed;
        // result_summary is None → error
        let graph = graph_with_nodes(vec![a.clone()]);
        let errors = validate_graph(&graph);
        assert!(
            errors
                .iter()
                .any(|e| e.details["field"] == "result_summary")
        );

        // With result_summary → no error
        a.result_summary = Some("done".to_string());
        let graph2 = graph_with_nodes(vec![a]);
        let errors2 = validate_graph(&graph2);
        assert!(
            !errors2
                .iter()
                .any(|e| e.details["field"] == "result_summary")
        );
    }
}
