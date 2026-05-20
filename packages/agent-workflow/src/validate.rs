use crate::error::ControllerError;
use crate::model::{Criterion, WorkflowDefinition};
use std::collections::HashSet;

/// Validate a workflow definition.
///
/// Returns `Ok(())` if every check passes, or `Err(ControllerError)` on the
/// first failure encountered.
///
/// Checks performed:
/// 1. `phases` is non-empty.
/// 2. `phase_id` values are unique across the workflow.
/// 3. No `Criterion::FutureHook` exists in any `entry_criteria` or `exit_criteria`.
/// 4. When `operator_approval_required == true`, the `exit_criteria` must
///    contain at least one `Criterion::OperatorApproval`.
/// 5. When `max_phase_duration_minutes` is `None`, `operator_approval_required`
///    must be `true` (safety gate against infinite leases).
pub fn validate_workflow_definition(def: &WorkflowDefinition) -> Result<(), ControllerError> {
    // 1. At least one phase.
    if def.phases.is_empty() {
        return Err(ControllerError::InvalidWorkflowDefinition {
            message: "Workflow definition must contain at least one phase".to_string(),
        });
    }

    // 2. Unique phase IDs.
    let mut seen_ids = HashSet::new();
    for phase in &def.phases {
        if !seen_ids.insert(&phase.phase_id) {
            return Err(ControllerError::InvalidWorkflowDefinition {
                message: format!(
                    "Duplicate phase_id '{}' in workflow definition",
                    phase.phase_id
                ),
            });
        }
    }

    for phase in &def.phases {
        // 3. Reject any `future_hook` criterion.
        for (idx, criterion) in phase.entry_criteria.iter().enumerate() {
            if matches!(criterion, Criterion::FutureHook) {
                return Err(ControllerError::UnsupportedCriterion {
                    phase_id: phase.phase_id.clone(),
                    criterion_type: format!("future_hook at entry_criteria index {}", idx),
                });
            }
        }
        for (idx, criterion) in phase.exit_criteria.iter().enumerate() {
            if matches!(criterion, Criterion::FutureHook) {
                return Err(ControllerError::UnsupportedCriterion {
                    phase_id: phase.phase_id.clone(),
                    criterion_type: format!("future_hook at exit_criteria index {}", idx),
                });
            }
        }

        // 4. Operator approval consistency: if required, exit criteria must
        //    contain an operator_approval criterion.
        if phase.operator_approval_required {
            let has_operator_approval = phase
                .exit_criteria
                .iter()
                .any(|c| matches!(c, Criterion::OperatorApproval(_)));
            if !has_operator_approval {
                return Err(ControllerError::InvalidWorkflowDefinition {
                    message: format!(
                        "Phase '{}' has operator_approval_required=true but no \
                         operator_approval criterion in exit_criteria",
                        phase.phase_id
                    ),
                });
            }
        }

        // 5. Safety gate: null max_phase_duration_minutes requires
        //    operator_approval_required == true to prevent infinite leases.
        if phase.max_phase_duration_minutes.is_none() && !phase.operator_approval_required {
            return Err(ControllerError::InvalidWorkflowDefinition {
                message: format!(
                    "Phase '{}' has max_phase_duration_minutes=null but \
                     operator_approval_required=false. Null duration requires \
                     an approval gate. See tech-spec §4.1.",
                    phase.phase_id
                ),
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ArtifactCriterion, Criterion, GraphStateCriterion, OperatorApprovalCriterion, Phase,
        ResultCriterion, RetryPolicy, TimeCriterion, TimeoutPolicy, WorkflowDefinition,
    };

    fn minimal_definition() -> WorkflowDefinition {
        WorkflowDefinition {
            workflow_id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test workflow".to_string(),
            version: "1.0.0".to_string(),
            adapter_profile: "default".to_string(),
            phases: vec![Phase {
                phase_id: "p1".to_string(),
                name: "Phase 1".to_string(),
                description: "desc".to_string(),
                entry_criteria: vec![Criterion::GraphState(GraphStateCriterion {
                    key: "status_counts.READY".to_string(),
                    op: ">=".to_string(),
                    value: 1,
                })],
                exit_criteria: vec![Criterion::Result(ResultCriterion {
                    status: "success".to_string(),
                    last_task_completed: None,
                })],
                operator_approval_required: false,
                verification_required: false,
                allowed_task_types: vec!["setup".to_string()],
                max_phase_duration_minutes: Some(30),
            }],
            timeout_policy: TimeoutPolicy {
                default_phase_timeout_minutes: 60,
                total_workflow_timeout_minutes: 120,
                on_timeout: "fail".to_string(),
            },
            retry_policy: RetryPolicy {
                workflow_max_retries: 3,
                sequential_task_failure_threshold: 2,
            },
            stop_conditions: vec!["all_phases_completed".to_string()],
        }
    }

    #[test]
    fn test_valid_definition_passes() {
        let def = minimal_definition();
        assert!(validate_workflow_definition(&def).is_ok());
    }

    #[test]
    fn test_empty_phases_rejected() {
        let mut def = minimal_definition();
        def.phases.clear();
        let err = validate_workflow_definition(&def).unwrap_err();
        assert_eq!(err.code(), "INVALID_WORKFLOW_DEFINITION");
        assert!(err.message().contains("at least one phase"));
    }

    #[test]
    fn test_duplicate_phase_ids_rejected() {
        let mut def = minimal_definition();
        def.phases.push(Phase {
            phase_id: "p1".to_string(), // duplicate
            name: "Phase 1 again".to_string(),
            description: "dup".to_string(),
            entry_criteria: vec![],
            exit_criteria: vec![],
            operator_approval_required: true,
            verification_required: false,
            allowed_task_types: vec![],
            max_phase_duration_minutes: None,
        });
        let err = validate_workflow_definition(&def).unwrap_err();
        assert_eq!(err.code(), "INVALID_WORKFLOW_DEFINITION");
        assert!(err.message().contains("Duplicate phase_id"));
    }

    #[test]
    fn test_future_hook_in_entry_rejected() {
        let mut def = minimal_definition();
        def.phases[0].entry_criteria.push(Criterion::FutureHook);
        let err = validate_workflow_definition(&def).unwrap_err();
        assert_eq!(err.code(), "UNSUPPORTED_CRITERION");
        assert!(err.message().contains("future_hook"));
    }

    #[test]
    fn test_future_hook_in_exit_rejected() {
        let mut def = minimal_definition();
        def.phases[0].exit_criteria.push(Criterion::FutureHook);
        let err = validate_workflow_definition(&def).unwrap_err();
        assert_eq!(err.code(), "UNSUPPORTED_CRITERION");
        assert!(err.message().contains("future_hook"));
    }

    #[test]
    fn test_operator_approval_consistency_missing_exit_criterion() {
        let mut def = minimal_definition();
        def.phases[0].operator_approval_required = true;
        // exit_criteria does NOT contain OperatorApproval
        let err = validate_workflow_definition(&def).unwrap_err();
        assert_eq!(err.code(), "INVALID_WORKFLOW_DEFINITION");
        assert!(err.message().contains("operator_approval_required=true"));
    }

    #[test]
    fn test_operator_approval_consistency_with_exit_criterion_passes() {
        let mut def = minimal_definition();
        def.phases[0].operator_approval_required = true;
        def.phases[0].exit_criteria =
            vec![Criterion::OperatorApproval(OperatorApprovalCriterion {
                decision: None,
            })];
        def.phases[0].max_phase_duration_minutes = None; // null OK because approval required
        assert!(validate_workflow_definition(&def).is_ok());
    }

    #[test]
    fn test_null_duration_without_approval_rejected() {
        let mut def = minimal_definition();
        def.phases[0].max_phase_duration_minutes = None;
        def.phases[0].operator_approval_required = false; // null duration + no approval = bad
        let err = validate_workflow_definition(&def).unwrap_err();
        assert_eq!(err.code(), "INVALID_WORKFLOW_DEFINITION");
        assert!(err
            .message()
            .contains("Null duration requires an approval gate"));
    }

    #[test]
    fn test_null_duration_with_approval_allowed() {
        let mut def = minimal_definition();
        def.phases[0].max_phase_duration_minutes = None;
        def.phases[0].operator_approval_required = true;
        def.phases[0].exit_criteria =
            vec![Criterion::OperatorApproval(OperatorApprovalCriterion {
                decision: None,
            })];
        assert!(validate_workflow_definition(&def).is_ok());
    }

    #[test]
    fn test_all_supported_criteria_accepted() {
        let def = WorkflowDefinition {
            workflow_id: "all_criteria".to_string(),
            name: "All Criteria".to_string(),
            description: "Tests all supported criterion types".to_string(),
            version: "1.0.0".to_string(),
            adapter_profile: "default".to_string(),
            phases: vec![Phase {
                phase_id: "all".to_string(),
                name: "All".to_string(),
                description: "Phase with every supported criterion".to_string(),
                entry_criteria: vec![
                    Criterion::GraphState(GraphStateCriterion {
                        key: "status_counts.READY".to_string(),
                        op: ">=".to_string(),
                        value: 1,
                    }),
                    Criterion::Artifact(ArtifactCriterion {
                        path: "./build/output.tar.gz".to_string(),
                        must_exist: true,
                        max_age_seconds: Some(3600),
                    }),
                    Criterion::Time(TimeCriterion {
                        since: "phase_start".to_string(),
                        elapsed_minutes: 30,
                        action: "fail".to_string(),
                    }),
                ],
                exit_criteria: vec![
                    Criterion::Result(ResultCriterion {
                        status: "success".to_string(),
                        last_task_completed: Some("task_1".to_string()),
                    }),
                    Criterion::OperatorApproval(OperatorApprovalCriterion { decision: None }),
                ],
                operator_approval_required: true,
                verification_required: false,
                allowed_task_types: vec!["build".to_string()],
                max_phase_duration_minutes: None,
            }],
            timeout_policy: TimeoutPolicy {
                default_phase_timeout_minutes: 60,
                total_workflow_timeout_minutes: 120,
                on_timeout: "fail".to_string(),
            },
            retry_policy: RetryPolicy {
                workflow_max_retries: 3,
                sequential_task_failure_threshold: 2,
            },
            stop_conditions: vec!["all_phases_completed".to_string()],
        };

        assert!(validate_workflow_definition(&def).is_ok());
    }
}
