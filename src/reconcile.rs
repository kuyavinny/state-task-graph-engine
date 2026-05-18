use crate::error::AppError;
use crate::io;
use crate::model::{Event, EventAction, Graph, Status};
use crate::validate;
use std::path::Path;

/// Result of a reconciliation pass.
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

        // Write graph atomically
        io::write_graph(project_dir, &graph)?;

        // Append all events in a single batch
        io::append_events_batch(project_dir, &events)?;
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
}
