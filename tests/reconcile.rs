use assert_cmd::Command;

fn stg() -> Command {
    Command::cargo_bin("state-task-graph-engine").unwrap()
}

fn init_project(dir: &assert_fs::TempDir) {
    stg().arg("init").current_dir(dir.path()).assert().success();
}

/// Helper: write a graph YAML with given nodes to the .agent directory.
fn write_graph(dir: &assert_fs::TempDir, yaml: &str) {
    let graph_path = dir.path().join(".agent").join("task_graph.yaml");
    std::fs::write(&graph_path, yaml).unwrap();
}

#[test]
fn status_returns_counts_for_empty_graph() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let output = stg()
        .arg("status")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["node_count"], 0);
}

#[test]
fn status_counts_nodes_by_status() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-1"
    parent_id: null
    title: "First task"
    description: "Do the thing"
    priority: 2
    status: "READY"
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
    data: null
  - id: "task-2"
    parent_id: null
    title: "Second task"
    description: "Do another thing"
    priority: 1
    status: "PENDING"
    dependencies: ["task-1"]
    created_at: "2026-05-17T01:00:00Z"
    updated_at: "2026-05-17T01:00:00Z"
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
    data: null
"#;
    write_graph(&tmp, yaml);

    let output = stg()
        .arg("status")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["node_count"], 2);
}

#[test]
fn next_returns_highest_priority_ready_task() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "low-priority"
    parent_id: null
    title: "Low priority task"
    description: "Low"
    priority: 1
    status: "READY"
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
    data: null
  - id: "high-priority"
    parent_id: null
    title: "High priority task"
    description: "High"
    priority: 5
    status: "READY"
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
    data: null
"#;
    write_graph(&tmp, yaml);

    let output = stg()
        .arg("next")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["id"], "high-priority");
    assert_eq!(envelope["data"]["priority"], 5);
}

#[test]
fn next_returns_no_ready_message_when_none_available() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Empty graph has no READY tasks
    let output = stg()
        .arg("next")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["message"], "No READY tasks available");
}

#[test]
fn next_promotes_pending_to_ready_after_dependency_completed() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // task-1 COMPLETED, task-2 PENDING with dependency on task-1
    // After reconciliation, task-2 becomes READY
    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-1"
    parent_id: null
    title: "First task"
    description: "Do the thing"
    priority: 1
    status: "COMPLETED"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 1
    max_attempts: 3
    lease:
      claimed_by: null
      claimed_at: null
      expires_at: null
    result_summary: "done"
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "task-2"
    parent_id: null
    title: "Second task"
    description: "After task-1"
    priority: 1
    status: "PENDING"
    dependencies: ["task-1"]
    created_at: "2026-05-17T01:00:00Z"
    updated_at: "2026-05-17T01:00:00Z"
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
    data: null
"#;
    write_graph(&tmp, yaml);

    let output = stg()
        .arg("next")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    let next_id = envelope["data"]["id"].as_str().unwrap();
    assert_eq!(next_id, "task-2");
}

#[test]
fn status_with_desync_includes_warning() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Write a graph with mismatched revision
    let graph_path = tmp.path().join(".agent").join("task_graph.yaml");
    let yaml = r#"
schema_version: "1.0"
graph_revision: 5
nodes: []
"#;
    std::fs::write(&graph_path, yaml).unwrap();

    // Write an event log with revision 3
    let events_path = tmp.path().join(".agent").join("task_events.jsonl");
    let event = r#"{"event_id":"test-1","timestamp":"2026-05-17T00:00:00Z","graph_revision_before":2,"graph_revision_after":3,"node_id":"a","actor":"system","action":"init","from_status":null,"to_status":null,"reason":null,"metadata":null}"#;
    std::fs::write(&events_path, event).unwrap();

    let output = stg()
        .arg("status")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    assert!(envelope["warnings"].is_array());
    let warnings = envelope["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].as_str().unwrap().contains("EVENT_LOG_DESYNC"));
}
