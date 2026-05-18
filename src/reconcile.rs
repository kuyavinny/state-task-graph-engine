use crate::error::AppError;
use crate::io;
use crate::model::{Event, EventAction, Graph, Status};
use crate::validate;
use std::path::Path;

/// Result of a reconciliation pass.
#[derive(Debug)]
pub struct ReconciliationResult {
    /// The reconciled graph (may have been mutated).
    pub graph: Graph,
    /// Events generated during reconciliation (for appending to event log).
    #[allow(dead_code)]
    pub events: Vec<Event>,
    /// Warnings produced during reconciliation (e.g., revision desync).
    pub warnings: Vec<ReconciliationWarning>,
}

/// A non-fatal warning produced during reconciliation.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconciliationWarning {
    /// The graph revision does not match the highest event log revision.
    EventLogDesync {
        graph_revision: u64,
        event_log_revision: u64,
    },
}

/// Load the graph, validate it, run reconciliation, and return the result.
///
/// If validation fails, returns an error immediately.
/// If reconciliation produces warnings, they are included in the result
/// but do not prevent operation.
pub fn load_validate_reconcile(project_dir: &Path) -> Result<ReconciliationResult, AppError> {
    // 1. Load graph
    let mut graph = io::read_graph(project_dir)?;

    // 2. Validate
    let validation_errors = validate::validate_graph(&graph);
    if !validation_errors.is_empty() {
        let count = validation_errors.len();
        return Err(AppError::GraphValidationFailed {
            count,
            errors: validation_errors,
        });
    }

    // 3. Check revision desync — before any mutations
    let mut warnings = Vec::new();
    if let Err(desync_warning) = check_revision_desync(project_dir, &graph) {
        warnings.push(desync_warning);
        // Return graph without reconciliation — don't blindly append events to a desynced log
        return Ok(ReconciliationResult {
            graph,
            events: Vec::new(),
            warnings,
        });
    }

    // 4. Reconcile
    let mut events = Vec::new();

    // 4a. Lazy lease expiry
    reconcile_lazy_leases(&mut graph, &mut events);

    // 4b. PENDING → READY promotion
    reconcile_pending_to_ready(&mut graph, &mut events);

    // 5. Persist if graph changed
    if !events.is_empty() {
        let new_revision = graph.graph_revision + events.len() as u64;
        // Update timestamps and revision
        graph.graph_revision = new_revision;

        // Write events first, then graph. If graph write fails, desync check
        // will catch the mismatch on next reconcile.
        io::append_events_batch(project_dir, &events)?;
        io::write_graph(project_dir, &graph)?;
    }

    Ok(ReconciliationResult {
        graph,
        events,
        warnings,
    })
}

/// Evaluate lazy lease expirations.
///
/// For each IN_PROGRESS node where `expires_at` is in the past:
/// - If `attempts < max_attempts`: clear lease → READY, append lease-expiration event
/// - If `attempts >= max_attempts`: clear lease → FAILED, append lease-expiration/failure event
fn reconcile_lazy_leases(graph: &mut Graph, events: &mut Vec<Event>) {
    let now = chrono::Utc::now();
    let now_rfc3339 = now.to_rfc3339();

    // Collect indices of expired leases to avoid borrow issues
    let expired: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.status == Status::InProgress
                && node.lease.expires_at.as_ref().is_some_and(|expires_at| {
                    chrono::DateTime::parse_from_rfc3339(expires_at)
                        .map(|dt| dt < now)
                        .unwrap_or(false)
                })
        })
        .map(|(i, _)| i)
        .collect();

    for idx in expired {
        let node = &mut graph.nodes[idx];
        let actor = node
            .lease
            .claimed_by
            .clone()
            .unwrap_or_else(|| "system".to_string());
        let old_status = node.status;

        if node.attempts < node.max_attempts {
            // Expired but under limit: back to READY
            node.status = Status::Ready;
        } else {
            // Expired and at limit: FAIL
            node.status = Status::Failed;
            node.failure_reason = Some("Lease expired: max attempts reached".to_string());
        }

        let new_status = node.status;

        // Clear lease
        node.lease.claimed_by = None;
        node.lease.claimed_at = None;
        node.lease.expires_at = None;

        node.updated_at = now_rfc3339.clone();

        events.push(make_event(
            &node.id,
            &actor,
            EventAction::LeaseExpired,
            Some(old_status),
            Some(new_status),
            Some("Lease expired".to_string()),
            graph.graph_revision + events.len() as u64 + 1,
        ));
    }
}

