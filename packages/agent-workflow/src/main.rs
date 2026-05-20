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

// ── Handlers ─────────────────────────────────────────────────────────────

fn handle_init_run(
    workflow: String,
    profile: String,
    _actor: String,
    _ttl_seconds: Option<u64>,
) -> Result<String, ControllerError> {
    let paths = ProjectPaths::discover()?;

    // 1. Validate path safety
    paths::validate_id(&workflow)?;

    // 2. Load workflow definition
    let def = {
        let yaml_path = paths.workflow_yaml(&workflow);
        let json_path = paths.workflow_json(&workflow);
        if yaml_path.exists() {
            WorkflowDefinition::from_yaml_file(&yaml_path)?
        } else if json_path.exists() {
            WorkflowDefinition::from_json_file(&json_path)?
        } else {
            return Err(ControllerError::WorkflowDefinitionNotFound {
                workflow_id: workflow.clone(),
            });
        }
    };

    // Verify workflow_id matches file
    if def.workflow_id != workflow {
        return Err(ControllerError::InvalidWorkflowDefinition {
            message: format!(
                "Workflow file for '{}' contains workflow_id '{}'. Mismatch.",
                workflow, def.workflow_id
            ),
        });
    }

    // 3. Validate definition
    validate_workflow_definition(&def)?;

    // 4. Initialize run
    let run_id =
        agent_workflow::run::init_run(&paths, &workflow, &profile, &def)?;

    // 5. Log
    agent_workflow::log::log_event(
        &paths,
        "run_initialized",
        &run_id,
        &serde_json::json!({
            "workflow_id": &workflow,
            "profile": &profile,
            "current_phase": def.phases.first().map(|p| &*p.phase_id),
        }),
    )?;

    // 6. Return envelope
    let envelope = SuccessEnvelope::with_run(
        &run_id,
        "Workflow run initialized. Use 'agent-workflow step --run-id <run_id>' to begin.",
    );
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_status(run_id: String) -> Result<String, ControllerError> {
    let paths = ProjectPaths::discover()?;
    paths::validate_id(&run_id)?;

    let state = agent_workflow::run::load_run(&paths, &run_id)?;
    let envelope = SuccessEnvelope::with_run(
        &run_id,
        &format!(
            "Phase: {} ({:?}), active_task_id: {}",
            state.current_phase.as_deref().unwrap_or("none"),
            state.phase_status,
            state.active_task_id.as_deref().unwrap_or("none")
        ),
    );
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_list_runs(workflow: Option<String>) -> Result<String, ControllerError> {
    let paths = ProjectPaths::discover()?;
    let runs = agent_workflow::run::list_runs(&paths, workflow.as_deref())?;
    let envelope = SuccessEnvelope {
        ok: true,
        workflow: None,
        run_id: None,
        current_phase: None,
        phase_status: None,
        message: Some(format!("Found {} run(s)", runs.len())),
    };
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_show_phase(run_id: String) -> Result<String, ControllerError> {
    let paths = ProjectPaths::discover()?;
    paths::validate_id(&run_id)?;

    let info = agent_workflow::phase::show_phase(&paths, &run_id)?;
    let envelope = SuccessEnvelope::new(&format!(
        "Phase: {} ({:?}) — operator_approval_required: {}",
        info.current_phase_id,
        info.phase_status,
        info.operator_approval_required,
    ));
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_cancel_run(run_id: String, reason: String) -> Result<String, ControllerError> {
    let paths = ProjectPaths::discover()?;
    paths::validate_id(&run_id)?;

    agent_workflow::run::cancel_run(&paths, &run_id, &reason)?;
    let envelope = SuccessEnvelope::new(&format!("Run '{}' cancelled.", &run_id),
    );
    Ok(serde_json::to_string_pretty(&envelope).expect("valid json"))
}

fn handle_validate(workflow: String) -> Result<String, ControllerError> {
    let paths = ProjectPaths::discover()?;

    paths::validate_id(&workflow)?;

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

    if def.workflow_id != workflow {
        return Err(ControllerError::InvalidWorkflowDefinition {
            message: format!(
                "Workflow file for '{}' contains workflow_id '{}'. Mismatch.",
                workflow, def.workflow_id
            ),
        });
    }

    validate_workflow_definition(&def)?;

    let envelope = SuccessEnvelope::new(&format!(
        "Workflow definition '{}' is valid.",
        &def.workflow_id
    ));
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
