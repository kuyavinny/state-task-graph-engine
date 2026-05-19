use serde::{Deserialize, Serialize};

/// Canonical task packet returned by the `get-work` command.
///
/// Embeds the post-claim graph revision and all context needed by an agent
/// to execute a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalTaskPacket {
    pub adapter_version: String,
    pub profile: String,
    pub actor: String,
    pub graph_revision: u64,
    pub task: TaskInfo,
    pub bounded_context: BoundedContext,
    pub instructions: String,
    #[serde(default)]
    pub reporting_requirements: Vec<String>,
    pub heartbeat_requirements: HeartbeatRequirements,
    pub constraints: Constraints,
}

/// Core task identity and status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskInfo {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub lease_expires_at: Option<String>,
}

/// Bounded context surrounding the claimed task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundedContext {
    #[serde(default)]
    pub parent_chain: Vec<String>,
    #[serde(default)]
    pub immediate_dependencies: Vec<DependencyInfo>,
    #[serde(default)]
    pub dependent_tasks: Vec<DependencyInfo>,
    #[serde(default)]
    pub recent_events: Vec<serde_json::Value>,
    #[serde(default)]
    pub completed_summaries: Vec<serde_json::Value>,
}

/// Reference to a dependency or dependent task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyInfo {
    pub id: String,
    pub status: String,
}

/// Heartbeat policy for the claimed task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatRequirements {
    #[serde(default)]
    pub interval_seconds: u64,
}

/// Capability constraints for the agent executing this task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Constraints {
    pub read_files: bool,
    pub write_files: bool,
    pub execute_shell: bool,
    #[serde(default)]
    pub run_tests: bool,
    #[serde(default)]
    pub network_access: bool,
    #[serde(default)]
    pub browser_access: bool,
}
