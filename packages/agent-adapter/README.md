# agent-adapter

A stateless CLI adapter that bridges external AI agent frameworks (Claude Code, OpenHands, Codex) to the `agent-graph` task engine via subprocess calls.

## Installation

```bash
cargo build --release -p agent-adapter
```

## Quick Start

```bash
# Initialize a profile
agent-adapter init-profile

# Get the next available task
agent-adapter get-work --profile read_only_agent

# Submit a result
agent-adapter submit-result --profile full_exec_agent --task-id task-1 --revision 3 --status success --summary "Tests passing at 95%"

# Extend a lease
agent-adapter heartbeat --profile read_only_agent --task-id task-1 --revision 3 --ttl-seconds 300

# Release a task back to READY
agent-adapter release-work --profile read_only_agent --task-id task-1 --revision 3

# Render task context as Markdown
agent-adapter render-context --profile read_only_agent
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `init-profile` | Create `.agent/` directory with default config |
| `validate-profile` | Validate the adapter config file |
| `list-profiles` | List profile names and agent identities |
| `get-work --profile <name>` | Discover → claim → summarize a task |
| `submit-result --profile <name> --task-id <id> --revision <rev> --status <status>` | Submit a task result |
| `submit-result --profile <name> --result-file <path>` | Submit from a YAML result file |
| `heartbeat --profile <name> --task-id <id> --revision <rev> --ttl-seconds <ttl>` | Extend a task lease |
| `release-work --profile <name> --task-id <id> --revision <rev>` | Release a claimed task |
| `render-context --profile <name>` | Render task context as Markdown |

## Configuration

The adapter reads `.agent/adapter.config.yaml`. Each profile defines:

- **capabilities** — what the runtime can do (read/write files, execute shell, etc.)
- **permissions** — what the adapter allows per profile (claim, submit success/fail/blocked/skipped/cancelled, release)
- **policies** — result policy, retry policy, logging policy, artifact size limits

### Result Statuses

| Status | Graph Command | Required Fields |
|--------|--------------|-----------------|
| `success` | `complete` | `summary` |
| `fail` | `fail` | `reason` |
| `blocked` | `block` | `reason` |
| `skipped` | `skip` | `reason` |
| `cancelled` | `cancel` | `reason` |

## Error Codes

| Code | Description |
|------|-------------|
| `PROFILE_NOT_FOUND` | Config file or profile not found |
| `INVALID_PROFILE` | Config validation failed |
| `INVALID_RESULT_PACKET` | Result packet validation failed |
| `PERMISSION_DENIED` | Profile lacks required permission |
| `NO_WORK_AVAILABLE` | No tasks available |
| `CONTEXT_STALE_REFETCH_REQUIRED` | Graph revision conflict |
| `CLAIM_FAILED` | Task claim failed |
| `SUMMARIZE_FAILED_AFTER_CLAIM` | Claim succeeded but summarize failed |
| `HEARTBEAT_FAILED` | Heartbeat failed |
| `RELEASE_FAILED` | Task release failed |
| `ARTIFACT_POLICY_VIOLATION` | Artifact path/size policy violation |

## Artifact Policy

The adapter distinguishes two artifact categories:

- **Project artifacts** — files in the project tree (referenced in place, never copied or rewritten)
- **Adapter artifacts** — files under `.agent/adapter_artifacts/` (copied if within size limits)

Size limits are configured in `policies.artifact_policy`:
- `max_copied_artifact_bytes` — individual adapter artifact limit (default 1MB)
- `max_total_copied_bytes` — total adapter artifact limit (default 5MB)

Paths that escape the project root via traversal (`../`, symlinks) are rejected with `ARTIFACT_POLICY_VIOLATION`.

## Safety Rules

1. **Subprocess isolation** — all graph engine calls use explicit argument arrays, never shell interpolation
2. **No direct graph file access** — the adapter never reads `.agent/task_graph.yaml` or `.agent/task_events.jsonl`
3. **Path normalization** — all file paths (result files, artifacts) are canonicalized against the project root
4. **Permission gates** — each profile explicitly enables/disables specific operations

## Render Context

The `render-context` command produces Markdown suitable for LLM context windows:

- **Core fields** (never truncated): task ID, title, description, graph revision, lease expiration, dependencies, reporting requirements
- **Peripheral fields** (truncated first): recent events, completed summaries
- Content is capped at `max_context_chars` from the profile capabilities

Output is a JSON envelope:
```json
{
  "ok": true,
  "adapter_version": "1.0.0",
  "profile": "full_exec_agent",
  "actor": "agent_openhands",
  "data": {
    "format": "markdown",
    "content": "# Task: ...",
    "truncated": false
  },
  "warnings": []
}
```

## License

Part of the state-task-graph-engine project.