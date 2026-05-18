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
fn append_nodes_file_not_found() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let output = stg()
        .args([
            "append-nodes",
            "--revision",
            "0",
            "--file",
            "nonexistent.yaml",
        ])
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
        "FILE_NOT_FOUND"
    );
    assert!(
        envelope["error"]["details"]["path"]
            .as_str()
            .unwrap()
            .contains("nonexistent.yaml")
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

#[test]
fn next_is_deterministic_across_multiple_runs() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Three READY tasks with same priority — should pick by created_at then id
    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-b"
    parent_id: null
    title: "B"
    description: "Second task"
    priority: 1
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:01Z"
    updated_at: "2026-05-17T00:00:01Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "task-a"
    parent_id: null
    title: "A"
    description: "First task"
    priority: 1
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "task-c"
    parent_id: null
    title: "C"
    description: "Third task"
    priority: 1
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:01Z"
    updated_at: "2026-05-17T00:00:01Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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

    // Run next 5 times — should return the same task every time
    let mut results = Vec::new();
    for _ in 0..5 {
        let output = stg()
            .arg("next")
            .current_dir(tmp.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let next_id = envelope["data"]["id"].as_str().unwrap();
        results.push(next_id.to_string());
    }
    assert_eq!(results, vec!["task-a"; 5]);
}

#[test]
fn next_skips_non_ready_tasks() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-a"
    parent_id: null
    title: "A"
    description: "Ready"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "task-b"
    parent_id: null
    title: "B"
    description: "In progress"
    priority: 10
    status: "IN_PROGRESS"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: "worker-1", claimed_at: "2026-05-17T00:00:00Z", expires_at: "2099-12-31T23:59:59Z" }
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
    let next_id = envelope["data"]["id"].as_str().unwrap();
    assert_eq!(next_id, "task-a");
}

#[test]
fn cli_claim_and_complete_happy_path() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-a"
    parent_id: null
    title: "A"
    description: "Test task"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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

    // Claim the task
    let claim_out = stg()
        .args(["claim", "task-a", "worker-1", "--ttl-seconds", "300"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let claim_env: serde_json::Value = serde_json::from_slice(&claim_out).unwrap();
    assert_eq!(claim_env["ok"], true);
    assert_eq!(claim_env["data"]["status"], "IN_PROGRESS");
    let rev = claim_env["graph_revision"].as_u64().unwrap();

    // Complete the task
    let comp_out = stg()
        .args([
            "complete",
            "task-a",
            "worker-1",
            "--revision",
            &rev.to_string(),
            "--result-summary",
            "Done",
        ])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let comp_env: serde_json::Value = serde_json::from_slice(&comp_out).unwrap();
    assert_eq!(comp_env["ok"], true);
    assert_eq!(comp_env["data"]["status"], "COMPLETED");

    // Status should show 1 COMPLETED
    let status_out = stg()
        .arg("status")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status_env: serde_json::Value = serde_json::from_slice(&status_out).unwrap();
    assert_eq!(status_env["data"]["status"]["COMPLETED"], 1);
}

#[test]
fn cli_non_owner_complete_rejected() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-a"
    parent_id: null
    title: "A"
    description: "Test task"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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

    // Claim by worker-1
    let claim_out = stg()
        .args(["claim", "task-a", "worker-1", "--ttl-seconds", "300"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let claim_env: serde_json::Value = serde_json::from_slice(&claim_out).unwrap();
    let rev = claim_env["graph_revision"].as_u64().unwrap();

    // Try to complete by worker-2 (non-owner)
    stg()
        .args([
            "complete",
            "task-a",
            "worker-2",
            "--revision",
            &rev.to_string(),
            "--result-summary",
            "Stolen",
        ])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("LEASE_NOT_OWNED"));
}

#[test]
fn cli_skip_with_reason_succeeds() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-a"
    parent_id: null
    title: "A"
    description: "Test task"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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

    // Skip directly from READY
    let out = stg()
        .args([
            "skip",
            "task-a",
            "worker-1",
            "--revision",
            "0",
            "--skip-reason",
            "Not needed",
        ])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let env: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["status"], "SKIPPED");
}

