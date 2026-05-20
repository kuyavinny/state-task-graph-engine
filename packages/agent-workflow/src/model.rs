use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Workflow Definition
// ---------------------------------------------------------------------------

/// Top-level workflow definition.
///
/// Loaded from `.agent/workflows/<workflow_id>.{yml,json}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub adapter_profile: String,
    pub phases: Vec<Phase>,
    pub timeout_policy: TimeoutPolicy,
    pub retry_policy: RetryPolicy,
    pub stop_conditions: Vec<String>,
}

impl WorkflowDefinition {
    /// Load a `WorkflowDefinition` from a YAML file path.
    pub fn from_yaml_file(path: &Path) -> Result<Self, crate::error::ControllerError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            crate::error::ControllerError::InvalidWorkflowDefinition {
                message: format!(
                    "Failed to read workflow file '{}': {}",
                    path.display(),
                    e
                ),
            }
        })?;

        let parsed: Self = serde_yaml::from_str(&contents).map_err(|e| {
            crate::error::ControllerError::InvalidWorkflowDefinition {
                message: format!("Failed to parse YAML: {}", e),
            }
        })?;

        Ok(parsed)
    }

    /// Load a `WorkflowDefinition` from a JSON file path.
    pub fn from_json_file(path: &Path) -> Result<Self, crate::error::ControllerError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            crate::error::ControllerError::InvalidWorkflowDefinition {
                message: format!(
                    "Failed to read workflow file '{}': {}",
                    path.display(),
                    e
                ),
            }
        })?;

        let parsed: Self = serde_json::from_str(&contents).map_err(|e| {
            crate::error::ControllerError::InvalidWorkflowDefinition {
                message: format!("Failed to parse JSON: {}", e),
            }
        })?;

        Ok(parsed)
    }
}

// ---------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------

/// A single phase within a workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Phase {
    pub phase_id: String,
    pub name: String,
    pub description: String,
    pub entry_criteria: Vec<Criterion>,
    pub exit_criteria: Vec<Criterion>,
    pub operator_approval_required: bool,
    pub verification_required: bool,
    pub allowed_task_types: Vec<String>,
    pub max_phase_duration_minutes: Option<u64>,
}

// ---------------------------------------------------------------------------
// Criterion
// ---------------------------------------------------------------------------

/// Workflow phase criterion.
///
/// V1 uses structured criteria (map of key/op/value) instead of string expressions.
/// The `criterion` field acts as the tag for serde deserialization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "criterion")]
pub enum Criterion {
    #[serde(rename = "graph_state")]
    GraphState(GraphStateCriterion),

    #[serde(rename = "artifact")]
    Artifact(ArtifactCriterion),

    #[serde(rename = "result")]
    Result(ResultCriterion),

    #[serde(rename = "operator_approval")]
    OperatorApproval(OperatorApprovalCriterion),

    #[serde(rename = "time")]
    Time(TimeCriterion),

    #[serde(rename = "future_hook")]
    FutureHook,
}

/// `graph_state` — evaluate against normalized graph status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphStateCriterion {
    pub key: String,
    pub op: String,
    #[serde(default)]
    pub value: i64,
}

/// `artifact` — evaluate filesystem presence/age.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactCriterion {
    pub path: String,
    #[serde(default = "default_true")]
    pub must_exist: bool,
    pub max_age_seconds: Option<u64>,
}

/// `result` — evaluate adapter result status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResultCriterion {
    pub status: String,
    pub last_task_completed: Option<String>,
}

/// `operator_approval` — requires explicit operator approval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperatorApprovalCriterion {
    #[serde(default)]
    pub decision: Option<String>,
}

/// `time` — evaluate elapsed time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimeCriterion {
    pub since: String,
    pub elapsed_minutes: u64,
    pub action: String,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Policy structs
// ---------------------------------------------------------------------------

/// Workflow-level timeout policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimeoutPolicy {
    pub default_phase_timeout_minutes: u64,
    pub total_workflow_timeout_minutes: u64,
    pub on_timeout: String,
}

