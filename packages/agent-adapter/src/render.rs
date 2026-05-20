//! Markdown rendering for canonical task packets.
//!
//! Transforms a [`CanonicalTaskPacket`](crate::task_packet::CanonicalTaskPacket)
//! into prompt-friendly Markdown while enforcing the context character budget from
//! [`Capabilities::max_context_chars`](crate::config::Capabilities).
//!
//! # Core vs peripheral fields
//!
//! **Core fields are never truncated:**
//!
//! - Task ID, title, description
//! - Graph revision, lease expiration
//! - Immediate dependencies
//! - Reporting requirements
//!
//! **Peripheral fields are truncated first:**
//!
//! - Completed summaries
//! - Recent events
//!
//! When truncation occurs, a `⚠ Truncated` notice is appended and a
//! `truncated: true` flag appears in the JSON envelope.

use crate::config::Capabilities;
use crate::error::AdapterError;
use crate::response::SuccessEnvelope;
use crate::task_packet::CanonicalTaskPacket;

/// Render a canonical task packet as Markdown, truncating peripheral content
/// to fit within `max_context_chars`.
///
/// Returns `(markdown_string, was_truncated)`.
pub fn render_markdown(packet: &CanonicalTaskPacket, max_context_chars: u64) -> (String, bool) {
    let mut md = String::new();

    // --- Core fields (never truncated) ---
    md.push_str(&format!("# Task: {}\n\n", packet.task.title));
    md.push_str(&format!("**ID:** {}\n\n", packet.task.id));
    md.push_str(&format!("**Status:** {}\n\n", packet.task.status));
    md.push_str(&format!(
        "**Graph Revision:** {}\n\n",
        packet.graph_revision
    ));

    if let Some(ref lease) = packet.task.lease_expires_at {
        md.push_str(&format!("**Lease Expires:** {}\n\n", lease));
    }

    md.push_str(&format!(
        "## Description\n\n{}\n\n",
        packet.task.description
    ));

    // Immediate dependencies (core)
    if !packet.bounded_context.immediate_dependencies.is_empty() {
        md.push_str("## Dependencies\n\n");
        for dep in &packet.bounded_context.immediate_dependencies {
            md.push_str(&format!("- {} ({})\n", dep.id, dep.status));
        }
        md.push('\n');
    }

    // Reporting requirements (core)
    if !packet.reporting_requirements.is_empty() {
        md.push_str("## Reporting Requirements\n\n");
        for req in &packet.reporting_requirements {
            md.push_str(&format!("- {}\n", req));
        }
        md.push('\n');
    }

    let core_len = md.len();

    // --- Peripheral fields (truncated if needed) ---
    let budget = max_context_chars as usize;
    let peripheral_budget = budget.saturating_sub(core_len);

    let mut truncated = false;

    // Completed summaries (peripheral, truncated first)
    if !packet.bounded_context.completed_summaries.is_empty() {
        md.push_str("## Completed Summaries\n\n");
        let mut summaries_len = 0;
        for summary in &packet.bounded_context.completed_summaries {
            let line = format!("- {}\n", serde_json::to_string(summary).unwrap_or_default());
            if summaries_len + line.len() > peripheral_budget / 2 {
                md.push_str("_...truncated_\n");
                truncated = true;
                break;
            }
            md.push_str(&line);
            summaries_len += line.len();
        }
        md.push('\n');
    }

    // Recalculate remaining budget
    let remaining = budget.saturating_sub(md.len());

    // Recent events (peripheral, truncated second)
    if !packet.bounded_context.recent_events.is_empty() {
        md.push_str("## Recent Events\n\n");
        let mut events_len = 0;
        for event in &packet.bounded_context.recent_events {
            let line = format!("- {}\n", serde_json::to_string(event).unwrap_or_default());
            if events_len + line.len() > remaining {
                md.push_str("_...truncated_\n");
                truncated = true;
                break;
            }
            md.push_str(&line);
            events_len += line.len();
        }
        md.push('\n');
    }

    // If still over budget, hard-truncate from the end
    if md.len() > budget {
        md.truncate(budget);
        md.push_str("\n\n⚠ _Truncated to fit context budget_\n");
        truncated = true;
    }

    if truncated {
        md.push_str("\n⚠ _Some content was truncated to fit the context budget._\n");
    }

    (md, truncated)
}

