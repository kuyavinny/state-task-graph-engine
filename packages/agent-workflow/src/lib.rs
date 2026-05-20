//! Workflow & Harness Controller
//!
//! Module 3 of agent-system-os. Orchestrates structured, repeatable workflow harnesses
//! over Module 1 (State & Task Graph Engine) and Module 2 (Universal Adapter Boundary).
//!
//! ## Core Principles
//!
//! - Loads declarative workflow definitions.
//! - Manages durable per-run state.
//! - Executes one supervised workflow step at a time.
//! - Enforces phase entry/exit criteria and approval gates.
//! - Routes all task mutations through Module 2 (`agent-adapter`).
//! - Never executes worker tasks directly.
//! - Never parses raw LLM output.
//! - Never calls Module 1 mutation commands directly.
//!
//! ## Architecture
//!
//! The crate is organized into modules:
//!
//! - `cli`: Command-line argument parsing with `clap`.
//! - `model`: Workflow definition and criterion structs.
//! - `run_state`: Per-run state structs (phase history, approval records, etc.).
//! - `error`: Controller error types and codes.
//! - `response`: JSON response envelope structs.
//! - `paths`: Path helpers for `.agent/` directories.
//! - `config`: Version and configuration constants.

pub mod adapter_client;
pub mod cli;
pub mod config;
pub mod criteria;
pub mod criteria_context;
pub mod error;
pub mod graph_client;
pub mod log;
pub mod model;
pub mod paths;
pub mod phase;
pub mod response;
pub mod run;
pub mod run_state;
pub mod step_dispatch;
pub mod step_intake;
pub mod validate;

/// Re-export key types at crate root.
pub use adapter_client::{
    AdapterClient, RealAdapterClient, ReleaseResult, RenderResult, SubmitResult, TaskPacket,
};
pub use cli::Cli;
pub use config::version;
pub use criteria::{
    evaluate_criteria, evaluate_one, CriterionResult, EvaluationContext, EvaluationResult,
};
pub use criteria_context::CriteriaContext;
pub use error::ControllerError;
pub use graph_client::{
    GraphStatusClient, GraphValidationResult, RealGraphStatusClient,
};
pub use model::{
    ArtifactCriterion, Criterion, GraphStateCriterion, OperatorApprovalCriterion, Phase,
    ResultCriterion, TimeCriterion, WorkflowDefinition,
};
pub use paths::ProjectPaths;
pub use phase::PhaseInfo;
pub use response::{FailureEnvelope, SuccessEnvelope};
pub use run::RunSummary;
pub use run_state::{
    ApprovalDecision, ApprovalRecord, PhaseHistoryItem, PhaseStatus, RunArtifact,
    WorkflowRetryCounters, WorkflowRunState,
};
pub use validate::validate_workflow_definition;
