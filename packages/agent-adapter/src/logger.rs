#[allow(unused_imports)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// A single structured entry in the adapter log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    /// ISO 8601 timestamp of the log event.
    pub timestamp: String,
    /// The graph command that was issued (e.g., "next", "claim", "summarize").
    pub command: String,
    /// The actor (agent identity) that issued the command.
    pub actor: String,
    /// Whether the command succeeded.
    pub success: bool,
    /// Optional error code if the command failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Optional human-readable error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Appends structured JSONL entries to `.agent/adapter_logs.jsonl`.
///
/// Each entry contains a timestamp, command, actor, success/failure status,
/// and optional error details. The log file is created if it doesn't exist
/// and entries are always appended (never overwritten).
pub struct AdapterLogger {
    log_path: PathBuf,
}

impl AdapterLogger {
    /// Create a new logger that writes to the given path.
    /// The file will be created on first write if it doesn't exist.
    pub fn new(log_path: PathBuf) -> Self {
        Self { log_path }
    }

    /// Create a logger using the default path `.agent/adapter_logs.jsonl`
    /// relative to the given base directory.
    pub fn default_path(base_dir: &PathBuf) -> Self {
        Self {
            log_path: base_dir.join(".agent").join("adapter_logs.jsonl"),
        }
    }

    /// Log a successful command.
    pub fn log_success(&self, command: &str, actor: &str) -> Result<(), std::io::Error> {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            command: command.to_string(),
            actor: actor.to_string(),
            success: true,
            error_code: None,
            error_message: None,
        };
        self.append_entry(&entry)
    }

    /// Log a failed command with error details.
    pub fn log_failure(
        &self,
        command: &str,
        actor: &str,
        error_code: &str,
        error_message: &str,
    ) -> Result<(), std::io::Error> {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            command: command.to_string(),
            actor: actor.to_string(),
            success: false,
            error_code: Some(error_code.to_string()),
            error_message: Some(error_message.to_string()),
        };
        self.append_entry(&entry)
    }

    /// Append a log entry as a single JSON line to the log file.
    fn append_entry(&self, entry: &LogEntry) -> Result<(), std::io::Error> {
        // Ensure parent directory exists
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        let mut json = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push('\n');
        file.write_all(json.as_bytes())?;
        file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    #[test]
    fn log_success_writes_valid_jsonl() {
        let dir = setup_temp_dir();
        let logger = AdapterLogger::new(dir.path().join("test_logs.jsonl"));
        logger.log_success("next", "agent-1").unwrap();

        let content = std::fs::read_to_string(dir.path().join("test_logs.jsonl")).unwrap();
        let entry: LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry.command, "next");
        assert_eq!(entry.actor, "agent-1");
        assert!(entry.success);
        assert!(entry.error_code.is_none());
        assert!(entry.error_message.is_none());
        // Timestamp should be valid RFC 3339
        assert!(DateTime::parse_from_rfc3339(&entry.timestamp).is_ok());
    }

    #[test]
    fn log_failure_writes_error_details() {
        let dir = setup_temp_dir();
        let logger = AdapterLogger::new(dir.path().join("test_logs.jsonl"));
        logger
            .log_failure("claim", "agent-1", "STALE_REVISION", "revision mismatch")
            .unwrap();

        let content = std::fs::read_to_string(dir.path().join("test_logs.jsonl")).unwrap();
        let entry: LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry.command, "claim");
        assert!(!entry.success);
        assert_eq!(entry.error_code, Some("STALE_REVISION".to_string()));
        assert_eq!(
            entry.error_message,
            Some("revision mismatch".to_string())
        );
    }

    #[test]
    fn log_appends_multiple_entries() {
        let dir = setup_temp_dir();
        let logger = AdapterLogger::new(dir.path().join("test_logs.jsonl"));
        logger.log_success("next", "agent-1").unwrap();
        logger.log_failure("claim", "agent-1", "CLAIM_FAILED", "already claimed").unwrap();
        logger.log_success("summarize", "agent-1").unwrap();

        let content = std::fs::read_to_string(dir.path().join("test_logs.jsonl")).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 3);

        let first: LogEntry = serde_json::from_str(lines[0]).unwrap();
        assert!(first.success);

        let second: LogEntry = serde_json::from_str(lines[1]).unwrap();
        assert!(!second.success);

        let third: LogEntry = serde_json::from_str(lines[2]).unwrap();
        assert!(third.success);
    }

    #[test]
    fn default_path_creates_adapter_dir() {
        let dir = setup_temp_dir();
        let logger = AdapterLogger::default_path(&dir.path().to_path_buf());
        logger.log_success("next", "agent-1").unwrap();

        assert!(dir.path().join(".agent").exists());
        assert!(dir.path().join(".agent/adapter_logs.jsonl").exists());
    }

    #[test]
    fn log_entry_serializes_skips_none_fields() {
        let entry = LogEntry {
            timestamp: "2026-05-19T00:00:00Z".to_string(),
            command: "next".to_string(),
            actor: "agent-1".to_string(),
            success: true,
            error_code: None,
            error_message: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("error_code"));
        assert!(!json.contains("error_message"));
    }
}