#[test]
fn cli_claim_pending_shows_dependency_info() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "task-a"
    parent_id: null
    title: "Dep task"
    description: "Has dep"
    priority: 5
    status: "PENDING"
    dependencies: ["task-b"]
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "task-b"
    parent_id: null
    title: "Unmet dep"
    description: "Not done yet"
    priority: 1
    status: "PENDING"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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

    // Claim should fail with dependency info in error message
    stg()
        .args(["claim", "task-a", "worker-1", "--ttl-seconds", "300"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("task-b"));
}

#[test]
fn summarize_returns_context_payload() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 1
nodes:
  - id: "root"
    parent_id: null
    title: "Root"
    description: "Root task"
    priority: 0
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "a"
    parent_id: "root"
    title: "Active Task"
    description: "Active"
    priority: 5
    status: "READY"
    dependencies: ["b"]
    created_at: "2026-05-17T00:00:01Z"
    updated_at: "2026-05-17T00:00:01Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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
    title: "Dep"
    description: "Completed dep"
    priority: 1
    status: "COMPLETED"
    dependencies: []
    created_at: "2026-05-17T00:00:02Z"
    updated_at: "2026-05-17T00:00:02Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: "Dep done"
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "c"
    parent_id: null
    title: "Dependent"
    description: "Depends on a"
    priority: 1
    status: "PENDING"
    dependencies: ["a"]
    created_at: "2026-05-17T00:00:03Z"
    updated_at: "2026-05-17T00:00:03Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "d"
    parent_id: null
    title: "Blocked"
    description: "Blocked task"
    priority: 1
    status: "BLOCKED"
    dependencies: []
    created_at: "2026-05-17T00:00:04Z"
    updated_at: "2026-05-17T00:00:04Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: "Waiting for input"
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "e"
    parent_id: null
    title: "Completed"
    description: "Completed task"
    priority: 1
    status: "COMPLETED"
    dependencies: []
    created_at: "2026-05-17T00:00:05Z"
    updated_at: "2026-05-17T00:00:05Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: "Completed summary"
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
"#;
    write_graph(&tmp, yaml);

    let events = [
        r#"{"event_id":"test-1","timestamp":"2026-05-17T00:01:00Z","graph_revision_before":0,"graph_revision_after":1,"node_id":"a","actor":"worker-1","action":"claim","from_status":null,"to_status":null,"reason":"Testing","metadata":null}"#,
    ];
    let events_path = tmp.path().join(".agent").join("task_events.jsonl");
    std::fs::write(&events_path, events.join("\n")).unwrap();

    let output = stg()
        .args([
            "summarize",
            "a",
            "--max-events",
            "1",
            "--max-completed-summaries",
            "1",
        ])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], true);
    let data = &envelope["data"];

    assert_eq!(data["active_task"]["id"], "a");
    assert_eq!(data["parent_chain"].as_array().unwrap().len(), 1);
    assert_eq!(data["parent_chain"][0]["id"], "root");
    assert_eq!(data["immediate_dependencies"].as_array().unwrap().len(), 1);
    assert_eq!(data["immediate_dependencies"][0]["id"], "b");
    assert_eq!(
        data["immediate_dependencies"][0]["result_summary"],
        "Dep done"
    );
    assert_eq!(data["dependent_tasks"].as_array().unwrap().len(), 1);
    assert_eq!(data["dependent_tasks"][0]["id"], "c");
    assert_eq!(
        data["blocked_or_failed_related"].as_array().unwrap().len(),
        1
    );
    assert_eq!(data["blocked_or_failed_related"][0]["id"], "d");
    assert_eq!(data["recent_events"].as_array().unwrap().len(), 1);
    assert_eq!(data["recent_events"][0]["reason"], "Testing");
    assert_eq!(data["completed_summaries"].as_array().unwrap().len(), 1);
    assert_eq!(data["completed_summaries"][0]["id"], "e");
    assert_eq!(
        data["completed_summaries"][0]["result_summary"],
        "Completed summary"
    );
    assert!(data["operator_notes"].is_null());
}

