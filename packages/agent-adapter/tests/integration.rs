use assert_cmd::Command;
use predicates::str::contains;

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
    let tmp = assert_fs::TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("init-profile");
    cmd.assert()
        .success()
        .stdout(contains("\"initialized\": true"));

    // Verify config file exists
    assert!(
        tmp.path()
            .join(".agent")
            .join("adapter.config.yaml")
            .exists()
    );
    // Verify artifacts directory exists
    assert!(tmp.path().join(".agent").join("adapter_artifacts").exists());
}

#[test]
fn validate_profile_passes_on_valid_yaml() {
    let tmp = assert_fs::TempDir::new().unwrap();

    // Init first
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("init-profile");
    cmd.assert().success();

    // Now validate
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("validate-profile");
    cmd.assert().success().stdout(contains("\"valid\": true"));
}

#[test]
fn validate_profile_fails_on_invalid_yaml() {
    let tmp = assert_fs::TempDir::new().unwrap();

    // Create .agent dir and write invalid YAML
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

    // Don't init - config file doesn't exist
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("validate-profile");
    cmd.assert().failure().stderr(contains("PROFILE_NOT_FOUND"));
}

#[test]
fn list_profiles_extracts_names_and_identities() {
    let tmp = assert_fs::TempDir::new().unwrap();

    // Init first
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("init-profile");
    cmd.assert().success();

    // Now list profiles
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

    // Don't init - config file doesn't exist
    let mut cmd = Command::cargo_bin("agent-adapter").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("list-profiles");
    cmd.assert().failure().stderr(contains("PROFILE_NOT_FOUND"));
}
