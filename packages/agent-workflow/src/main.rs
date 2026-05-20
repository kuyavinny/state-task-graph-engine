use agent_workflow::cli::{Cli, Commands};
use agent_workflow::error::ControllerError;
use agent_workflow::model::WorkflowDefinition;
use agent_workflow::paths::{self, ProjectPaths};
use agent_workflow::response::{FailureEnvelope, SuccessEnvelope};
use agent_workflow::validate::validate_workflow_definition;
use clap::Parser;

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::InitRun {
            workflow,
            profile,
            actor,
            ttl_seconds,
        } => handle_init_run(workflow, profile, actor, ttl_seconds),
        Commands::Step {
            run_id,
            result_file,
            approve,
            reason,
            yes,
        } => handle_step(run_id, result_file, approve, reason, yes),
        Commands::Status { run_id } => handle_status(run_id),
        Commands::ListRuns { workflow } => handle_list_runs(workflow),
        Commands::CancelRun { run_id, reason } => handle_cancel_run(run_id, reason),
        Commands::ShowPhase { run_id } => handle_show_phase(run_id),
        Commands::Validate { workflow } => handle_validate(workflow),
    };

    match result {
        Ok(json) => {
            println!("{}", json);
        }
        Err(err) => {
            let envelope = FailureEnvelope::from_controller_error(&err);
            let json = serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| {
                r#"{"ok":false,"error":{"code":"UNKNOWN","source":"workflow_controller","message":"Serialization failure","retryable":true,"agent_action":"RETRY","human_action":"Report issue"}}"#.to_string()
            });
            eprintln!("{}", json);
            std::process::exit(1);
        }
    }
}

// ── Stub handlers (PR 1: return NOT_IMPLEMENTED) ────────────────────────

fn handle_init_run(
    _workflow: String,
    _profile: String,
    _actor: String,
    _ttl_seconds: Option<u64>,
) -> Result<String, ControllerError> {
    let envelope = FailureEnvelope::not_implemented("init-run");
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_step(
    _run_id: String,
    _result_file: Option<String>,
    _approve: Option<String>,
    _reason: String,
    _yes: bool,
) -> Result<String, ControllerError> {
    let envelope = FailureEnvelope::not_implemented("step");
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_status(_run_id: String) -> Result<String, ControllerError> {
    let envelope = FailureEnvelope::not_implemented("status");
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_list_runs(_workflow: Option<String>) -> Result<String, ControllerError> {
    let envelope = FailureEnvelope::not_implemented("list-runs");
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_cancel_run(_run_id: String, _reason: String) -> Result<String, ControllerError> {
    let envelope = FailureEnvelope::not_implemented("cancel-run");
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_show_phase(_run_id: String) -> Result<String, ControllerError> {
    let envelope = FailureEnvelope::not_implemented("show-phase");
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_validate(workflow: String) -> Result<String, ControllerError> {
    // 1. Discover project root
    let paths = ProjectPaths::discover()?;

    // 2. Validate workflow_id for path safety
    paths::validate_id(&workflow)?;

    // 3. Load definition: try .yml first, then .json
    let yaml_path = paths.workflow_yaml(&workflow);
    let json_path = paths.workflow_json(&workflow);

    let def = if yaml_path.exists() {
        WorkflowDefinition::from_yaml_file(&yaml_path)?
    } else if json_path.exists() {
        WorkflowDefinition::from_json_file(&json_path)?
    } else {
        return Err(ControllerError::WorkflowDefinitionNotFound {
            workflow_id: workflow.clone(),
        });
    };

    // 4. Verify workflow_id in file matches the CLI argument
    if def.workflow_id != workflow {
        return Err(ControllerError::InvalidWorkflowDefinition {
            message: format!(
                "Workflow file for '{}' contains workflow_id '{}'. Mismatch.",
                workflow, def.workflow_id
            ),
        });
    }

    // 5. Run structural validation
    validate_workflow_definition(&def)?;

    // 6. Return success envelope
    let envelope = SuccessEnvelope::new(&format!(
        "Workflow definition '{}' is valid.",
        &def.workflow_id
    ));
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}