/// Promote PENDING nodes to READY when all dependencies are COMPLETED or SKIPPED.
fn reconcile_pending_to_ready(graph: &mut Graph, events: &mut Vec<Event>) {
    let now_rfc3339 = chrono::Utc::now().to_rfc3339();

    // Build a map of node statuses for dependency checking
    let status_map: std::collections::HashMap<&str, Status> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.status))
        .collect();

    let ready_ids: Vec<String> = graph
        .nodes
        .iter()
        .filter(|node| {
            node.status == Status::Pending
                && node.dependencies.iter().all(|dep_id| {
                    status_map
                        .get(dep_id.as_str())
                        .is_some_and(|s| *s == Status::Completed || *s == Status::Skipped)
                })
        })
        .map(|n| n.id.clone())
        .collect();

    for id in ready_ids {
        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == id) {
            let old_status = node.status;
            node.status = Status::Ready;
            node.updated_at = now_rfc3339.clone();

            events.push(make_event(
                &node.id,
                "system",
                EventAction::DependencyResolved,
                Some(old_status),
                Some(Status::Ready),
                Some("All dependencies resolved".to_string()),
                graph.graph_revision + events.len() as u64 + 1,
            ));
        }
    }
}

/// Check for revision desync between the graph and event log.
///
/// Compares `graph.graph_revision` against the highest revision found
/// in the event log. If they differ, returns a warning.
fn check_revision_desync(project_dir: &Path, graph: &Graph) -> Result<(), ReconciliationWarning> {
    let events_path = project_dir.join(io::AGENT_DIR).join(io::EVENTS_FILE);

    if !events_path.exists() {
        // No event log yet — no desync possible
        return Ok(());
    }

    let content = match std::fs::read_to_string(&events_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // Can't read — skip check
    };

    // Find highest graph_revision_after in the event log
    let mut max_event_revision: u64 = 0;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<Event>(line)
            && event.graph_revision_after > max_event_revision
        {
            max_event_revision = event.graph_revision_after;
        }
    }

    let graph_revision = graph.graph_revision;

    if graph_revision != max_event_revision {
        Err(ReconciliationWarning::EventLogDesync {
            graph_revision,
            event_log_revision: max_event_revision,
        })
    } else {
        Ok(())
    }
}

/// Helper to create an Event with a UUID.
fn make_event(
    node_id: &str,
    actor: &str,
    action: EventAction,
    from_status: Option<Status>,
    to_status: Option<Status>,
    reason: Option<String>,
    graph_revision_after: u64,
) -> Event {
    Event {
        event_id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        graph_revision_before: graph_revision_after.saturating_sub(1),
        graph_revision_after,
        node_id: node_id.to_string(),
        actor: actor.to_string(),
        action,
        from_status,
        to_status,
        reason,
        metadata: serde_json::Value::Null,
    }
}

