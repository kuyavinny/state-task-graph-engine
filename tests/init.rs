use assert_cmd::Command;
use predicates::prelude::*;

fn stg() -> Command {
    Command::cargo_bin("stg").unwrap()
}

#[test]
fn cli_help_flag_works() {
    stg()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "agent-graph: task graph engine for agents",
        ));
}

#[test]
fn cli_init_flag_works() {
    stg()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"));
}

#[test]
fn init_creates_agent_directory_and_files() {
    let tmp = assert_fs::TempDir::new().unwrap();

    stg().arg("init").current_dir(tmp.path()).assert().success();

    assert!(tmp.path().join(".agent").exists());
    assert!(tmp.path().join(".agent/task_graph.yaml").exists());
    assert!(tmp.path().join(".agent/task_events.jsonl").exists());
}

#[test]
fn init_outputs_success_json_envelope() {
    let tmp = assert_fs::TempDir::new().unwrap();

    stg()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains(r#""initialized": true"#))
        .stdout(predicate::str::contains(r#""graph_revision": 0"#));
}

#[test]
fn init_fails_if_already_initialized() {
    let tmp = assert_fs::TempDir::new().unwrap();

    // First init should succeed
    stg().arg("init").current_dir(tmp.path()).assert().success();

    // Second init should fail with ATOMIC_WRITE_FAILED
    stg()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("ATOMIC_WRITE_FAILED"));
}

#[test]
fn uninitialized_subcommands_return_not_implemented() {
    let tmp = assert_fs::TempDir::new().unwrap();

    // validate is implemented in PR#2 — needs a graph file
    stg()
        .arg("validate")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("TASK_NOT_FOUND"));

    // status — implemented in PR#3, needs a graph file
    stg()
        .arg("status")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("TASK_NOT_FOUND"));

    // next — implemented in PR#3, needs a graph file
    stg()
        .arg("next")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("TASK_NOT_FOUND"));
}

#[test]
fn json_failure_envelope_format() {
    let tmp = assert_fs::TempDir::new().unwrap();

    stg().arg("init").current_dir(tmp.path()).assert().success();

    // Second init should output a structured JSON error envelope
    let output = stg()
        .arg("init")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse as JSON to verify structure
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(envelope["ok"], false);
    assert!(envelope["error"].is_object());
    assert_eq!(envelope["error"]["code"], "ATOMIC_WRITE_FAILED");
}

#[test]
fn init_graph_yaml_is_valid() {
    let tmp = assert_fs::TempDir::new().unwrap();

    stg().arg("init").current_dir(tmp.path()).assert().success();

    let graph_content = std::fs::read_to_string(tmp.path().join(".agent/task_graph.yaml")).unwrap();

    let graph: serde_yaml::Value = serde_yaml::from_str(&graph_content).unwrap();
    assert_eq!(graph["schema_version"], "1.0");
    assert_eq!(graph["graph_revision"], 0);
    assert!(graph["nodes"].is_sequence());
}

#[test]
fn init_events_file_contains_init_event() {
    let tmp = assert_fs::TempDir::new().unwrap();

    stg().arg("init").current_dir(tmp.path()).assert().success();

    let events_content =
        std::fs::read_to_string(tmp.path().join(".agent/task_events.jsonl")).unwrap();
    assert!(
        !events_content.is_empty(),
        "Event log should contain an init event"
    );
    assert!(
        events_content.contains("init"),
        "Event log should contain an init action"
    );
    assert!(
        events_content.contains("__graph__"),
        "Init event should have __graph__ node_id"
    );
    assert!(
        events_content.contains("system"),
        "Init event should have system actor"
    );
}

#[test]
fn atomic_write_no_orphaned_tmp_file() {
    let tmp = assert_fs::TempDir::new().unwrap();

    stg().arg("init").current_dir(tmp.path()).assert().success();

    // After init, there should be no .tmp file
    assert!(!tmp.path().join(".agent/task_graph.yaml.tmp").exists());
}
