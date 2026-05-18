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

// ── append-nodes integration tests ───────────────────────────────────────

#[test]
fn append_nodes_valid_succeeds() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Pre-populate one node using status (via init then write), keeping rev 0
    let existing_yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "existing"
    parent_id: null
    title: "Existing"
    description: "Existing node"
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
"#;
    write_graph(&tmp, existing_yaml);

    // Write nodes file
    let nodes_path = tmp.path().join("nodes.yaml");
    let nodes_yaml = r#"
- id: "new-1"
  parent_id: null
  title: "New Node 1"
  description: "First appended node"
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
- id: "new-2"
  parent_id: null
  title: "New Node 2"
  description: "Second appended node"
  priority: 1
  status: "PENDING"
  dependencies: ["existing"]
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
    std::fs::write(&nodes_path, nodes_yaml).unwrap();

    let output = stg()
        .args(["append-nodes", "--revision", "0", "--file"])
        .arg(nodes_path)
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    assert!(envelope["data"]["revision"].as_u64().unwrap() > 0);
    assert_eq!(envelope["data"]["node_count"], 3); // existing + 2 new
    assert!(envelope["data"]["events_generated"].as_u64().unwrap() >= 2); // at least append events, possibly more
}

#[test]
fn append_nodes_stale_revision_fails() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Graph has revision 0, request revision 5
    let nodes_path = tmp.path().join("nodes.yaml");
    let nodes_yaml = r#"
- id: "a"
  parent_id: null
  title: "A"
  description: "Node A"
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
"#;
    std::fs::write(&nodes_path, nodes_yaml).unwrap();

    let output = stg()
        .args(["append-nodes", "--revision", "5", "--file"])
        .arg(nodes_path)
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "STALE_REVISION");
}

#[test]
fn append_nodes_creates_cycle_fails() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Append two nodes that form a cycle: a -> b -> a
    let nodes_path = tmp.path().join("nodes.yaml");
    let nodes_yaml = r#"
- id: "a"
  parent_id: null
  title: "A"
  description: "Node A"
  priority: 1
  status: "READY"
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
  data: null
- id: "b"
  parent_id: null
  title: "B"
  description: "Node B"
  priority: 1
  status: "READY"
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
  data: null
"#;
    std::fs::write(&nodes_path, nodes_yaml).unwrap();

    let output = stg()
        .args(["append-nodes", "--revision", "0", "--file"])
        .arg(nodes_path)
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], false);
    // Should be CYCLE_DETECTED or VALIDATION_FAILED
    let code = envelope["error"]["code"].as_str().unwrap();
    let is_validation_error = code == "CYCLE_DETECTED" || code == "VALIDATION_FAILED";
    assert!(
        is_validation_error,
        "Expected cycle/validation error, got {}",
        code
    );
}

#[test]
fn append_nodes_desync_rejected() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Write graph with revision 5 but empty event log -> desync
    let agent_dir = tmp.path().join(".agent");
    let graph_path = agent_dir.join("task_graph.yaml");
    let content = std::fs::read_to_string(&graph_path).unwrap();
    let modified = content.replace("graph_revision: 0", "graph_revision: 5");
    std::fs::write(&graph_path, modified).unwrap();

    // Try appending a node
    let nodes_path = tmp.path().join("desync_nodes.yaml");
    std::fs::write(
        &nodes_path,
        r#"
- id: "x"
  parent_id: null
  title: "X"
  description: "Should fail"
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
"#,
    )
    .unwrap();

    let output = stg()
        .args(["append-nodes", "--revision", "5", "--file"])
        .arg(nodes_path)
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(
        envelope["error"]["code"].as_str().unwrap(),
        "EVENT_LOG_DESYNC"
    );
}
