use assert_cmd::Command;
use predicates::str::contains;
use std::path::PathBuf;

/// Helper: build the fake-graph binary and return its path.
fn fake_graph_bin() -> PathBuf {
    // The fake-graph example binary should be built before running tests.
    // Use `cargo build -p agent-adapter --example fake-graph` first.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let target_dir = PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("debug")
        .join("examples")
        .join("fake-graph");
    if cfg!(windows) {
        target_dir.with_extension("exe")
    } else {
        target_dir
    }
}

/// Helper: init a profile in a temp dir, returning the temp dir.
fn init_profile() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("init-profile");
    cmd.assert().success();
    tmp
}

/// Helper: set up config to use fake-graph as the graph engine binary.
fn config_with_fake_graph(tmp: &assert_fs::TempDir) {
    let config_path = tmp.path().join(".agent").join("adapter.config.yaml");
    let fake_bin = fake_graph_bin();
    let content = std::fs::read_to_string(&config_path).unwrap();
    let content = content.replace("./target/release/stage", fake_bin.to_str().unwrap());
    std::fs::write(&config_path, content).unwrap();
}

// --- Existing tests (preserved) ---

#[test]
fn binary_prints_help() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("--help");
    cmd.assert().success().stdout(contains("agent-adapter"));
}

#[test]
fn binary_prints_version() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("--version");
    cmd.assert().success().stdout(contains("0.1.0"));
}

#[test]
fn init_profile_creates_config_and_artifacts_dir() {
    let tmp = init_profile();

    assert!(
        tmp.path()
            .join(".agent")
            .join("adapter.config.yaml")
            .exists()
    );
    assert!(tmp.path().join(".agent").join("adapter_artifacts").exists());
}

#[test]
fn validate_profile_passes_on_valid_yaml() {
    let tmp = init_profile();

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("validate-profile");
    cmd.assert().success().stdout(contains("\"valid\": true"));
}

#[test]
fn validate_profile_fails_on_invalid_yaml() {
    let tmp = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".agent")).unwrap();
    std::fs::write(
        tmp.path().join(".agent").join("adapter.config.yaml"),
        "{{invalid yaml",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("validate-profile");
    cmd.assert().failure().stderr(contains("INVALID_PROFILE"));
}

#[test]
fn validate_profile_fails_when_config_missing() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("validate-profile");
    cmd.assert().failure().stderr(contains("PROFILE_NOT_FOUND"));
}

#[test]
fn list_profiles_extracts_names_and_identities() {
    let tmp = init_profile();

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("list-profiles");
    cmd.assert()
        .success()
        .stdout(contains("read_only_agent"))
        .stdout(contains("full_exec_agent"))
        .stdout(contains("claude_code"))
        .stdout(contains("openhands"));
}

#[test]
fn list_profiles_fails_when_config_missing() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("list-profiles");
    cmd.assert().failure().stderr(contains("PROFILE_NOT_FOUND"));
}

// --- E2E tests with fake-graph ---

#[test]
fn get_work_returns_task_with_fake_graph() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("get-work")
        .arg("--profile")
        .arg("read_only_agent");
    cmd.assert()
        .success()
        .stdout(contains("\"id\": \"t1\""))
        .stdout(contains("\"title\": \"Test task\""));
}

#[test]
fn heartbeat_succeeds_with_fake_graph() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("heartbeat")
        .arg("--profile")
        .arg("read_only_agent")
        .arg("--task-id")
        .arg("t1")
        .arg("--revision")
        .arg("1")
        .arg("--ttl-seconds")
        .arg("300");
    cmd.assert()
        .success()
        .stdout(contains("\"status\""))
        .stdout(contains("IN_PROGRESS"));
}

#[test]
fn release_work_succeeds_with_fake_graph() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("release-work")
        .arg("--profile")
        .arg("read_only_agent")
        .arg("--task-id")
        .arg("t1")
        .arg("--revision")
        .arg("1");
    cmd.assert().success().stdout(contains("\"released\""));
}

#[test]
fn submit_result_success_with_fake_graph() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("submit-result")
        .arg("--profile")
        .arg("full_exec_agent")
        .arg("--task-id")
        .arg("t1")
        .arg("--revision")
        .arg("1")
        .arg("--status")
        .arg("success")
        .arg("--summary")
        .arg("Task completed successfully");
    cmd.assert().success().stdout(contains("\"status\""));
}

#[test]
fn submit_result_rejects_missing_permissions() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    // read_only_agent does not have allow_skip
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("submit-result")
        .arg("--profile")
        .arg("read_only_agent")
        .arg("--task-id")
        .arg("t1")
        .arg("--revision")
        .arg("1")
        .arg("--status")
        .arg("skipped")
        .arg("--reason")
        .arg("Not applicable");
    cmd.assert().failure().stderr(contains("PERMISSION_DENIED"));
}

#[test]
fn render_context_returns_markdown_with_fake_graph() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("render-context")
        .arg("--profile")
        .arg("read_only_agent");
    cmd.assert()
        .success()
        .stdout(contains("\"format\": \"markdown\""));
}

#[test]
fn get_work_handles_no_work_available() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env(
            "FAKE_GRAPH_RESPONSE",
            "next:{\"status\":\"success\",\"data\":{\"task_id\":null,\"graph_revision\":0}}",
        )
        .arg("get-work")
        .arg("--profile")
        .arg("read_only_agent");
    cmd.assert().failure().stderr(contains("NO_WORK_AVAILABLE"));
}

#[test]
fn get_work_handles_graph_failure() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "next:{\"status\":\"failure\",\"code\":\"INTERNAL_ERROR\",\"message\":\"something broke\"}")
        .arg("get-work")
        .arg("--profile")
        .arg("read_only_agent");
    cmd.assert().failure().stderr(contains("FAILURE"));
}

#[test]
fn submit_result_rejects_empty_task_id() {
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("submit-result")
        .arg("--profile")
        .arg("read_only_agent")
        .arg("--task-id")
        .arg("")
        .arg("--revision")
        .arg("1")
        .arg("--status")
        .arg("success")
        .arg("--summary")
        .arg("Done");
    cmd.assert()
        .failure()
        .stderr(contains("INVALID_RESULT_PACKET"));
}

#[test]
fn release_work_rejects_missing_permission() {
    // Use read_only_agent which has allow_release=true, but we can test
    // by modifying the config to remove the permission
    let tmp = init_profile();
    config_with_fake_graph(&tmp);

    // Override config to remove allow_release
    let config_path = tmp.path().join(".agent").join("adapter.config.yaml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    let content = content.replace("allow_release: true", "allow_release: false");
    std::fs::write(&config_path, content).unwrap();

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path())
        .env("FAKE_GRAPH_RESPONSE", "")
        .arg("release-work")
        .arg("--profile")
        .arg("read_only_agent")
        .arg("--task-id")
        .arg("t1")
        .arg("--revision")
        .arg("1");
    cmd.assert().failure().stderr(contains("PERMISSION_DENIED"));
}