/// Append new nodes to the graph with revision-gated mutation.
///
/// 1. Loads current graph
/// 2. Checks revision (STALE_REVISION if mismatch)
/// 3. Validates no duplicate IDs between new and existing nodes
/// 4. Merges new nodes into graph
/// 5. Validates merged graph (cycle detection, referential integrity)
/// 6. Generates AppendNodes events for each new node
/// 7. Runs reconciliation (leases, PENDING → READY)
/// 8. Persists graph and events
pub fn append_nodes(
    project_dir: &Path,
    revision: u64,
    new_nodes: Vec<crate::model::Node>,
) -> Result<ReconciliationResult, AppError> {
    use crate::model::Node;
    use std::collections::HashSet;

    // 1. Preflight: load, validate, reconcile, catch desync
    let preflight = load_validate_reconcile(project_dir)?;
    if !preflight.warnings.is_empty() {
        return Err(AppError::EventLogDesync);
    }
    let mut graph = preflight.graph;

    // 2. Stale revision check (against reconciled revision)
    if graph.graph_revision != revision {
        return Err(AppError::StaleRevision {
            expected: graph.graph_revision,
            provided: revision,
        });
    }

    // 3. Check for duplicate IDs: existing <-> new AND new <-> new
    let mut seen: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    let mut new_nodes_clean: Vec<Node> = Vec::with_capacity(new_nodes.len());
    for node in new_nodes {
        if !seen.insert(node.id.clone()) {
            return Err(AppError::DuplicateNodeId { id: node.id });
        }
        new_nodes_clean.push(node);
    }

    let now_rfc3339 = chrono::Utc::now().to_rfc3339();

    // 4. Merge new nodes (ensure timestamps) and generate events
    let mut events = Vec::new();
    for mut node in new_nodes_clean {
        if node.created_at.is_empty() {
            node.created_at = now_rfc3339.clone();
        }
        if node.updated_at.is_empty() {
            node.updated_at = now_rfc3339.clone();
        }
        // Normalize lease: if unclaimed, ensure all lease fields are None
        if node.lease.claimed_by.is_none() {
            node.lease = crate::model::Lease::empty();
        }
        // Emit AppendNodes event
        events.push(make_event(
            &node.id,
            "system",
            EventAction::AppendNodes,
            None,
            Some(node.status),
            None,
            // Tentative revision: indexed from 1 relative to graph_revision.
            // Final revision = old_rev + total_event_count is computed at persist time below.
            graph.graph_revision + events.len() as u64 + 1,
        ));
        graph.nodes.push(node);
    }

    // 5. Validate merged graph
    let validation_errors = validate::validate_graph(&graph);
    if !validation_errors.is_empty() {
        let count = validation_errors.len();
        return Err(AppError::GraphValidationFailed {
            count,
            errors: validation_errors,
        });
    }

    // 6. Run reconciliation (lease expiry, PENDING → READY promotion)
    reconcile_lazy_leases(&mut graph, &mut events);
    reconcile_pending_to_ready(&mut graph, &mut events);

    // 7. Persist (write events first, then graph for crash safety)
    let new_revision = graph.graph_revision + events.len() as u64;
    graph.graph_revision = new_revision;
    io::append_events_batch(project_dir, &events)?;
    io::write_graph(project_dir, &graph)?;

    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io;
    use crate::model::{Lease, Node};

    fn valid_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            parent_id: None,
            title: format!("Task {id}"),
            description: format!("Description for {id}"),
            priority: 1,
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

    #[test]
    fn expired_lease_under_max_attempts_returns_to_ready() {
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.attempts = 1;
        node.max_attempts = 3;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2026-05-17T00:01:00Z".to_string()), // in the past
        };

        let mut graph = Graph::new();
        graph.nodes.push(node);
        let mut events = Vec::new();

        reconcile_lazy_leases(&mut graph, &mut events);

        assert_eq!(graph.nodes[0].status, Status::Ready);
        assert!(graph.nodes[0].lease.claimed_by.is_none());
        assert!(graph.nodes[0].lease.claimed_at.is_none());
        assert!(graph.nodes[0].lease.expires_at.is_none());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::LeaseExpired);
    }

    #[test]
    fn expired_lease_at_max_attempts_becomes_failed() {
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.attempts = 3;
        node.max_attempts = 3;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2026-05-17T00:01:00Z".to_string()), // in the past
        };

        let mut graph = Graph::new();
        graph.nodes.push(node);
        let mut events = Vec::new();

        reconcile_lazy_leases(&mut graph, &mut events);

        assert_eq!(graph.nodes[0].status, Status::Failed);
        assert!(graph.nodes[0].failure_reason.is_some());
        assert!(graph.nodes[0].lease.claimed_by.is_none());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::LeaseExpired);
    }

    #[test]
    fn active_lease_not_expired_remains_in_progress() {
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.attempts = 1;
        node.max_attempts = 3;
        // Set expires_at far in the future
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };

        let mut graph = Graph::new();
        graph.nodes.push(node);
        let mut events = Vec::new();

        reconcile_lazy_leases(&mut graph, &mut events);

        assert_eq!(graph.nodes[0].status, Status::InProgress);
        assert!(graph.nodes[0].lease.claimed_by.is_some());
        assert!(events.is_empty());
    }

    #[test]
    fn pending_promotes_to_ready_when_deps_completed() {
        let mut a = valid_node("a");
        a.status = Status::Completed;
        a.result_summary = Some("done".to_string());

        let mut b = valid_node("b");
        b.status = Status::Pending;
        b.dependencies = vec!["a".to_string()];

        let mut graph = Graph::new();
        graph.nodes.push(a);
        graph.nodes.push(b);
        let mut events = Vec::new();

        reconcile_pending_to_ready(&mut graph, &mut events);

        assert_eq!(graph.nodes[1].status, Status::Ready);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::DependencyResolved);
    }

    #[test]
    fn pending_promotes_to_ready_when_deps_skipped() {
        let mut a = valid_node("a");
        a.status = Status::Skipped;
        a.skip_reason = Some("not needed".to_string());

        let mut b = valid_node("b");
        b.status = Status::Pending;
        b.dependencies = vec!["a".to_string()];

        let mut graph = Graph::new();
        graph.nodes.push(a);
        graph.nodes.push(b);
        let mut events = Vec::new();

        reconcile_pending_to_ready(&mut graph, &mut events);

        assert_eq!(graph.nodes[1].status, Status::Ready);
    }

    #[test]
    fn pending_with_unresolved_dep_stays_pending() {
        let mut a = valid_node("a");
        a.status = Status::InProgress;

        let mut b = valid_node("b");
        b.status = Status::Pending;
        b.dependencies = vec!["a".to_string()];

        let mut graph = Graph::new();
        graph.nodes.push(a);
        graph.nodes.push(b);
        let mut events = Vec::new();

        reconcile_pending_to_ready(&mut graph, &mut events);

        assert_eq!(graph.nodes[1].status, Status::Pending);
        assert!(events.is_empty());
    }

    #[test]
    fn revision_desync_detected() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Write graph with revision 5
        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.graph_revision = 5;
        io::write_graph(tmp.path(), &graph).unwrap();

        // Append event with revision_after = 3
        let event = Event {
            event_id: "test-1".to_string(),
            timestamp: "2026-05-17T00:00:00Z".to_string(),
            graph_revision_before: 2,
            graph_revision_after: 3,
            node_id: "a".to_string(),
            actor: "system".to_string(),
            action: EventAction::Init,
            from_status: None,
            to_status: None,
            reason: None,
            metadata: serde_json::Value::Null,
        };
        let event_json = serde_json::to_string(&event).unwrap();
        io::append_event(tmp.path(), &event_json).unwrap();

        let result = check_revision_desync(tmp.path(), &graph);
        assert!(result.is_err());
        if let Err(ReconciliationWarning::EventLogDesync {
            graph_revision,
            event_log_revision,
        }) = result
        {
            assert_eq!(graph_revision, 5);
            assert_eq!(event_log_revision, 3);
        }
    }

    #[test]
    fn no_desync_when_revisions_match() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.graph_revision = 0;
        io::write_graph(tmp.path(), &graph).unwrap();

        // Empty event log → no events → max revision = 0 → matches graph_revision
        let result = check_revision_desync(tmp.path(), &graph);
        assert!(result.is_ok());
    }

    #[test]
    fn no_post_reconcile_desync() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Set up a graph with a PENDING node that has a COMPLETED dependency
        let mut graph = io::read_graph(tmp.path()).unwrap();
        let mut a = valid_node("a");
        a.status = Status::Completed;
        a.result_summary = Some("done".to_string());
        let mut b = valid_node("b");
        b.status = Status::Pending;
        b.dependencies = vec!["a".to_string()];
        graph.nodes.push(a);
        graph.nodes.push(b);
        io::write_graph(tmp.path(), &graph).unwrap();

        // Run load_validate_reconcile — should promote b→Ready, write graph, append events
        let result = load_validate_reconcile(tmp.path()).unwrap();
        assert!(!result.events.is_empty());

        // Check revision desync — should be clean after reconcile
        let result2 = check_revision_desync(tmp.path(), &result.graph);
        assert!(
            result2.is_ok(),
            "No desync expected after successful reconcile"
        );
    }

    #[test]
    fn desync_gates_reconciliation_mutations() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Write graph with revision 5
        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.graph_revision = 5;
        let mut a = valid_node("a");
        // An expired IN_PROGRESS node
        a.status = Status::InProgress;
        a.attempts = 1;
        a.max_attempts = 3;
        a.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2026-05-17T00:01:00Z".to_string()), // in the past
        };
        graph.nodes.push(a);
        io::write_graph(tmp.path(), &graph).unwrap();

        // Append event with revision_after = 3 (desynced)
        let event = Event {
            event_id: "test-1".to_string(),
            timestamp: "2026-05-17T00:00:00Z".to_string(),
            graph_revision_before: 2,
            graph_revision_after: 3,
            node_id: "a".to_string(),
            actor: "system".to_string(),
            action: EventAction::Init,
            from_status: None,
            to_status: None,
            reason: None,
            metadata: serde_json::Value::Null,
        };
        let event_json = serde_json::to_string(&event).unwrap();
        io::append_event(tmp.path(), &event_json).unwrap();

        // Run load_validate_reconcile — should return desync warning, NO mutations
        let result = load_validate_reconcile(tmp.path());
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(
            result.events.is_empty(),
            "No reconciliation events when desynced"
        );
        assert_eq!(result.warnings.len(), 1);

        // Graph should not have been modified
        let graph_after = io::read_graph(tmp.path()).unwrap();
        assert_eq!(
            graph_after.nodes[0].status,
            Status::InProgress,
            "Node should not have been mutated"
        );
        assert_eq!(
            graph_after.graph_revision, 5,
            "Revision should not have been updated"
        );
    }

    // ── append_nodes unit tests ───────────────────────────────────────────

    #[test]
    fn append_nodes_adds_nodes_and_increments_revision() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Graph starts at rev 0 with no nodes (empty event log matches)
        let new_nodes = vec![valid_node("new-1"), valid_node("new-2")];
        let result = append_nodes(tmp.path(), 0, new_nodes).unwrap();

        assert_eq!(result.graph.nodes.len(), 2);
        assert!(result.graph.graph_revision > 0);
        assert!(!result.events.is_empty());
        // Each new node gets an AppendNodes event
        assert_eq!(result.events.len(), 2);
    }

    #[test]
    fn append_nodes_empty_list_returns_ok_with_no_events() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        let result = append_nodes(tmp.path(), 0, vec![]).unwrap();
        assert_eq!(result.graph.nodes.len(), 0);
        assert_eq!(result.graph.graph_revision, 0);
        assert!(result.events.is_empty());
    }

    #[test]
    fn append_nodes_rejects_stale_revision() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        let result = append_nodes(tmp.path(), 5, vec![valid_node("a")]);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::StaleRevision { expected, provided } => {
                assert_eq!(expected, 0);
                assert_eq!(provided, 5);
            }
            other => panic!("Expected StaleRevision, got: {:?}", other),
        }
    }

    #[test]
    fn append_nodes_rejects_duplicate_id() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Write one node at rev 0
        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.nodes.push(valid_node("existing"));
        io::write_graph(tmp.path(), &graph).unwrap();

        // Try to append another node with the same ID
        let new_nodes = vec![valid_node("existing")];
        let result = append_nodes(tmp.path(), 0, new_nodes);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::DuplicateNodeId { id } => assert_eq!(id, "existing"),
            other => panic!("Expected DuplicateNodeId, got: {:?}", other),
        }
    }

    #[test]
    fn append_nodes_rejects_cycle_creating_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Append two nodes that form a cycle: a -> b -> a
        let mut a = valid_node("a");
        a.dependencies = vec!["b".to_string()];
        let mut b = valid_node("b");
        b.dependencies = vec!["a".to_string()];
        let result = append_nodes(tmp.path(), 0, vec![a, b]);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::GraphValidationFailed { count, .. } => assert!(count > 0),
            other => panic!("Expected GraphValidationFailed, got: {:?}", other),
        }
    }

    #[test]
    fn append_nodes_resolves_dependencies_and_promotes() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Write COMPLETED node "a" at rev 0
        let mut graph = io::read_graph(tmp.path()).unwrap();
        let mut a = valid_node("a");
        a.status = Status::Completed;
        a.result_summary = Some("done".to_string());
        graph.nodes.push(a);
        io::write_graph(tmp.path(), &graph).unwrap();

        // Append PENDING node "b" depending on "a" — should promote to READY
        let mut b = valid_node("b");
        b.status = Status::Pending;
        b.dependencies = vec!["a".to_string()];
        let result = append_nodes(tmp.path(), 0, vec![b]).unwrap();

        let b_node = result.graph.nodes.iter().find(|n| n.id == "b").unwrap();
        assert_eq!(b_node.status, Status::Ready);
        // 1 append event + 1 dep resolution = 2 events
        assert_eq!(result.events.len(), 2);
    }

    #[test]
    fn append_nodes_rejects_duplicate_ids_in_input() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Two nodes in the same input share an ID
        let mut dup = valid_node("a");
        dup.title = "First A".to_string();
        let mut dup2 = valid_node("a");
        dup2.title = "Second A".to_string();
        let new_nodes = vec![dup, dup2];
        let result = append_nodes(tmp.path(), 0, new_nodes);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::DuplicateNodeId { id } => assert_eq!(id, "a"),
            other => panic!("Expected DuplicateNodeId, got: {:?}", other),
        }
    }

    #[test]
    fn append_nodes_refuses_desynced_event_log() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Write graph with revision 5 but empty event log (max rev = 0) -> desync
        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.graph_revision = 5;
        io::write_graph(tmp.path(), &graph).unwrap();

        let result = append_nodes(tmp.path(), 5, vec![valid_node("a")]);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::EventLogDesync => {} // OK
            other => panic!("Expected EventLogDesync, got: {:?}", other),
        }
    }

    #[test]
    fn append_nodes_preflight_reconciles_then_revision_gates() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Write a graph where reconciliation fires:
        // IN_PROGRESS node with expired lease, attempts < max → back to READY
        let mut graph = io::read_graph(tmp.path()).unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.attempts = 1;
        node.max_attempts = 3;
        node.lease = crate::model::Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2026-05-17T00:01:00Z".to_string()), // in the past
        };
        graph.nodes.push(node);
        io::write_graph(tmp.path(), &graph).unwrap();

        // Preflight should reconcile (rev becomes > 0 after persist)
        // Then stale check: request rev 0 vs reconciled rev → STALE_REVISION
        let result = append_nodes(tmp.path(), 0, vec![valid_node("b")]);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::StaleRevision { expected, provided } => {
                // expected should be 1 (0 + 1 lease-expiry event)
                assert_eq!(
                    expected, 1,
                    "Expected reconciled revision 1 after lease expiry"
                );
                assert_eq!(provided, 0);
            }
            other => panic!("Expected StaleRevision, got: {:?}", other),
        }
    }
}
