use crate::error::AppError;
use crate::model::Graph;

use std::path::Path;

/// File names for the canonical v1 formats.
pub const GRAPH_FILE: &str = "task_graph.yaml";
pub const EVENTS_FILE: &str = "task_events.jsonl";
pub const AGENT_DIR: &str = ".agent";

/// Initialize a new empty graph and event log in the given directory.
///
/// Creates the `.agent/` directory and writes:
/// - `.agent/task_graph.yaml` — empty initial graph
/// - `.agent/task_events.jsonl` — empty event log
pub fn init_graph(project_dir: &Path) -> Result<(), AppError> {
    let agent_dir = project_dir.join(AGENT_DIR);

    if agent_dir.exists() {
        // Check if graph file already exists
        let graph_path = agent_dir.join(GRAPH_FILE);
        if graph_path.exists() {
            return Err(AppError::AtomicWriteFailed {
                message: format!("Graph already exists at {}", graph_path.display()),
            });
        }
    }

    std::fs::create_dir_all(&agent_dir)?;

    // Write empty graph
    let graph = Graph::new();
    write_graph(project_dir, &graph)?;

    // Write empty event log
    let events_path = agent_dir.join(EVENTS_FILE);
    std::fs::write(&events_path, "")?;

    Ok(())
}

/// Write graph state using atomic tempfile + rename.
///
/// 1. Write to `.agent/task_graph.yaml.tmp`
/// 2. Rename to `.agent/task_graph.yaml`
///
/// If the rename fails, the orphaned temp file is cleaned up automatically
/// via a Drop guard.
pub fn write_graph(project_dir: &Path, graph: &Graph) -> Result<(), AppError> {
    let agent_dir = project_dir.join(AGENT_DIR);
    let target_path = agent_dir.join(GRAPH_FILE);
    let tmp_path = agent_dir.join(format!("{}.tmp", GRAPH_FILE));

    // Ensure directory exists
    std::fs::create_dir_all(&agent_dir)?;

    // Serialize to YAML
    let yaml_content =
        serde_yaml::to_string(graph).map_err(|e| AppError::Serialization(e.to_string()))?;

    // Write to temp file
    std::fs::write(&tmp_path, yaml_content)?;

    // Guard cleans up the temp file on drop if the rename hasn't committed
    let mut guard = TmpFileGuard {
        path: &tmp_path,
        committed: false,
    };

    // Atomic rename
    std::fs::rename(&tmp_path, &target_path)?;
    guard.committed = true;

    Ok(())
}

/// Guard that cleans up the temp file on drop if the rename hasn't committed.
struct TmpFileGuard<'a> {
    path: &'a Path,
    committed: bool,
}

impl<'a> Drop for TmpFileGuard<'a> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

/// Read the graph state from disk.
#[allow(dead_code)]
pub fn read_graph(project_dir: &Path) -> Result<Graph, AppError> {
    let graph_path = project_dir.join(AGENT_DIR).join(GRAPH_FILE);

    if !graph_path.exists() {
        return Err(AppError::TaskNotFound {
            id: format!("graph file at {}", graph_path.display()),
        });
    }

    let content = std::fs::read_to_string(&graph_path)?;
    let graph: Graph = serde_yaml::from_str(&content)?;
    Ok(graph)
}

/// Append an event to the JSONL event log.
#[allow(dead_code)]
pub fn append_event(project_dir: &Path, event_json: &str) -> Result<(), AppError> {
    let events_path = project_dir.join(AGENT_DIR).join(EVENTS_FILE);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)?;
    writeln!(file, "{}", event_json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Event;

    #[test]
    fn init_creates_empty_graph_and_events() {
        let tmp = tempfile::tempdir().unwrap();
        init_graph(tmp.path()).unwrap();

        let graph_path = tmp.path().join(AGENT_DIR).join(GRAPH_FILE);
        assert!(graph_path.exists(), "Graph file should exist");

        let events_path = tmp.path().join(AGENT_DIR).join(EVENTS_FILE);
        assert!(events_path.exists(), "Events file should exist");

        let graph = read_graph(tmp.path()).unwrap();
        assert_eq!(graph.schema_version, "1.0");
        assert_eq!(graph.graph_revision, 0);
        assert!(graph.nodes.is_empty());
    }

    #[test]
    fn init_rejects_existing_graph() {
        let tmp = tempfile::tempdir().unwrap();
        init_graph(tmp.path()).unwrap();
        let result = init_graph(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let tmp = tempfile::tempdir().unwrap();
        init_graph(tmp.path()).unwrap();

        let tmp_path = tmp
            .path()
            .join(AGENT_DIR)
            .join(format!("{}.tmp", GRAPH_FILE));
        assert!(!tmp_path.exists(), "Temp file should not remain");
    }

    #[test]
    fn write_and_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        init_graph(tmp.path()).unwrap();

        let mut graph = read_graph(tmp.path()).unwrap();
        graph.graph_revision = 42;
        write_graph(tmp.path(), &graph).unwrap();

        let read_back = read_graph(tmp.path()).unwrap();
        assert_eq!(read_back.graph_revision, 42);
    }

    #[test]
    fn append_event_writes_jsonl_line() {
        let tmp = tempfile::tempdir().unwrap();
        init_graph(tmp.path()).unwrap();

        let event = Event {
            event_id: "test-uuid".to_string(),
            timestamp: "2026-05-17T23:00:55Z".to_string(),
            graph_revision_before: 0,
            graph_revision_after: 0,
            node_id: "root".to_string(),
            actor: "system".to_string(),
            action: crate::model::EventAction::Init,
            from_status: None,
            to_status: None,
            reason: None,
            metadata: serde_json::Value::Null,
        };

        let event_json = serde_json::to_string(&event).unwrap();
        append_event(tmp.path(), &event_json).unwrap();

        let events_path = tmp.path().join(AGENT_DIR).join(EVENTS_FILE);
        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(content.contains("test-uuid"));
        assert!(content.contains("init"));

        // Parse it back
        let parsed: Event = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed.event_id, "test-uuid");
    }

    #[test]
    fn read_nonexistent_graph_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_graph(tmp.path());
        assert!(result.is_err());
    }
}
