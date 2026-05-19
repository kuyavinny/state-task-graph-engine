use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;

fn stage() -> Command {
    Command::cargo_bin("stage").unwrap()
}

fn init_project(tmp: &TempDir) {
    stage()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success();
}

fn write_graph(tmp: &TempDir, yaml: &str) {
    std::fs::write(tmp.path().join(".agent/task_graph.yaml"), yaml).unwrap();
}

fn valid_graph_yaml() -> &'static str {
    r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: task-1
    parent_id: null
    title: First task
    description: The first task
    priority: 10
    status: READY
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#
}

#[test]
fn validate_empty_graph_passes() {
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""valid": true"#));
}

#[test]
fn validate_valid_dag_passes() {
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, valid_graph_yaml());
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""valid": true"#));
}

#[test]
fn validate_detects_duplicate_ids() {
    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: task-1
    parent_id: null
    title: Task A
    description: First
    priority: 10
    status: READY
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
  - id: task-1
    parent_id: null
    title: Task B
    description: Second
    priority: 5
    status: PENDING
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#;
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, yaml);
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("DUPLICATE_NODE_ID"));
}

#[test]
fn validate_detects_unknown_dependency() {
    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: task-1
    parent_id: null
    title: Task
    description: A task
    priority: 10
    status: READY
    dependencies: ["nonexistent"]
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#;
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, yaml);
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("UNKNOWN_DEPENDENCY"));
}

#[test]
fn validate_detects_cycle() {
    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: a
    parent_id: null
    title: A
    description: A
    priority: 10
    status: PENDING
    dependencies: ["b"]
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
  - id: b
    parent_id: null
    title: B
    description: B
    priority: 5
    status: PENDING
    dependencies: ["a"]
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#;
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, yaml);
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("CYCLE_DETECTED"));
}

#[test]
fn validate_invalid_yaml_returns_structured_error() {
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    // Write invalid YAML
    std::fs::write(
        tmp.path().join(".agent/task_graph.yaml"),
        "{{not valid yaml",
    )
    .unwrap();
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID_YAML"));
}

#[test]
fn validate_detects_invalid_lease() {
    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: task-1
    parent_id: null
    title: Active task
    description: A task in progress without lease
    priority: 10
    status: IN_PROGRESS
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 1
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#;
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, yaml);
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("lease.claimed_by"));
}

#[test]
fn validate_detects_terminal_without_reason() {
    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: task-1
    parent_id: null
    title: Completed task
    description: A completed task without result_summary
    priority: 10
    status: COMPLETED
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 1
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#;
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, yaml);
    stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("result_summary"));
}

#[test]
fn validate_returns_all_errors_in_details() {
    // Graph with both duplicate IDs and unknown deps
    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: dup
    parent_id: null
    title: A
    description: A
    priority: 10
    status: READY
    dependencies: ["ghost"]
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
  - id: dup
    parent_id: null
    title: B
    description: B
    priority: 5
    status: PENDING
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: {}
"#;
    let tmp = TempDir::new().unwrap();
    init_project(&tmp);
    write_graph(&tmp, yaml);

    let output = stage()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(envelope["ok"], false);
    // The top-level error code is INVALID_SCHEMA
    assert_eq!(envelope["error"]["code"], "VALIDATION_FAILED");
    // The details contain all errors
    let errors = envelope["error"]["details"]["errors"].as_array().unwrap();
    assert!(errors.len() >= 2);
    // Should have both DUPLICATE_NODE_ID and UNKNOWN_DEPENDENCY
    let codes: Vec<_> = errors.iter().map(|e| e["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"DUPLICATE_NODE_ID"));
    assert!(codes.contains(&"UNKNOWN_DEPENDENCY"));
}