#[test]
fn summarize_respects_max_events_and_max_completed_summaries() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 3
nodes:
  - id: "a"
    parent_id: null
    title: "Active Task"
    description: "Active"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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
    title: "Completed 1"
    description: "Completed"
    priority: 1
    status: "COMPLETED"
    dependencies: []
    created_at: "2026-05-17T00:00:01Z"
    updated_at: "2026-05-17T00:00:01Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: "Summary 1"
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
  - id: "c"
    parent_id: null
    title: "Completed 2"
    description: "Completed"
    priority: 1
    status: "COMPLETED"
    dependencies: []
    created_at: "2026-05-17T00:00:02Z"
    updated_at: "2026-05-17T00:00:02Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: "Summary 2"
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
"#;
    write_graph(&tmp, yaml);

    let events = [
        r#"{"event_id":"test-1","timestamp":"2026-05-17T00:01:00Z","graph_revision_before":0,"graph_revision_after":1,"node_id":"a","actor":"worker-1","action":"claim","from_status":null,"to_status":null,"reason":"Event 1","metadata":null}"#,
        r#"{"event_id":"test-2","timestamp":"2026-05-17T00:02:00Z","graph_revision_before":1,"graph_revision_after":2,"node_id":"a","actor":"worker-1","action":"release","from_status":null,"to_status":null,"reason":"Event 2","metadata":null}"#,
        r#"{"event_id":"test-3","timestamp":"2026-05-17T00:03:00Z","graph_revision_before":2,"graph_revision_after":3,"node_id":"a","actor":"worker-1","action":"claim","from_status":null,"to_status":null,"reason":"Event 3","metadata":null}"#,
    ];
    let events_path = tmp.path().join(".agent").join("task_events.jsonl");
    std::fs::write(&events_path, events.join("\n")).unwrap();

    let output = stg()
        .args([
            "summarize",
            "a",
            "--max-events",
            "2",
            "--max-completed-summaries",
            "1",
        ])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let data = &envelope["data"];

    assert_eq!(data["recent_events"].as_array().unwrap().len(), 2);
    assert_eq!(data["recent_events"][0]["reason"], "Event 3");
    assert_eq!(data["recent_events"][1]["reason"], "Event 2");
    assert_eq!(data["completed_summaries"].as_array().unwrap().len(), 1);
    assert_eq!(data["completed_summaries"][0]["id"], "c");
}

#[test]
fn summarize_excludes_blocked_when_include_blocked_false() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "a"
    parent_id: null
    title: "Active Task"
    description: "Active"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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
    title: "Blocked"
    description: "Blocked task"
    priority: 1
    status: "BLOCKED"
    dependencies: []
    created_at: "2026-05-17T00:00:01Z"
    updated_at: "2026-05-17T00:00:01Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: "Waiting"
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
"#;
    write_graph(&tmp, yaml);

    let output = stg()
        .args(["summarize", "a", "--include-blocked", "false"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let data = &envelope["data"];

    assert_eq!(
        data["blocked_or_failed_related"].as_array().unwrap().len(),
        0
    );
}

#[test]
fn summarize_returns_error_on_malformed_event_log() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    let yaml = r#"
schema_version: "1.0"
graph_revision: 0
nodes:
  - id: "a"
    parent_id: null
    title: "Active Task"
    description: "Active"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
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

    // Write malformed JSONL
    let events_path = tmp.path().join(".agent").join("task_events.jsonl");
    std::fs::write(&events_path, "not-valid-json-at-all\n").unwrap();

    let output = stg()
        .args(["summarize", "a"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "SERIALIZATION_ERROR");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("EVENT_LOG_PARSE_ERROR")
    );
}

#[test]
fn summarize_propagates_desync_warning() {
    let tmp = assert_fs::TempDir::new().unwrap();
    init_project(&tmp);

    // Write a graph with mismatched revision
    let graph_path = tmp.path().join(".agent").join("task_graph.yaml");
    let yaml = r#"
schema_version: "1.0"
graph_revision: 5
nodes:
  - id: "a"
    parent_id: null
    title: "Task"
    description: "Task"
    priority: 5
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease: { claimed_by: null, claimed_at: null, expires_at: null }
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
"#;
    std::fs::write(&graph_path, yaml).unwrap();

    // Write an event log with revision 3
    let events_path = tmp.path().join(".agent").join("task_events.jsonl");
    let event = r#"{"event_id":"test-1","timestamp":"2026-05-17T00:00:00Z","graph_revision_before":2,"graph_revision_after":3,"node_id":"a","actor":"system","action":"init","from_status":null,"to_status":null,"reason":null,"metadata":null}"#;
    std::fs::write(&events_path, event).unwrap();

    let output = stg()
        .args(["summarize", "a"])
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
    assert_eq!(envelope["data"]["active_task"]["id"], "a");
}
