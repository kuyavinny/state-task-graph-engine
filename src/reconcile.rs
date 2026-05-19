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

impl std::fmt::Display for ReconciliationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconciliationWarning::EventLogDesync {
                graph_revision,
                event_log_revision,
            } => write!(
                f,
                "EVENT_LOG_DESYNC: Graph revision {} does not match event log revision {}",
                graph_revision, event_log_revision
            ),
        }
    }
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
        // Normalize lease: if any lease field is Some, keep them;
        // if all three are None, set empty lease
        if node.lease.claimed_by.is_none()
            && node.lease.claimed_at.is_none()
            && node.lease.expires_at.is_none()
        {
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

// ---------------------------------------------------------------------------
// State mutation commands (PR#5)
// ---------------------------------------------------------------------------

/// Find a node by ID in the graph, returning TASK_NOT_FOUND if absent.
fn find_node_mut<'a>(
    graph: &'a mut Graph,
    node_id: &str,
) -> Result<&'a mut crate::model::Node, AppError> {
    let idx = graph
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| AppError::TaskNotFound {
            id: node_id.to_string(),
        })?;
    Ok(&mut graph.nodes[idx])
}

/// Common preflight + persist for state mutation commands.
/// Returns the preflighted graph and an empty events vec.
fn preflight_mutation(project_dir: &Path, node_id: &str) -> Result<(Graph, Vec<Event>), AppError> {
    let preflight = load_validate_reconcile(project_dir)?;
    if !preflight.warnings.is_empty() {
        return Err(AppError::EventLogDesync);
    }
    // Verify node exists
    find_node_mut(&mut preflight.graph.clone(), node_id)?;
    // Return empty events vec — preflight events were already persisted by load_validate_reconcile.
    // The command's new events are appended separately and written once by persist_mutation.
    Ok((preflight.graph, Vec::new()))
}

/// Persist graph + events after a successful mutation.
fn persist_mutation(
    project_dir: &Path,
    graph: &mut Graph,
    events: &mut Vec<Event>,
) -> Result<(), AppError> {
    // Run reconciliation (lease expiry, PENDING → READY promotion)
    reconcile_lazy_leases(graph, events);
    reconcile_pending_to_ready(graph, events);

    let new_revision = graph.graph_revision + events.len() as u64;
    graph.graph_revision = new_revision;
    io::append_events_batch(project_dir, events)?;
    io::write_graph(project_dir, graph)?;
    Ok(())
}

/// Lock a READY task with a lease.
pub fn claim(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    ttl_seconds: u64,
) -> Result<ReconciliationResult, AppError> {
    if ttl_seconds == 0 {
        return Err(AppError::InvalidArgument {
            message: "ttl_seconds must be > 0".to_string(),
        });
    }
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;

    // Find node position for dependency check (no mutable borrow yet)
    let node_idx = graph
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| AppError::TaskNotFound {
            id: node_id.to_string(),
        })?;

    // Dependency-check early-exit before acquiring mutable borrow
    if graph.nodes[node_idx].status == crate::model::Status::Pending {
        let deps = graph.nodes[node_idx].dependencies.clone();
        let unresolved: Vec<String> = deps
            .into_iter()
            .filter(|dep| {
                !graph.nodes.iter().any(|n| {
                    n.id == *dep
                        && matches!(
                            n.status,
                            crate::model::Status::Completed | crate::model::Status::Skipped
                        )
                })
            })
            .collect();
        if !unresolved.is_empty() {
            return Err(AppError::InvalidTransition {
                action: "claim".to_string(),
                current_status: format!("PENDING (waiting for dependencies: {:?})", unresolved),
            });
        }
    }

    let node = find_node_mut(&mut graph, node_id)?;

    if node.status != crate::model::Status::Ready {
        return Err(AppError::InvalidTransition {
            action: "claim".to_string(),
            current_status: node.status.to_string(),
        });
    }

    let now = chrono::Utc::now();
    let now_rfc3339 = now.to_rfc3339();
    let expires_rfc3339 = (now + chrono::Duration::seconds(ttl_seconds as i64)).to_rfc3339();

    let old_status = node.status;
    node.status = crate::model::Status::InProgress;
    node.lease = crate::model::Lease {
        claimed_by: Some(actor.to_string()),
        claimed_at: Some(now_rfc3339.clone()),
        expires_at: Some(expires_rfc3339),
    };
    node.attempts += 1;
    node.updated_at = now_rfc3339.clone();

    events.push(make_event(
        node_id,
        actor,
        crate::model::EventAction::Claim,
        Some(old_status),
        Some(crate::model::Status::InProgress),
        None,
        graph.graph_revision + events.len() as u64 + 1,
    ));

    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Extend a lease on an IN_PROGRESS node.
