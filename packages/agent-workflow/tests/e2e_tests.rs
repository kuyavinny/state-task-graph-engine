//! End-to-end integration tests for agent-workflow.
//!
//! Uses mock adapter and graph clients for deterministic testing.

use agent_workflow::{
    adapter_client::mock::MockAdapterClient,
    criteria_context::CriteriaContext,
    graph_client::mock::MockGraphStatusClient,
    model::{
        Criterion, GraphStateCriterion, Phase, ResultCriterion, TimeoutPolicy,
        WorkflowDefinition,
    },
    paths::ProjectPaths,
    run::{cancel_run, init_run, load_run},
    run_state::PhaseStatus,
    step_dispatch::{execute_step_dispatch, DispatchOutcome},
    step_intake::{execute_step_intake, IntakeOutcome},
};
use std::collections::HashMap;

fn e2e_definition() -> WorkflowDefinition {
    WorkflowDefinition {
        workflow_id: "deploy".to_string(),
        name: "Deploy".to_string(),
        description: "Deploy workflow".to_string(),
        version: "1.0.0".to_string(),
        adapter_profile: "default".to_string(),
        phases: vec![
            Phase {
                phase_id: "build".to_string(),
                name: "Build".to_string(),
                description: "".to_string(),
                entry_criteria: vec![Criterion::GraphState(GraphStateCriterion {
                    key: "status_counts.READY".to_string(),
                    op: ">=".to_string(),
                    value: 1,
                })],
                exit_criteria: vec![Criterion::Result(ResultCriterion {
                    status: "success".to_string(),
                    last_task_completed: None,
                })],
                operator_approval_required: false,
                verification_required: false,
                allowed_task_types: vec![],
                max_phase_duration_minutes: None,
            },
            Phase {
                phase_id: "test".to_string(),
                name: "Test".to_string(),
                description: "".to_string(),
                entry_criteria: vec![],
                exit_criteria: vec![Criterion::Result(ResultCriterion {
                    status: "success".to_string(),
                    last_task_completed: None,
                })],
                operator_approval_required: true,
                verification_required: false,
                allowed_task_types: vec![],
                max_phase_duration_minutes: None,
            },
        ],
        timeout_policy: TimeoutPolicy {
            default_phase_timeout_minutes: 60,
            total_workflow_timeout_minutes: 120,
            on_timeout: "fail".to_string(),
        },
        retry_policy: agent_workflow::model::RetryPolicy {
            workflow_max_retries: 2,
            sequential_task_failure_threshold: 2,
        },
        stop_conditions: vec!["all_phases_completed".to_string()],
    }
}

fn make_graph() -> MockGraphStatusClient {
    let mut graph = MockGraphStatusClient::new();
    graph.status_result = Ok(CriteriaContext {
        graph_revision: 1,
        node_count: 5,
        status_counts: {
            let mut m = HashMap::new();
            m.insert("READY".to_string(), 2);
            m
        },
        warnings: vec![],
    });
    graph
}

#[test]
fn test_e2e_happy_path() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
    let def = e2e_definition();

    // Init run
    let run_id = init_run(&paths, &def.workflow_id, &def.adapter_profile, &def,
    )
    .expect("init_run");

    let mut state = load_run(&paths, &run_id).expect("load_run");
    let adapter = MockAdapterClient::new();
    let graph = make_graph();

    // Dispatch phase 1 (build)
    let result = execute_step_dispatch(&paths, &adapter, &graph, &def, &mut state,
    );
    assert_eq!(
        result.unwrap(),
        DispatchOutcome::AwaitingWorker { task_id: "task_001".to_string() }
    );

    // Simulate worker completion with success result
    let result_file = tmp.path().join("result.json");
    std::fs::write(
        &result_file,
        serde_json::json!({"status": "success"}).to_string(),
    )
    .unwrap();

    let outcome = execute_step_intake(
        &paths, &adapter, &result_file, &def, &mut state,
    );
    assert_eq!(
        outcome.unwrap(),
        IntakeOutcome::PhaseAdvanced { phase_id: "test".to_string() }
    );

    // Check phase advanced
    assert_eq!(state.current_phase, Some("test".to_string()));
    assert_eq!(state.active_task_id, None);
}

#[test]
fn test_e2e_approval_gate() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
    let def = e2e_definition();

    let run_id = init_run(&paths, &def.workflow_id, &def.adapter_profile, &def,
    )
    .expect("init_run");

    let mut state = load_run(&paths, &run_id).expect("load_run");

    // Manually set to test phase (skipping build)
    state.current_phase = Some("test".to_string());
    state.phase_status = PhaseStatus::InProgress;

    agent_workflow::run::save_run_state(&paths, &run_id, &state).expect("save");

    let adapter = MockAdapterClient::new();
    let graph = make_graph();

    // Try to dispatch — should pause because operator_approval_required=true
    let result = execute_step_dispatch(
        &paths, &adapter, &graph, &def, &mut state,
    );
    assert!(result.is_err());
}

#[test]
fn test_e2e_cancel_run_without_task() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
    let def = e2e_definition();

    let run_id = init_run(&paths, &def.workflow_id, &def.adapter_profile, &def,
    )
    .expect("init_run");

    cancel_run(&paths, &run_id, "user cancelled",
    )
    .expect("cancel_run");

    let state = load_run(&paths, &run_id).expect("load_run");
    assert_eq!(state.phase_status, PhaseStatus::Cancelled);
    assert_eq!(state.stop_reason, Some("user cancelled".to_string()));
}

#[test]
fn test_e2e_result_packet_not_modified() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
    let def = e2e_definition();

    let run_id = init_run(&paths, &def.workflow_id, &def.adapter_profile, &def,
    )
    .expect("init_run");

    let mut state = load_run(&paths, &run_id).expect("load_run");
    let adapter = MockAdapterClient::new();
    let graph = make_graph();

    // Dispatch
    execute_step_dispatch(
        &paths, &adapter, &graph, &def, &mut state,
    )
    .unwrap();

    // Create exact result JSON
    let original = r#"{"status":"success","task_id":"task_001","extra":"field"}"#;
    let result_file = tmp.path().join("result.json");
    std::fs::write(&result_file, original).unwrap();

    // Submit result
    execute_step_intake(
        &paths, &adapter, &result_file, &def, &mut state,
    )
    .unwrap();

    // Verify original file is unchanged
    let after = std::fs::read_to_string(&result_file).unwrap();
    assert_eq!(after, original, "Result file must not be modified");
}

#[test]
fn test_e2e_run_resumes_from_persisted_state() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
    let def = e2e_definition();

    let run_id = init_run(
        &paths, &def.workflow_id, &def.adapter_profile, &def,
    )
    .expect("init_run");

    // Dispatch and persist
    let mut state = load_run(&paths, &run_id).expect("load_run");
    let adapter = MockAdapterClient::new();
    let graph = make_graph();

    execute_step_dispatch(
        &paths, &adapter, &graph, &def, &mut state,
    )
    .unwrap();

    // Simulate restart: reload state from disk
    let mut reloaded = load_run(&paths, &run_id).expect("load_run");
    assert_eq!(reloaded.active_task_id, Some("task_001".to_string()));

    // Verify can continue from reloaded state
    let result = execute_step_dispatch(
        &paths, &adapter, &graph, &def, &mut reloaded,
    );
    assert_eq!(
        result.unwrap(),
        DispatchOutcome::AwaitingResult {
            task_id: "task_001".to_string()
        }
    );
}
