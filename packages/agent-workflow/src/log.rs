use crate::error::ControllerError;
use crate::paths::ProjectPaths;
use chrono::Utc;
use serde::Serialize;
use std::io::Write;

/// Append a structured log event to `.agent/workflow_logs.jsonl`.
///
/// Each line is a single JSON object with `event_type`, `run_id`,
/// `timestamp`, and a `detail` blob derived from `detail`.
pub fn log_event<T: Serialize>(
    paths: &ProjectPaths,
    event_type: &str,
    run_id: &str,
    detail: &T,
) -> Result<(), ControllerError> {
    let log_path = paths.workflow_logs_file();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ControllerError::UnknownWorkflowError {
            message: format!("Failed to create logs directory: {}", e),
        })?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| ControllerError::UnknownWorkflowError {
            message: format!("Failed to open workflow logs: {}", e),
        })?;

    let entry = serde_json::json!({
        "event_type": event_type,
        "run_id": run_id,
        "timestamp": Utc::now().to_rfc3339(),
        "detail": detail,
    });

    let line = serde_json::to_string(&entry).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to serialize log entry: {}", e),
    })?;

    writeln!(file, "{}", line).map_err(|e| ControllerError::UnknownWorkflowError {
        message: format!("Failed to write log entry: {}", e),
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize)]
    struct DummyDetail {
        key: String,
        value: i32,
    }

    #[test]
    fn test_log_event_appends_jsonl() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        log_event(
            &paths,
            "run_initialized",
            "run_abc",
            &DummyDetail {
                key: "phase".to_string(),
                value: 1,
            },
        )
        .expect("log");

        let log_file = paths.workflow_logs_file();
        assert!(log_file.exists());

        let contents = std::fs::read_to_string(&log_file).expect("read log");
        let lines: Vec<&str> = contents.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).expect("valid json");
        assert_eq!(parsed["event_type"], "run_initialized");
        assert_eq!(parsed["run_id"], "run_abc");
        assert!(parsed["timestamp"].as_str().is_some());
        assert_eq!(parsed["detail"]["key"], "phase");
        assert_eq!(parsed["detail"]["value"], 1);
    }

    #[test]
    fn test_log_event_appends_multiple_lines() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        log_event(&paths, "event_a", "run_1", &serde_json::json!({"n": 1})).expect("log");
        log_event(&paths, "event_b", "run_1", &serde_json::json!({"n": 2})).expect("log");
        log_event(&paths, "event_a", "run_2", &serde_json::json!({"n": 3})).expect("log");

        let contents = std::fs::read_to_string(paths.workflow_logs_file()).expect("read log");
        let lines: Vec<&str> = contents.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);

        let parsed: Vec<serde_json::Value> =
            lines.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(parsed[0]["event_type"], "event_a");
        assert_eq!(parsed[1]["event_type"], "event_b");
        assert_eq!(parsed[2]["event_type"], "event_a");
    }
}