/// Workflow-level retry policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryPolicy {
    pub workflow_max_retries: u64,
    pub sequential_task_failure_threshold: u64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// YAML round-trip test for WorkflowDefinition + Criterion.
    #[test]
    fn test_workflow_definition_yaml_roundtrip() {
        let yaml = r#"
workflow_id: "api_deployment_v1"
name: "API Deployment"
description: "Standard workflow for deploying an API service."
version: "1.0.0"
adapter_profile: "full_exec_agent"
phases:
  - phase_id: "setup"
    name: "Pre-deployment Setup"
    description: "Checkout code"
    entry_criteria:
      - criterion: "graph_state"
        key: "status_counts.READY"
        op: ">="
        value: 1
    exit_criteria:
      - criterion: "result"
        status: "success"
    operator_approval_required: false
    verification_required: false
    allowed_task_types:
      - "setup"
    max_phase_duration_minutes: 30
timeout_policy:
  default_phase_timeout_minutes: 60
  total_workflow_timeout_minutes: 480
  on_timeout: "cancel"
retry_policy:
  workflow_max_retries: 3
  sequential_task_failure_threshold: 3
stop_conditions:
  - "all_phases_completed"
"#;

        let parsed: WorkflowDefinition = serde_yaml::from_str(yaml).expect("parse yaml");

        assert_eq!(parsed.workflow_id, "api_deployment_v1");
        assert_eq!(parsed.name, "API Deployment");
        assert_eq!(parsed.phases.len(), 1);

        let phase = &parsed.phases[0];
        assert_eq!(phase.phase_id, "setup");
        assert_eq!(phase.entry_criteria.len(), 1);
        assert_eq!(phase.exit_criteria.len(), 1);

        // entry_criteria
        match &phase.entry_criteria[0] {
            Criterion::GraphState(gc) => {
                assert_eq!(gc.key, "status_counts.READY");
                assert_eq!(gc.op, ">=");
                assert_eq!(gc.value, 1);
            }
            other => panic!("Expected GraphState criterion, got: {:?}", other),
        }

        // exit_criteria
        match &phase.exit_criteria[0] {
            Criterion::Result(rc) => {
                assert_eq!(rc.status, "success");
                assert_eq!(rc.last_task_completed, None);
            }
            other => panic!("Expected Result criterion, got: {:?}", other),
        }

        // Serialize back to YAML and verify it round-trips
        let serialized = serde_yaml::to_string(&parsed).expect("serialize to yaml");
        let reparsed: WorkflowDefinition =
            serde_yaml::from_str(&serialized).expect("re-parse yaml");
        assert_eq!(reparsed.workflow_id, "api_deployment_v1");
        assert_eq!(reparsed.phases.len(), 1);
    }

    #[test]
    fn test_criterion_all_types_yaml() {
        let yaml = r#"
workflow_id: "test"
name: "Test"
description: "Test workflow"
version: "1.0.0"
adapter_profile: "test"
phases:
  - phase_id: "all_criteria"
    name: "All Criteria"
    description: "Phase with all criterion types"
    entry_criteria:
      - criterion: "graph_state"
        key: "status_counts.READY"
        op: ">="
        value: 1
      - criterion: "artifact"
        path: "./build/output.tar.gz"
        must_exist: true
        max_age_seconds: 3600
      - criterion: "time"
        since: "phase_start"
        elapsed_minutes: 30
        action: "fail"
    exit_criteria:
      - criterion: "operator_approval"
      - criterion: "result"
        status: "success"
        last_task_completed: "build_package"
    operator_approval_required: true
    verification_required: false
    allowed_task_types: []
    max_phase_duration_minutes: null
timeout_policy:
  default_phase_timeout_minutes: 60
  total_workflow_timeout_minutes: 120
  on_timeout: "fail"
retry_policy:
  workflow_max_retries: 3
  sequential_task_failure_threshold: 2
stop_conditions:
  - "all_phases_completed"
"#;

        let parsed: WorkflowDefinition = serde_yaml::from_str(yaml).expect("parse yaml");
        let phase = &parsed.phases[0];

        assert_eq!(phase.entry_criteria.len(), 3);
        assert_eq!(phase.exit_criteria.len(), 2);

        match &phase.entry_criteria[0] {
            Criterion::GraphState(gc) => {
                assert_eq!(gc.key, "status_counts.READY");
                assert_eq!(gc.op, ">=");
                assert_eq!(gc.value, 1);
            }
            other => panic!("expected GraphState, got {:?}", other),
        }

        match &phase.entry_criteria[1] {
            Criterion::Artifact(ac) => {
                assert_eq!(ac.path, "./build/output.tar.gz");
                assert_eq!(ac.must_exist, true);
                assert_eq!(ac.max_age_seconds, Some(3600));
            }
            other => panic!("expected Artifact, got {:?}", other),
        }

        match &phase.entry_criteria[2] {
            Criterion::Time(tc) => {
                assert_eq!(tc.since, "phase_start");
                assert_eq!(tc.elapsed_minutes, 30);
                assert_eq!(tc.action, "fail");
            }
            other => panic!("expected Time, got {:?}", other),
        }

        match &phase.exit_criteria[0] {
            Criterion::OperatorApproval(oa) => {
                assert_eq!(oa.decision, None);
            }
            other => panic!("expected OperatorApproval, got {:?}", other),
        }

        match &phase.exit_criteria[1] {
            Criterion::Result(rc) => {
                assert_eq!(rc.status, "success");
                assert_eq!(rc.last_task_completed, Some("build_package".to_string()));
            }
            other => panic!("expected Result, got {:?}", other),
        }
    }

    #[test]
    fn test_future_hook_criterion() {
        let yaml = r#"
workflow_id: "test"
name: "Test"
description: "Test"
version: "1.0.0"
adapter_profile: "test"
phases:
  - phase_id: "p1"
    name: "P1"
    description: "Phase with future_hook"
    entry_criteria:
      - criterion: "future_hook"
    exit_criteria: []
    operator_approval_required: false
    verification_required: false
    allowed_task_types: []
    max_phase_duration_minutes: null
timeout_policy:
  default_phase_timeout_minutes: 60
  total_workflow_timeout_minutes: 120
  on_timeout: "fail"
retry_policy:
  workflow_max_retries: 3
  sequential_task_failure_threshold: 2
stop_conditions:
  - "all_phases_completed"
"#;

        let parsed: WorkflowDefinition = serde_yaml::from_str(yaml).expect("parse yaml");
        let phase = &parsed.phases[0];
        assert_eq!(phase.entry_criteria.len(), 1);

        match &phase.entry_criteria[0] {
            Criterion::FutureHook => {}
            other => panic!("expected FutureHook, got {:?}", other),
        }
    }
}