/// Render a canonical task packet as a Markdown string and wrap it in a
/// standard JSON success envelope.
///
/// The `data` field contains:
/// - `format`: `"markdown"`
/// - `content`: the rendered Markdown
/// - `truncated`: whether content was truncated
pub fn render_context(
    packet: &CanonicalTaskPacket,
    profile_name: &str,
    actor: &str,
    capabilities: &Capabilities,
) -> Result<String, AdapterError> {
    let (markdown, was_truncated) = render_markdown(packet, capabilities.max_context_chars);

    let data = serde_json::json!({
        "format": "markdown",
        "content": markdown,
        "truncated": was_truncated,
    });

    let mut warnings = Vec::new();
    if was_truncated {
        warnings.push("Output truncated to fit max_context_chars".to_string());
    }

    let envelope = SuccessEnvelope::with_warnings(profile_name, actor, data, warnings);
    serde_json::to_string_pretty(&envelope).map_err(|e| AdapterError::InvalidResultPacket {
        message: format!("failed to serialize render-context envelope: {}", e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_packet::{
        BoundedContext, Constraints, DependencyInfo, HeartbeatRequirements, TaskInfo,
    };

    fn minimal_packet() -> CanonicalTaskPacket {
        CanonicalTaskPacket {
            adapter_version: "1.0.0".to_string(),
            profile: "test".to_string(),
            actor: "agent".to_string(),
            graph_revision: 42,
            task: TaskInfo {
                id: "task-1".to_string(),
                title: "Test Task".to_string(),
                description: "A test task description".to_string(),
                status: "IN_PROGRESS".to_string(),
                lease_expires_at: Some("2026-01-01T00:00:00Z".to_string()),
            },
            bounded_context: BoundedContext {
                parent_chain: vec![],
                immediate_dependencies: vec![DependencyInfo {
                    id: "dep-1".to_string(),
                    status: "COMPLETED".to_string(),
                }],
                dependent_tasks: vec![],
                recent_events: vec![],
                completed_summaries: vec![],
            },
            instructions: "Complete the task".to_string(),
            reporting_requirements: vec!["summary".to_string(), "artifacts".to_string()],
            heartbeat_requirements: HeartbeatRequirements {
                interval_seconds: 300,
            },
            constraints: Constraints {
                read_files: true,
                write_files: true,
                execute_shell: false,
                run_tests: false,
                network_access: false,
                browser_access: false,
            },
        }
    }

    fn test_capabilities(max_context_chars: u64) -> Capabilities {
        Capabilities {
            read_files: true,
            write_files: true,
            execute_shell: false,
            run_tests: false,
            network_access: false,
            browser_access: false,
            long_running_tasks: false,
            max_task_minutes: 10,
            preferred_format: "markdown".to_string(),
            max_context_chars,
        }
    }

    #[test]
    fn core_fields_always_present() {
        let packet = minimal_packet();
        let (md, truncated) = render_markdown(&packet, 10000);
        assert!(!truncated);
        assert!(md.contains("# Task: Test Task"));
        assert!(md.contains("**ID:** task-1"));
        assert!(md.contains("**Status:** IN_PROGRESS"));
        assert!(md.contains("**Graph Revision:** 42"));
        assert!(md.contains("**Lease Expires:** 2026-01-01"));
        assert!(md.contains("## Description"));
        assert!(md.contains("## Dependencies"));
        assert!(md.contains("## Reporting Requirements"));
    }

    #[test]
    fn massive_payload_truncated_under_budget() {
        let mut packet = minimal_packet();
        // Add lots of recent events to exceed budget
        for i in 0..500 {
            packet
                .bounded_context
                .recent_events
                .push(serde_json::json!({
                    "event": format!("event-{}", i),
                    "detail": "x".repeat(200),
                }));
        }
        let (md, truncated) = render_markdown(&packet, 1000);
        assert!(truncated);
        assert!(md.len() <= 1200); // some slack for truncation notice
    }

    #[test]
    fn core_fields_never_removed() {
        let mut packet = minimal_packet();
        // Overflow with events
        for i in 0..200 {
            packet
                .bounded_context
                .recent_events
                .push(serde_json::json!({
                    "event": format!("event-{}", i),
                    "detail": "y".repeat(100),
                }));
        }
        let (md, _truncated) = render_markdown(&packet, 2000);
        // Core fields are still present even when truncated
        assert!(md.contains("# Task: Test Task"));
        assert!(md.contains("**ID:** task-1"));
        assert!(md.contains("**Graph Revision:** 42"));
    }

    #[test]
    fn peripheral_fields_truncated_first() {
        let mut packet = minimal_packet();
        packet.bounded_context.completed_summaries = (0..50)
            .map(
                |i| serde_json::json!({"summary": format!("sum-{}", i), "detail": "z".repeat(100)}),
            )
            .collect();
        packet.bounded_context.recent_events = (0..50)
            .map(|i| serde_json::json!({"event": format!("evt-{}", i), "detail": "w".repeat(100)}))
            .collect();

        let (md, truncated) = render_markdown(&packet, 1500);
        assert!(truncated);
        // Core fields must still be present
        assert!(md.contains("## Description"));
        assert!(md.contains("## Dependencies"));
    }

    #[test]
    fn json_envelope_valid() {
        let packet = minimal_packet();
        let caps = test_capabilities(10000);
        let output = render_context(&packet, "test_profile", "test_agent", &caps).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["data"]["format"], "markdown");
        assert_eq!(parsed["data"]["truncated"], false);
    }

    #[test]
    fn truncation_warning_appears() {
        let mut packet = minimal_packet();
        for i in 0..500 {
            packet
                .bounded_context
                .recent_events
                .push(serde_json::json!({
                    "event": format!("event-{}", i),
                    "detail": "x".repeat(200),
                }));
        }
        let caps = test_capabilities(500);
        let output = render_context(&packet, "test_profile", "test_agent", &caps).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["data"]["truncated"], true);
        assert!(!parsed["warnings"].as_array().unwrap().is_empty());
    }
}
