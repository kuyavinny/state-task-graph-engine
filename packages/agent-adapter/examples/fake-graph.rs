//! Fake `agent-graph` binary for integration tests.
//!
//! Accepts the same subcommands as the real `agent-graph` CLI and returns
//! pre-configured JSON responses.  Controlled via environment variables:
//!
//! - `FAKE_GRAPH_RESPONSE` — comma-separated list of subcommand:json pairs,
//!   e.g. `next:{\"status\":\"success\",\"data\":...},claim:{...}`
//! - `FAKE_GRAPH_EXIT_CODE` — override exit code (default 0)
//! - `FAKE_GRAPH_STDERR` — write to stderr (for simulating crashes)
//!
//! If no `FAKE_GRAPH_RESPONSE` is set, returns default success responses
//! for each subcommand.

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("fake-graph: usage: fake-graph <subcommand> [args...]");
        process::exit(1);
    }

    let subcommand = &args[1];
    let responses_env = env::var("FAKE_GRAPH_RESPONSE").unwrap_or_default();
    let exit_code_env = env::var("FAKE_GRAPH_EXIT_CODE").unwrap_or_default();
    let stderr_env = env::var("FAKE_GRAPH_STDERR").unwrap_or_default();

    // Write stderr if configured (simulates crash/error output)
    if !stderr_env.is_empty() {
        eprintln!("{}", stderr_env);
    }

    // Override exit code if configured
    if !exit_code_env.is_empty() {
        let code: i32 = exit_code_env.parse().unwrap_or(1);
        process::exit(code);
    }

    // Parse response map from env var
    let response_map: std::collections::HashMap<String, String> = if responses_env.is_empty() {
        std::collections::HashMap::new()
    } else {
        responses_env
            .split("||")
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, ':');
                let cmd = parts.next()?;
                let json = parts.next()?;
                Some((cmd.to_string(), json.to_string()))
            })
            .collect()
    };

    let response = if let Some(custom) = response_map.get(subcommand) {
        custom.clone()
    } else {
        // Default responses per subcommand
        match subcommand.as_str() {
            "next" => r#"{"status":"success","data":{"task_id":"t1","title":"Test task","description":"Desc","graph_revision":1,"lease_expiration":"2026-01-01T00:00:00Z","dependencies":[]}}"#.to_string(),
            "claim" => r#"{"status":"success","data":{"claimed":true,"task_id":"t1","actor":"test-agent","graph_revision":2}}"#.to_string(),
            "summarize" => r#"{"status":"success","data":{"task_id":"t1","summary":"Task summary","graph_revision":2,"dependencies":[],"recent_events":[]}}"#.to_string(),
            "release" => r#"{"status":"success","data":{"released":true,"task_id":"t1","graph_revision":3}}"#.to_string(),
            "heartbeat" => r#"{"status":"success","data":{"node_id":"t1","status":"IN_PROGRESS","actor":"test-agent","lease_expires_at":"2026-12-31T23:59:59Z"}}"#.to_string(),
            "complete" => r#"{"status":"success","data":{"node_id":"t1","status":"COMPLETED","graph_revision":3}}"#.to_string(),
            "fail" => r#"{"status":"success","data":{"node_id":"t1","status":"FAILED","graph_revision":3}}"#.to_string(),
            "block" => r#"{"status":"success","data":{"node_id":"t1","status":"BLOCKED","graph_revision":3}}"#.to_string(),
            "skip" => r#"{"status":"success","data":{"node_id":"t1","status":"SKIPPED","graph_revision":3}}"#.to_string(),
            "cancel" => r#"{"status":"success","data":{"node_id":"t1","status":"CANCELLED","graph_revision":3}}"#.to_string(),
            _ => {
                eprintln!("fake-graph: unknown subcommand '{}'", subcommand);
                process::exit(1);
            }
        }
    };

    println!("{}", response);
}
