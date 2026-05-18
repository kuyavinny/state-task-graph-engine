use crate::error::AppError;
use crate::io;
use crate::model::Status;
use crate::reconcile;
use crate::response::ResponseEnvelope;
use crate::validate;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "stg", about = "State & Task Graph Engine")]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new empty graph and event log
    Init,
    /// Run schema and cycle validation checks
    Validate,
    /// Return the high-level progress of the graph
    Status,
    /// Return the highest-priority READY task
    Next,
    /// Lock a task with a lease and worker ID
    Claim {
        /// Node ID to claim
        node_id: String,
        /// Actor claiming the task
        actor: String,
        /// Lease time-to-live in seconds
        #[arg(long)]
        ttl_seconds: u64,
    },
    /// Extend an active lease
    Heartbeat {
        /// Node ID
        node_id: String,
        /// Actor extending the lease
        actor: String,
        /// Additional TTL in seconds
        #[arg(long)]
        ttl_seconds: u64,
    },
    /// Release a claimed task back to READY
    Release {
        /// Node ID
        node_id: String,
        /// Actor releasing the task
        actor: String,
    },
    /// Mark an active task as completed
    Complete {
        /// Node ID
        node_id: String,
        /// Actor completing the task
        actor: String,
        /// Brief outcome description
        #[arg(long)]
        result_summary: String,
    },
    /// Mark an active task as failed
    Fail {
        /// Node ID
        node_id: String,
        /// Actor reporting failure
        actor: String,
        /// Failure reason
        #[arg(long)]
        failure_reason: String,
    },
    /// Mark an active task as blocked
    Block {
        /// Node ID
        node_id: String,
        /// Actor blocking the task
        actor: String,
        /// Reason for blocking
        #[arg(long)]
        blocked_reason: String,
    },
    /// Intentionally bypass a task
    Skip {
        /// Node ID
        node_id: String,
        /// Actor skipping the task
        actor: String,
        /// Reason for skipping
        #[arg(long)]
        skip_reason: String,
    },
    /// Cancel a task
    Cancel {
        /// Node ID
        node_id: String,
        /// Actor cancelling the task
        actor: String,
        /// Reason for cancellation
        #[arg(long)]
        cancel_reason: String,
    },
    /// Reset a terminal state back to PENDING or READY
    Reopen {
        /// Node ID
        node_id: String,
        /// Actor reopening the task
        actor: String,
    },
    /// Add new tasks dynamically from a file
    AppendNodes {
        /// Current graph revision for optimistic concurrency
        #[arg(long)]
        revision: u64,
        /// Path to YAML file containing new nodes
        #[arg(long)]
        file: String,
    },
    /// Generate a bounded context payload for an LLM
    Summarize {
        /// Node ID to summarize around
        node_id: String,
        /// Maximum number of recent events to include
        #[arg(long, default_value = "10")]
        max_events: usize,
        /// Maximum number of completed summaries to include
        #[arg(long, default_value = "5")]
        max_completed_summaries: usize,
        /// Whether to include blocked/failed related nodes
        #[arg(long, default_value = "true")]
        include_blocked: bool,
    },
}

impl Cli {
    pub fn run(self) -> Result<(), AppError> {
        match self.command {
            Commands::Init => {
                let dir = std::env::current_dir()?;
                io::init_graph(&dir)?;
                let envelope: ResponseEnvelope<serde_json::Value> =
                    ResponseEnvelope::ok(0, serde_json::json!({"initialized": true}));
                println!("{}", serde_json::to_string_pretty(&envelope)?);
                Ok(())
            }
            // Remaining commands are stubs for later PRs
            Commands::Validate => {
                let dir = std::env::current_dir()?;
                let graph = io::read_graph(&dir)?;
                let validation_errors = validate::validate_graph(&graph);
                if validation_errors.is_empty() {
                    let envelope: ResponseEnvelope<serde_json::Value> = ResponseEnvelope::ok(
                        graph.graph_revision,
                        serde_json::json!({"valid": true}),
                    );
                    println!("{}", serde_json::to_string_pretty(&envelope)?);
                    Ok(())
                } else {
                    let count = validation_errors.len();
                    Err(AppError::GraphValidationFailed {
                        count,
                        errors: validation_errors,
                    })
                }
            }
            Commands::Status => {
                let dir = std::env::current_dir()?;
                let result = reconcile::load_validate_reconcile(&dir)?;

                let mut counts = std::collections::HashMap::new();
                for node in &result.graph.nodes {
                    let status_str = node.status.to_string();
                    *counts.entry(status_str).or_insert(0) += 1;
                }

                let warnings: Vec<String> = result
                    .warnings
                    .iter()
                    .map(|w| match w {
                        reconcile::ReconciliationWarning::EventLogDesync {
                            graph_revision,
                            event_log_revision,
                        } => format!(
                            "EVENT_LOG_DESYNC: Graph revision {} does not match event log revision {}",
                            graph_revision, event_log_revision
                        ),
                    })
                    .collect();

                let data = serde_json::json!({
                    "revision": result.graph.graph_revision,
                    "node_count": result.graph.nodes.len(),
                    "status": counts,
                });

                let envelope: ResponseEnvelope<serde_json::Value> =
                    ResponseEnvelope::ok_with_warnings(result.graph.graph_revision, data, warnings);
                println!("{}", serde_json::to_string_pretty(&envelope)?);
                Ok(())
            }
            Commands::Next => {
                let dir = std::env::current_dir()?;
                let result = reconcile::load_validate_reconcile(&dir)?;

                // Find highest-priority READY node
                let next_task = result
                    .graph
                    .nodes
                    .iter()
                    .filter(|n| n.status == Status::Ready)
                    .min_by(|a, b| {
                        // priority descending, then created_at ascending, then id ascending
                        b.priority
                            .cmp(&a.priority)
                            .then(a.created_at.cmp(&b.created_at))
                            .then(a.id.cmp(&b.id))
                    });

                let data = match next_task {
                    Some(node) => serde_json::json!({
                        "id": node.id,
                        "title": node.title,
                        "priority": node.priority,
                        "status": node.status.to_string(),
                    }),
                    None => serde_json::json!({
                        "message": "No READY tasks available",
                    }),
                };

                let envelope: ResponseEnvelope<serde_json::Value> =
                    ResponseEnvelope::ok(result.graph.graph_revision, data);
                println!("{}", serde_json::to_string_pretty(&envelope)?);
                Ok(())
            }
            Commands::Claim { .. } => Err(AppError::NotImplemented("claim".into())),
            Commands::Heartbeat { .. } => Err(AppError::NotImplemented("heartbeat".into())),
            Commands::Release { .. } => Err(AppError::NotImplemented("release".into())),
            Commands::Complete { .. } => Err(AppError::NotImplemented("complete".into())),
            Commands::Fail { .. } => Err(AppError::NotImplemented("fail".into())),
            Commands::Block { .. } => Err(AppError::NotImplemented("block".into())),
            Commands::Skip { .. } => Err(AppError::NotImplemented("skip".into())),
            Commands::Cancel { .. } => Err(AppError::NotImplemented("cancel".into())),
            Commands::Reopen { .. } => Err(AppError::NotImplemented("reopen".into())),
            Commands::AppendNodes { .. } => Err(AppError::NotImplemented("append-nodes".into())),
            Commands::Summarize { .. } => Err(AppError::NotImplemented("summarize".into())),
        }
    }
}
