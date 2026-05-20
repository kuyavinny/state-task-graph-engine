use clap::{Parser, Subcommand};

/// Module 3: Workflow & Harness Controller
///
/// Orchestrates structured, repeatable workflow harnesses over
/// Module 1 (State & Task Graph Engine) and Module 2 (Universal Adapter Boundary).
#[derive(Parser)]
#[command(name = "agent-workflow")]
#[command(version)]
#[command(about = "Workflow & Harness Controller for agent-system-os")]
#[command(
    long_about = "Module 3: Single-agent supervised control-plane that loads \
    declarative workflow definitions, manages durable per-run state, enforces \
    phase entry/exit criteria and approval gates, and routes all task mutations \
    through Module 2 (agent-adapter)."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Supported CLI subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new workflow run from a definition file.
    InitRun {
        /// Workflow definition ID (matches `.agent/workflows/<id>.yml` or `.json`).
        #[arg(long)]
        workflow: String,

        /// Adapter profile name (references `.agent/adapter.config.yaml`).
        #[arg(long)]
        profile: String,

        /// Name of the operator initiating the run.
        #[arg(long, default_value = "operator")]
        actor: String,

        /// Task lease TTL in seconds for acquired work.
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },

    /// Execute one discrete supervised step.
    Step {
        /// Run ID to operate on.
        #[arg(long)]
        run_id: String,

        /// Path to a Canonical Result Packet JSON file.
        #[arg(long)]
        result_file: Option<String>,

        /// Operator approval decision: APPROVED, REJECTED, or DEFERRED.
        #[arg(long)]
        approve: Option<String>,

        /// Reason for approval decision (required for APPROVED and REJECTED).
        #[arg(long, default_value = "")]
        reason: String,

        /// Skip confirmation prompts (for non-interactive use).
        #[arg(long, default_value_t = false)]
        yes: bool,
    },

    /// Show current workflow run progress.
    Status {
        /// Run ID to query.
        #[arg(long)]
        run_id: String,
    },

    /// List workflow runs, optionally filtered by workflow ID.
    ListRuns {
        /// Filter by workflow definition ID.
        #[arg(long)]
        workflow: Option<String>,
    },

    /// Cancel an active workflow run.
    CancelRun {
        /// Run ID to cancel.
        #[arg(long)]
        run_id: String,

        /// Reason for cancellation.
        #[arg(long, default_value = "")]
        reason: String,
    },

    /// Show the current phase definition and criteria for a run.
    ShowPhase {
        /// Run ID to query.
        #[arg(long)]
        run_id: String,
    },

    /// Validate a workflow definition file without creating a run.
    Validate {
        /// Workflow definition ID to validate.
        #[arg(long)]
        workflow: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_parse_init_run() {
        let cli = Cli::try_parse_from([
            "agent-workflow",
            "init-run",
            "--workflow",
            "my_workflow",
            "--profile",
            "full_exec_agent",
        ])
        .expect("parse init-run");

        match cli.command {
            Commands::InitRun {
                workflow,
                profile,
                actor,
                ttl_seconds,
            } => {
                assert_eq!(workflow, "my_workflow");
                assert_eq!(profile, "full_exec_agent");
                assert_eq!(actor, "operator");
                assert_eq!(ttl_seconds, None);
            }
            _ => panic!("Expected InitRun command"),
        }
    }

    #[test]
    fn test_cli_parse_step_dispatch() {
        let cli = Cli::try_parse_from(["agent-workflow", "step", "--run-id", "run_123"])
            .expect("parse step dispatch");

        match cli.command {
            Commands::Step {
                run_id,
                result_file,
                approve,
                reason: _,
                yes,
            } => {
                assert_eq!(run_id, "run_123");
                assert_eq!(result_file, None);
                assert_eq!(approve, None);
                assert!(!yes);
            }
            _ => panic!("Expected Step command"),
        }
    }

    #[test]
    fn test_cli_parse_step_with_result() {
        let cli = Cli::try_parse_from([
            "agent-workflow",
            "step",
            "--run-id",
            "run_123",
            "--result-file",
            "/tmp/result.json",
        ])
        .expect("parse step with result");

        match cli.command {
            Commands::Step { result_file, .. } => {
                assert_eq!(result_file, Some("/tmp/result.json".to_string()));
            }
            _ => panic!("Expected Step command"),
        }
    }

    #[test]
    fn test_cli_parse_step_with_approval() {
        let cli = Cli::try_parse_from([
            "agent-workflow",
            "step",
            "--run-id",
            "run_123",
            "--approve",
            "APPROVED",
            "--reason",
            "Looks good",
        ])
        .expect("parse step with approval");

        match cli.command {
            Commands::Step {
                approve, reason, ..
            } => {
                assert_eq!(approve, Some("APPROVED".to_string()));
                assert_eq!(reason, "Looks good");
            }
            _ => panic!("Expected Step command"),
        }
    }

    #[test]
    fn test_cli_parse_status() {
        let cli = Cli::try_parse_from(["agent-workflow", "status", "--run-id", "run_123"])
            .expect("parse status");

        match cli.command {
            Commands::Status { run_id } => {
                assert_eq!(run_id, "run_123");
            }
            _ => panic!("Expected Status command"),
        }
    }

    #[test]
    fn test_cli_parse_list_runs() {
        let cli = Cli::try_parse_from(["agent-workflow", "list-runs", "--workflow", "my_workflow"])
            .expect("parse list-runs");

        match cli.command {
            Commands::ListRuns { workflow } => {
                assert_eq!(workflow, Some("my_workflow".to_string()));
            }
            _ => panic!("Expected ListRuns command"),
        }
    }

    #[test]
    fn test_cli_parse_cancel_run() {
        let cli = Cli::try_parse_from([
            "agent-workflow",
            "cancel-run",
            "--run-id",
            "run_123",
            "--reason",
            "Wrong workflow",
        ])
        .expect("parse cancel-run");

        match cli.command {
            Commands::CancelRun { run_id, reason } => {
                assert_eq!(run_id, "run_123");
                assert_eq!(reason, "Wrong workflow");
            }
            _ => panic!("Expected CancelRun command"),
        }
    }

    #[test]
    fn test_cli_parse_show_phase() {
        let cli = Cli::try_parse_from(["agent-workflow", "show-phase", "--run-id", "run_123"])
            .expect("parse show-phase");

        match cli.command {
            Commands::ShowPhase { run_id } => {
                assert_eq!(run_id, "run_123");
            }
            _ => panic!("Expected ShowPhase command"),
        }
    }

    #[test]
    fn test_cli_parse_validate() {
        let cli = Cli::try_parse_from(["agent-workflow", "validate", "--workflow", "my_workflow"])
            .expect("parse validate");

        match cli.command {
            Commands::Validate { workflow } => {
                assert_eq!(workflow, "my_workflow");
            }
            _ => panic!("Expected Validate command"),
        }
    }
}
