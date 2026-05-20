//! Integration tests: `agent-workflow validate --workflow <id>`

use std::fs;

/// Create a temporary `.agent/workflows/` directory and write a YAML
/// workflow definition into it.
fn setup_workflows_dir(name: &str, contents: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join(".agent/workflows")).expect("create workflows dir");
    fs::write(
        dir.path().join(format!(".agent/workflows/{}.yml", name)),
        contents,
    )
    .expect("write workflow yaml");
    dir
}

/// Create a temporary `.agent/workflows/` directory and write a JSON
/// workflow definition into it.
fn setup_workflows_dir_json(name: &str, contents: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join(".agent/workflows")).expect("create workflows dir");
    fs::write(
        dir.path().join(format!(".agent/workflows/{}.json", name)),
        contents,
    )
    .expect("write workflow json");
    dir
}

fn valid_workflow_yaml() -> &'static str {
    r#"
workflow_id: "api_deployment_v1"
name: "API Deployment"
description: "Deploy an API service."
version: "1.0.0"
adapter_profile: "full_exec_agent"
phases:
  - phase_id: "setup"
    name: "Setup"
    description: "Checkout code"
    entry_criteria:
      - criterion: "graph_state"
        key: "status_counts.READY"
        op: ">="
        value: 1
    exit_criteria:
      - criterion: "result"
        status: "success"
    operator_approval_required: false
    verification_required: false
    allowed_task_types:
      - "setup"
    max_phase_duration_minutes: 30
timeout_policy:
  default_phase_timeout_minutes: 60
  total_workflow_timeout_minutes: 480
  on_timeout: "cancel"
retry_policy:
  workflow_max_retries: 3
  sequential_task_failure_threshold: 3
stop_conditions:
  - "all_phases_completed"
"#
}

fn valid_workflow_json() -> &'static str {
    r#"{
  "workflow_id": "api_deployment_v2",
  "name": "API Deployment JSON",
  "description": "Deploy an API service (JSON format).",
  "version": "2.0.0",
  "adapter_profile": "full_exec_agent",
  "phases": [
    {
      "phase_id": "deploy",
      "name": "Deploy",
      "description": "Build and deploy",
      "entry_criteria": [
        { "criterion": "graph_state", "key": "status_counts.READY", "op": ">=", "value": 1 }
      ],
      "exit_criteria": [
        { "criterion": "result", "status": "success" }
      ],
      "operator_approval_required": false,
      "verification_required": false,
      "allowed_task_types": ["deploy"],
      "max_phase_duration_minutes": 20
    }
  ],
  "timeout_policy": {
    "default_phase_timeout_minutes": 60,
    "total_workflow_timeout_minutes": 480,
    "on_timeout": "cancel"
  },
  "retry_policy": {
    "workflow_max_retries": 3,
    "sequential_task_failure_threshold": 3
  },
  "stop_conditions": ["all_phases_completed"]
}"#
}

#[test]
fn test_validate_cli_valid_yaml() {
    let tmp = setup_workflows_dir("api_deployment_v1", valid_workflow_yaml());

    let yaml_path = tmp.path().join(".agent/workflows/api_deployment_v1.yml");
    assert!(yaml_path.exists());

    let def = agent_workflow::model::WorkflowDefinition::from_yaml_file(&yaml_path).expect("load yaml");
    assert_eq!(def.workflow_id, "api_deployment_v1");
    assert_eq!(def.phases.len(), 1);

    agent_workflow::validate::validate_workflow_definition(&def).expect("validate");
}

#[test]
fn test_validate_cli_valid_json() {
    let tmp = setup_workflows_dir_json("api_deployment_v2", valid_workflow_json());

    let json_path = tmp.path().join(".agent/workflows/api_deployment_v2.json");
    assert!(json_path.exists());

    let def = agent_workflow::model::WorkflowDefinition::from_json_file(&json_path).expect("load json");
    assert_eq!(def.workflow_id, "api_deployment_v2");
    assert_eq!(def.phases.len(), 1);

    agent_workflow::validate::validate_workflow_definition(&def).expect("validate");
}

#[test]
fn test_validate_cli_file_not_found() {
    let tmp = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(tmp.path().join(".agent/workflows")).expect("create workflows dir");

    let yaml_path = tmp.path().join(".agent/workflows/missing.yml");
    assert!(!yaml_path.exists());

    let result = agent_workflow::model::WorkflowDefinition::from_yaml_file(&yaml_path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), "INVALID_WORKFLOW_DEFINITION");
    assert!(err.message().contains("Failed to read workflow file"));
}

#[test]
fn test_validate_cli_workflow_id_mismatch() {
    // File name says "mismatched" but content says "other_workflow"
    // from_yaml_file succeeds; handler detects mismatch
    let yaml = r#"
workflow_id: "other_workflow"
name: "Other"
description: "Just a test"
version: "1.0.0"
adapter_profile: "default"
phases:
  - phase_id: "p1"
    name: "P1"
    description: "desc"
    entry_criteria: []
    exit_criteria: []
    operator_approval_required: false
    verification_required: false
    allowed_task_types: []
    max_phase_duration_minutes: 30
timeout_policy:
  default_phase_timeout_minutes: 60
  total_workflow_timeout_minutes: 120
  on_timeout: "fail"
retry_policy:
  workflow_max_retries: 3
  sequential_task_failure_threshold: 2
stop_conditions:
  - "all_phases_completed"
"#;
    let tmp = setup_workflows_dir("mismatched", yaml);
    let yaml_path = tmp.path().join(".agent/workflows/mismatched.yml");

    let def = agent_workflow::model::WorkflowDefinition::from_yaml_file(&yaml_path).expect("load");
    // Mismatch: file name "mismatched" vs content "other_workflow"
    assert_eq!(def.workflow_id, "other_workflow");
    assert_eq!(def.phases.len(), 1);
}

#[test]
fn test_validate_cli_malformed_yaml() {
    let tmp = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(tmp.path().join(".agent/workflows")).expect("create workflows dir");
    fs::write(
        tmp.path().join(".agent/workflows/bad.yml"),
        "not: a: valid: yaml: [bad",
    )
    .expect("write bad yaml");

    let yaml_path = tmp.path().join(".agent/workflows/bad.yml");
    let result = agent_workflow::model::WorkflowDefinition::from_yaml_file(&yaml_path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), "INVALID_WORKFLOW_DEFINITION");
}

#[test]
fn test_validate_cli_rejects_future_hook_file() {
    let yaml = r#"
workflow_id: "future_workflow"
name: "Future"
description: "Has future_hook"
version: "1.0.0"
adapter_profile: "default"
phases:
  - phase_id: "p1"
    name: "P1"
    description: "desc"
    entry_criteria:
      - criterion: "future_hook"
    exit_criteria:
      - criterion: "operator_approval"
    operator_approval_required: true
    verification_required: false
    allowed_task_types: []
    max_phase_duration_minutes: null
timeout_policy:
  default_phase_timeout_minutes: 60
  total_workflow_timeout_minutes: 120
  on_timeout: "fail"
retry_policy:
  workflow_max_retries: 3
  sequential_task_failure_threshold: 2
stop_conditions:
  - "all_phases_completed"
"#;
    let tmp = setup_workflows_dir("future_workflow", yaml);
    let yaml_path = tmp.path().join(".agent/workflows/future_workflow.yml");
    let def = agent_workflow::model::WorkflowDefinition::from_yaml_file(&yaml_path).expect("load yaml");
    let result = agent_workflow::validate::validate_workflow_definition(&def);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_CRITERION");
}