pub fn heartbeat(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    ttl_seconds: u64,
) -> Result<ReconciliationResult, AppError> {
    if ttl_seconds == 0 {
        return Err(AppError::InvalidArgument {
            message: "ttl_seconds must be > 0".to_string(),
        });
    }
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;
    let node = find_node_mut(&mut graph, node_id)?;

    if node.status != crate::model::Status::InProgress {
        return Err(AppError::InvalidTransition {
            action: "heartbeat".to_string(),
            current_status: node.status.to_string(),
        });
    }

    // Lease ownership check
    if node.lease.claimed_by.as_deref() != Some(actor) {
        return Err(AppError::LeaseNotOwned);
    }

    let now = chrono::Utc::now();
    let now_rfc3339 = now.to_rfc3339();
    let expires_rfc3339 = (now + chrono::Duration::seconds(ttl_seconds as i64)).to_rfc3339();

    node.lease.expires_at = Some(expires_rfc3339);
    node.lease.claimed_at = Some(now_rfc3339.clone());
    node.updated_at = now_rfc3339.clone();

    events.push(make_event(
        node_id,
        actor,
        crate::model::EventAction::Heartbeat,
        Some(crate::model::Status::InProgress),
        Some(crate::model::Status::InProgress),
        Some(format!("Lease extended by {}s", ttl_seconds)),
        graph.graph_revision + events.len() as u64 + 1,
    ));

    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Release a claimed task back to READY.
pub fn release(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
) -> Result<ReconciliationResult, AppError> {
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;
    let node = find_node_mut(&mut graph, node_id)?;

    if node.status != crate::model::Status::InProgress {
        return Err(AppError::InvalidTransition {
            action: "release".to_string(),
            current_status: node.status.to_string(),
        });
    }

    // Lease ownership check
    if node.lease.claimed_by.as_deref() != Some(actor) {
        return Err(AppError::LeaseNotOwned);
    }

    let old_status = node.status;
    node.status = crate::model::Status::Ready;
    node.lease = crate::model::Lease::empty();
    node.updated_at = chrono::Utc::now().to_rfc3339();

    events.push(make_event(
        node_id,
        actor,
        crate::model::EventAction::Release,
        Some(old_status),
        Some(crate::model::Status::Ready),
        None,
        graph.graph_revision + events.len() as u64 + 1,
    ));

    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Internal helper for status transition commands with lease and revision checks.
#[allow(clippy::too_many_arguments)]
fn apply_simple_transition(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
    new_status: crate::model::Status,
    valid_from_statuses: &[crate::model::Status],
    action: crate::model::EventAction,
    check_lease: bool,
    reason: Option<String>,
) -> Result<ReconciliationResult, AppError> {
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;

    // Revision check
    if graph.graph_revision != revision {
        return Err(AppError::StaleRevision {
            expected: graph.graph_revision,
            provided: revision,
        });
    }

    let node = find_node_mut(&mut graph, node_id)?;
    let old_status = node.status;

    // Lease ownership check first — if the actor's lease expired during preflight
    // reconciliation, this gives LeaseNotOwned instead of InvalidTransition,
    // which is more actionable.
    if check_lease && node.lease.claimed_by.as_deref() != Some(actor) {
        return Err(AppError::LeaseNotOwned);
    }

    // Validate from-status
    if !valid_from_statuses.contains(&old_status) {
        return Err(AppError::InvalidTransition {
            action: format!("{:?}", action),
            current_status: old_status.to_string(),
        });
    }

    let now_rfc3339 = chrono::Utc::now().to_rfc3339();
    node.status = new_status;
    node.updated_at = now_rfc3339.clone();
    node.lease = crate::model::Lease::empty();

    // Set reason field based on target status
    match action {
        crate::model::EventAction::Complete => {
            node.result_summary = reason.clone();
        }
        crate::model::EventAction::Fail => {
            node.failure_reason = reason.clone();
        }
        crate::model::EventAction::Block => {
            node.blocked_reason = reason.clone();
        }
        crate::model::EventAction::Skip => {
            node.skip_reason = reason.clone();
        }
        crate::model::EventAction::Cancel => {
            node.cancel_reason = reason.clone();
        }
        _ => {}
    }

    events.push(make_event(
        node_id,
        actor,
        action,
        Some(old_status),
        Some(node.status),
        reason,
        graph.graph_revision + events.len() as u64 + 1,
    ));

    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Mark an active task as completed.
pub fn complete(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
    result_summary: String,
) -> Result<ReconciliationResult, AppError> {
    apply_simple_transition(
        project_dir,
        node_id,
        actor,
        revision,
        crate::model::Status::Completed,
        &[crate::model::Status::InProgress],
        crate::model::EventAction::Complete,
        true,
        Some(result_summary),
    )
}

/// Mark an active task as failed.
pub fn fail(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
    failure_reason: String,
) -> Result<ReconciliationResult, AppError> {
    apply_simple_transition(
        project_dir,
        node_id,
        actor,
        revision,
        crate::model::Status::Failed,
        &[crate::model::Status::InProgress],
        crate::model::EventAction::Fail,
        true,
        Some(failure_reason),
    )
}

/// Mark an active task as blocked.
pub fn block(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
    blocked_reason: String,
) -> Result<ReconciliationResult, AppError> {
    apply_simple_transition(
        project_dir,
        node_id,
        actor,
        revision,
        crate::model::Status::Blocked,
        &[crate::model::Status::InProgress],
        crate::model::EventAction::Block,
        true,
        Some(blocked_reason),
    )
}

/// Intentionally bypass a task.
pub fn skip(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
    skip_reason: String,
) -> Result<ReconciliationResult, AppError> {
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;

    // Revision check
    if graph.graph_revision != revision {
        return Err(AppError::StaleRevision {
            expected: graph.graph_revision,
            provided: revision,
        });
    }

    let node = find_node_mut(&mut graph, node_id)?;
    let old_status = node.status;

    // Valid from-statuses: PENDING, READY, BLOCKED, or IN_PROGRESS (with lease check)
    let valid = [
        crate::model::Status::Pending,
        crate::model::Status::Ready,
        crate::model::Status::Blocked,
        crate::model::Status::InProgress,
    ];
    if !valid.contains(&old_status) {
        return Err(AppError::InvalidTransition {
            action: "skip".to_string(),
            current_status: old_status.to_string(),
        });
    }

    // If IN_PROGRESS and leased to another actor → LEASE_NOT_OWNED
    if old_status == crate::model::Status::InProgress
        && node.lease.claimed_by.as_deref() != Some(actor)
    {
        return Err(AppError::LeaseNotOwned);
    }

    let now_rfc3339 = chrono::Utc::now().to_rfc3339();
    node.status = crate::model::Status::Skipped;
    node.skip_reason = Some(skip_reason.clone());
    node.updated_at = now_rfc3339.clone();
    node.lease = crate::model::Lease::empty();

    events.push(make_event(
        node_id,
        actor,
        crate::model::EventAction::Skip,
        Some(old_status),
        Some(crate::model::Status::Skipped),
        Some(skip_reason),
        graph.graph_revision + events.len() as u64 + 1,
    ));

    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Cancel a task from any non-terminal state.
pub fn cancel(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
    cancel_reason: String,
) -> Result<ReconciliationResult, AppError> {
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;

    // Revision check
    if graph.graph_revision != revision {
        return Err(AppError::StaleRevision {
            expected: graph.graph_revision,
            provided: revision,
        });
    }

    let node = find_node_mut(&mut graph, node_id)?;
    let old_status = node.status;

    // Can cancel any non-terminal state
    let terminal = [
        crate::model::Status::Completed,
        crate::model::Status::Failed,
        crate::model::Status::Cancelled,
        crate::model::Status::Skipped,
    ];
    if terminal.contains(&old_status) {
        return Err(AppError::InvalidTransition {
            action: "cancel".to_string(),
            current_status: old_status.to_string(),
        });
    }

    // If IN_PROGRESS and leased to another actor → LEASE_NOT_OWNED
    if old_status == crate::model::Status::InProgress
        && node.lease.claimed_by.as_deref() != Some(actor)
    {
        return Err(AppError::LeaseNotOwned);
    }

    let now_rfc3339 = chrono::Utc::now().to_rfc3339();
    node.status = crate::model::Status::Cancelled;
    node.cancel_reason = Some(cancel_reason.clone());
    node.updated_at = now_rfc3339.clone();
    node.lease = crate::model::Lease::empty();

    events.push(make_event(
        node_id,
        actor,
        crate::model::EventAction::Cancel,
        Some(old_status),
        Some(crate::model::Status::Cancelled),
        Some(cancel_reason),
        graph.graph_revision + events.len() as u64 + 1,
    ));

    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Reset a terminal or BLOCKED state back to PENDING or READY.
pub fn reopen(
    project_dir: &Path,
    node_id: &str,
    actor: &str,
    revision: u64,
) -> Result<ReconciliationResult, AppError> {
    let (mut graph, mut events) = preflight_mutation(project_dir, node_id)?;

    // Revision check
    if graph.graph_revision != revision {
        return Err(AppError::StaleRevision {
            expected: graph.graph_revision,
            provided: revision,
        });
    }

    let node = find_node_mut(&mut graph, node_id)?;
    let old_status = node.status;

    // Valid from-statuses: terminal states (COMPLETED, FAILED, CANCELLED, SKIPPED) or BLOCKED
    let valid = [
        crate::model::Status::Completed,
        crate::model::Status::Failed,
        crate::model::Status::Cancelled,
        crate::model::Status::Skipped,
        crate::model::Status::Blocked,
    ];
    if !valid.contains(&old_status) {
        return Err(AppError::InvalidTransition {
            action: "reopen".to_string(),
            current_status: old_status.to_string(),
        });
    }

    let now_rfc3339 = chrono::Utc::now().to_rfc3339();

    // Clear terminal state fields
    node.result_summary = None;
    node.failure_reason = None;
    node.blocked_reason = None;
    node.skip_reason = None;
    node.cancel_reason = None;
    node.lease = crate::model::Lease::empty();

    // Set to PENDING initially; reconciliation below will promote to READY if applicable
    node.status = crate::model::Status::Pending;
    node.updated_at = now_rfc3339.clone();

    events.push(make_event(
        node_id,
        actor,
        crate::model::EventAction::Reopen,
        Some(old_status),
        Some(crate::model::Status::Pending),
        Some("Task reopened".to_string()),
        graph.graph_revision + events.len() as u64 + 1,
    ));

    // persist_mutation runs reconciliation which promotes PENDING→READY if deps met
    persist_mutation(project_dir, &mut graph, &mut events)?;
    Ok(ReconciliationResult {
        graph,
        events,
        warnings: Vec::new(),
    })
}

/// Bounded context payload for LLM integration.
/// Returns only the data needed to avoid context bloat.
pub fn summarize(
    graph: &Graph,
    events: &[Event],
    node_id: &str,
    max_events: usize,
    max_completed_summaries: usize,
    include_blocked: bool,
) -> Result<serde_json::Value, AppError> {
    let active_task = graph
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| AppError::TaskNotFound {
            id: node_id.to_string(),
        })?;

    let mut parent_chain = Vec::new();
    let mut current_parent = active_task.parent_id.as_deref();
    while let Some(parent_id) = current_parent {
        if let Some(parent) = graph.nodes.iter().find(|n| n.id == parent_id) {
            parent_chain.push(serde_json::json!({"id": parent.id, "title": parent.title}));
            current_parent = parent.parent_id.as_deref();
        } else {
            break;
        }
    }
    parent_chain.reverse();

    let mut immediate_dependencies = Vec::new();
    for dep_id in &active_task.dependencies {
        if let Some(dep) = graph.nodes.iter().find(|n| n.id == *dep_id) {
            immediate_dependencies.push(serde_json::json!({
                "id": dep.id,
                "status": dep.status,
                "result_summary": if matches!(dep.status, Status::Completed | Status::Skipped) {
                    dep.result_summary.clone()
                } else {
                    None
                },
            }));
        }
    }

    let dependent_tasks: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.dependencies.iter().any(|d| d == node_id))
        .map(|n| serde_json::json!({"id": n.id, "title": n.title}))
        .collect();

    let mut blocked_or_failed_related = Vec::new();
    if include_blocked {
        for node in &graph.nodes {
            if node.id != node_id && matches!(node.status, Status::Blocked | Status::Failed) {
                let reason = if node.status == Status::Blocked {
                    node.blocked_reason.clone()
                } else {
                    node.failure_reason.clone()
                };
                blocked_or_failed_related.push(serde_json::json!({
                    "id": node.id,
                    "status": node.status,
                    "reason": reason,
                }));
            }
        }
    }

    let recent_events: Vec<_> = events
        .iter()
        .filter(|e| e.node_id == node_id)
        .rev()
        .take(max_events)
        .map(|e| {
            serde_json::json!({
                "timestamp": e.timestamp,
                "action": e.action,
                "reason": e.reason,
            })
        })
        .collect();

    let mut completed_summaries: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.status, Status::Completed | Status::Skipped))
        .filter_map(|n| {
            n.result_summary.as_ref().map(|summary| {
                serde_json::json!({
                    "id": n.id,
                    "title": n.title,
                    "result_summary": summary,
                })
            })
        })
        .collect();
    completed_summaries.reverse();
    completed_summaries.truncate(max_completed_summaries);

    let operator_notes = None::<String>;

    Ok(serde_json::json!({
        "active_task": {
            "id": active_task.id,
            "title": active_task.title,
            "description": active_task.description,
            "data": active_task.data,
        },
        "parent_chain": parent_chain,
        "immediate_dependencies": immediate_dependencies,
        "dependent_tasks": dependent_tasks,
        "blocked_or_failed_related": blocked_or_failed_related,
        "recent_events": recent_events,
        "completed_summaries": completed_summaries,
        "operator_notes": operator_notes,
    }))
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

    // -----------------------------------------------------------------------
    // PR#5: State command unit tests
    // -----------------------------------------------------------------------

    fn setup_graph_with_node(tmp: &tempfile::TempDir, node: &Node) {
        io::init_graph(tmp.path()).unwrap();
        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.nodes.push(node.clone());
        io::write_graph(tmp.path(), &graph).unwrap();
    }

    #[test]
    fn claim_ready_node_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        setup_graph_with_node(&tmp, &valid_node("a"));

        let result = claim(tmp.path(), "a", "worker-1", 300).unwrap();
        assert_eq!(result.graph.nodes.len(), 1);
        let node = &result.graph.nodes[0];
        assert_eq!(node.status, Status::InProgress);
        assert_eq!(node.attempts, 1);
        assert_eq!(node.lease.claimed_by.as_deref(), Some("worker-1"));
        assert!(node.lease.claimed_at.is_some());
        assert!(node.lease.expires_at.is_some());
    }

    #[test]
    fn claim_invalid_transition_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = claim(tmp.path(), "a", "worker-1", 300);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::InvalidTransition { action, .. } => {
                assert_eq!(action, "claim");
            }
            other => panic!("Expected InvalidTransition, got: {:?}", other),
        }
    }

    #[test]
    fn claim_non_existent_node_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        let result = claim(tmp.path(), "nonexistent", "worker-1", 300);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::TaskNotFound { id } => {
                assert_eq!(id, "nonexistent");
            }
            other => panic!("Expected TaskNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn heartbeat_extends_lease() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = heartbeat(tmp.path(), "a", "worker-1", 600).unwrap();
        let node = &result.graph.nodes[0];
        assert_eq!(node.status, Status::InProgress);
        assert!(node.lease.expires_at.is_some());
    }

    #[test]
    fn heartbeat_non_owner_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = heartbeat(tmp.path(), "a", "worker-2", 600);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::LeaseNotOwned));
    }

    #[test]
    fn release_reverts_to_ready() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = release(tmp.path(), "a", "worker-1").unwrap();
        let node = &result.graph.nodes[0];
        assert_eq!(node.status, Status::Ready);
        assert!(node.lease.claimed_by.is_none());
    }

    #[test]
    fn release_non_owner_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = release(tmp.path(), "a", "worker-2");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::LeaseNotOwned));
    }

    #[test]
    fn complete_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = complete(tmp.path(), "a", "worker-1", 0, "All done".to_string()).unwrap();
        let n = &result.graph.nodes[0];
        assert_eq!(n.status, Status::Completed);
        assert_eq!(n.result_summary.as_deref(), Some("All done"));
    }

    #[test]
    fn complete_non_owner_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = complete(tmp.path(), "a", "worker-2", 0, "nope".to_string());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::LeaseNotOwned));
    }

    #[test]
    fn fail_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = fail(
            tmp.path(),
            "a",
            "worker-1",
            0,
            "Something broke".to_string(),
        )
        .unwrap();
        let n = &result.graph.nodes[0];
        assert_eq!(n.status, Status::Failed);
        assert_eq!(n.failure_reason.as_deref(), Some("Something broke"));
    }

    #[test]
    fn block_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = block(tmp.path(), "a", "worker-1", 0, "Waiting on dep".to_string()).unwrap();
        let n = &result.graph.nodes[0];
        assert_eq!(n.status, Status::Blocked);
        assert_eq!(n.blocked_reason.as_deref(), Some("Waiting on dep"));
    }

    #[test]
    fn skip_pending_or_ready_or_blocked_succeeds() {
        // Skip PENDING (gets promoted by preflight, tested via dep-gated case)
        for from_status in [Status::Ready, Status::Blocked] {
            let tmp = tempfile::tempdir().unwrap();
            let mut node = valid_node("a");
            node.status = from_status;
            if from_status == Status::Blocked {
                node.blocked_reason = Some("External blocker".to_string());
            }
            setup_graph_with_node(&tmp, &node);

            let result = skip(tmp.path(), "a", "worker-1", 0, "Not needed".to_string()).unwrap();
            let n = &result.graph.nodes[0];
            assert_eq!(n.status, Status::Skipped);
            assert_eq!(n.skip_reason.as_deref(), Some("Not needed"));
        }
    }

    #[test]
    fn skip_in_progress_owned_by_other_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = skip(tmp.path(), "a", "worker-2", 0, "skip".to_string());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::LeaseNotOwned));
    }

    #[test]
    fn cancel_non_terminal_succeeds() {
        // Skip PENDING (gets promoted by preflight)
        for from_status in [Status::Ready, Status::InProgress, Status::Blocked] {
            let tmp = tempfile::tempdir().unwrap();
            let mut node = valid_node("a");
            node.status = from_status;
            if from_status == Status::Blocked {
                node.blocked_reason = Some("External blocker".to_string());
            }
            if from_status == Status::InProgress {
                node.lease = Lease {
                    claimed_by: Some("worker-1".to_string()),
                    claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
                    expires_at: Some("2099-12-31T23:59:59Z".to_string()),
                };
            }
            setup_graph_with_node(&tmp, &node);

            let result = cancel(tmp.path(), "a", "worker-1", 0, "Cancelled".to_string()).unwrap();
            let n = &result.graph.nodes[0];
            assert_eq!(n.status, Status::Cancelled);
            assert_eq!(n.cancel_reason.as_deref(), Some("Cancelled"));
        }
    }

    #[test]
    fn cancel_terminal_state_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::Completed;
        node.result_summary = Some("Done".to_string());
        setup_graph_with_node(&tmp, &node);

        let result = cancel(tmp.path(), "a", "worker-1", 0, "nope".to_string());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppError::InvalidTransition { .. }
        ));
    }

    #[test]
    fn reopen_terminal_state_succeeds() {
        for from_status in [
            Status::Completed,
            Status::Failed,
            Status::Cancelled,
            Status::Skipped,
            Status::Blocked,
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut node = valid_node("a");
            node.status = from_status;
            if from_status == Status::Completed {
                node.result_summary = Some("Done".to_string());
            }
            if from_status == Status::Failed {
                node.failure_reason = Some("Failed".to_string());
            }
            if from_status == Status::Cancelled {
                node.cancel_reason = Some("Cancelled".to_string());
            }
            if from_status == Status::Skipped {
                node.skip_reason = Some("Skipped".to_string());
            }
            if from_status == Status::Blocked {
                node.blocked_reason = Some("Blocked".to_string());
            }
            setup_graph_with_node(&tmp, &node);

            let result = reopen(tmp.path(), "a", "worker-1", 0).unwrap();
            let n = &result.graph.nodes[0];
            // Should be PENDING or READY after reconciliation
            assert!(
                n.status == Status::Pending || n.status == Status::Ready,
                "Expected PENDING or READY after reopen, got {:?}",
                n.status
            );
            // Terminal state fields should be cleared
            assert!(n.result_summary.is_none());
            assert!(n.failure_reason.is_none());
            assert!(n.blocked_reason.is_none());
            assert!(n.skip_reason.is_none());
            assert!(n.cancel_reason.is_none());
        }
    }

    #[test]
    fn reopen_non_terminal_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::Ready;
        setup_graph_with_node(&tmp, &node);

        let result = reopen(tmp.path(), "a", "worker-1", 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppError::InvalidTransition { .. }
        ));
    }

    #[test]
    fn claim_does_not_duplicate_preflight_events() {
        // Regression: preflight_mutation() must not return preflight.events,
        // otherwise persist_mutation would write already-persisted events again.
        // Test: claim on a PENDING node that gets promoted to READY by preflight.
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        // Node with dep on itself — can't use self-dep, make a two-node graph
        let mut a = valid_node("a");
        a.status = Status::Pending;
        a.dependencies = vec!["b".to_string()];
        let mut b = valid_node("b");
        b.status = Status::Completed;
        b.result_summary = Some("Done".to_string());

        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.nodes.push(a);
        graph.nodes.push(b);
        io::write_graph(tmp.path(), &graph).unwrap();

        // Claim "a" — preflight promotes PENDING→READY (1 event),
        // then claim adds 1 more event. Total should be 2.
        let result = claim(tmp.path(), "a", "worker-1", 300).unwrap();
        assert_eq!(result.graph.graph_revision, 2);

        let events_path = tmp.path().join(".agent").join("task_events.jsonl");
        let content = std::fs::read_to_string(&events_path).unwrap();
        let event_count = content.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(
            event_count, 3,
            "Expected exactly 3 events (1 init + 1 preflight + 1 claim), got {}: {:?}",
            event_count, content
        );
    }

    #[test]
    fn claim_rejects_zero_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::Ready;
        setup_graph_with_node(&tmp, &node);

        let result = claim(tmp.path(), "a", "worker-1", 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn heartbeat_rejects_zero_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        let mut node = valid_node("a");
        node.status = Status::InProgress;
        node.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2099-12-31T23:59:59Z".to_string()),
        };
        setup_graph_with_node(&tmp, &node);

        let result = heartbeat(tmp.path(), "a", "worker-1", 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn complete_returns_lease_not_owned_when_lease_expired_during_preflight() {
        // Regression: I3 fix swapped lease-before-status check.
        // When preflight reconciliation expires a lease (IN_PROGRESS→READY),
        // complete() should return LeaseNotOwned, not InvalidTransition.
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        let mut a = valid_node("a");
        a.status = Status::InProgress;
        a.lease = Lease {
            claimed_by: Some("worker-1".to_string()),
            claimed_at: Some("2026-05-17T00:00:00Z".to_string()),
            expires_at: Some("2020-01-01T00:00:00Z".to_string()), // already expired
        };

        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.nodes.push(a);
        io::write_graph(tmp.path(), &graph).unwrap();

        // Preflight reconciliation will expire the lease, reverting to READY.
        // Claim attempts increment but max_attempts is 3 so won't fail yet.
        let result = complete(tmp.path(), "a", "worker-1", 1, "Done".to_string());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, AppError::LeaseNotOwned),
            "Expected LeaseNotOwned when lease expired during preflight, got {:?}",
            err
        );
    }

    #[test]
    fn claim_pending_with_unresolved_deps_shows_dependency_info() {
        // Regression: I5 fix enriches claim error with dependency info.
        let tmp = tempfile::tempdir().unwrap();
        io::init_graph(tmp.path()).unwrap();

        let mut a = valid_node("a");
        a.status = Status::Pending;
        a.dependencies = vec!["b".to_string()];
        let mut b = valid_node("b");
        b.status = Status::Pending; // not completed/skipped
        b.dependencies = vec![];

        let mut graph = io::read_graph(tmp.path()).unwrap();
        graph.nodes.push(a);
        graph.nodes.push(b);
        io::write_graph(tmp.path(), &graph).unwrap();

        let result = claim(tmp.path(), "a", "worker-1", 300);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            AppError::InvalidTransition { current_status, .. } => {
                assert!(
                    current_status.contains("b"),
                    "Expected error to mention dependency 'b', got: {}",
                    current_status
                );
            }
            other => panic!("Expected InvalidTransition, got {:?}", other),
        }
    }
}
