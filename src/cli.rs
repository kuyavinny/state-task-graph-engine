use crate::error::AppError;
use crate::io;
use crate::response::ResponseEnvelope;

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
            Commands::Validate => Err(AppError::NotImplemented("validate".into())),
            Commands::Status => Err(AppError::NotImplemented("status".into())),
            Commands::Next => Err(AppError::NotImplemented("next".into())),
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
