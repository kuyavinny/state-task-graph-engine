# Technical Specification: Module 2 - Universal Adapter Boundary

---

## 1. Technical Overview

The Universal Adapter Boundary is a stateless CLI protocol boundary designed to mediate between external coding assistants and the authoritative State & Task Graph Engine.

It does not hold authoritative task state. It operates strictly as a translation, validation, and normalization layer.

Call chain:

```text
External Agent/Runtime -> Adapter CLI -> Graph Engine CLI -> task_graph.yaml / task_events.jsonl
```

By composing low-level Graph Engine commands into single agent-facing operations with explicit failure handling, the adapter protects the Graph Engine from runtime-specific logic, normalizes agent inputs and outputs into canonical JSON, and prevents unapproved state mutations.

---

## 2. File System Contracts

### 2.1. Adapter-Owned Files

The adapter may read and write:

- `.agent/adapter.config.yaml`
- `.agent/adapter_logs.jsonl`
- `.agent/adapter_artifacts/`

### 2.2. Prohibited Direct Access

The adapter must not:

- read, parse, or mutate `.agent/task_graph.yaml`;
- read, parse, or mutate `.agent/task_events.jsonl`;
- perform graph/event desync checks itself;
- fall back to direct graph-file mutation under any error condition.

All authoritative task-state interaction must occur through subprocess calls to the Graph Engine CLI binary.

---

## 3. Adapter Configuration Schema

```yaml
schema_version: "1.0"
graph_engine_binary_path: "./agent-graph"
default_profile: "read_only_agent"

profiles:
  - name: "read_only_agent"
    agent_identity:
      runtime: "claude_code"
      version: "1.0.0"
    capabilities:
      read_files: true
      write_files: false
      execute_shell: false
      run_tests: false
      network_access: false
      browser_access: false
      long_running_tasks: false
      max_task_minutes: 10
      preferred_format: "markdown"
      max_context_chars: 16000
    permissions:
      allow_claim: true
      allow_submit_success: true
      allow_submit_fail: true
      allow_submit_blocked: true
      allow_skip: false
      allow_cancel: false
      allow_release: true
    policies:
      result_policy: "strict_validation"
      retry_policy: "fail_fast"
      logging_policy: "debug"
      artifact_policy:
        max_copied_artifact_bytes: 1048576
        max_total_copied_bytes: 5242880

  - name: "full_exec_agent"
    agent_identity:
      runtime: "openhands"
      version: "1.5.0"
    capabilities:
      read_files: true
      write_files: true
      execute_shell: true
      run_tests: true
      network_access: true
      browser_access: false
      long_running_tasks: true
      max_task_minutes: 120
      preferred_format: "json"
      max_context_chars: 64000
    permissions:
      allow_claim: true
      allow_submit_success: true
      allow_submit_fail: true
      allow_submit_blocked: true
      allow_skip: true
      allow_cancel: true
      allow_release: true
    policies:
      result_policy: "allow_artifacts"
      retry_policy: "fail_fast"
      logging_policy: "standard"
      artifact_policy:
        max_copied_artifact_bytes: 1048576
        max_total_copied_bytes: 5242880
```

---

## 4. Capability and Permission Model

Capabilities describe what the runtime can do.

Permissions describe what the adapter allows the profile to report or request.

V1 does not perform advanced capability-based routing. These fields are used for safety, basic validation, formatting, and permissions.

### 4.1. Required Capability Fields

- `read_files`
- `write_files`
- `execute_shell`
- `run_tests`
- `network_access`
- `browser_access`
- `long_running_tasks`
- `max_task_minutes`
- `preferred_format`
- `max_context_chars`

### 4.2. Required Permission Fields

- `allow_claim`
- `allow_submit_success`
- `allow_submit_fail`
- `allow_submit_blocked`
- `allow_skip`
- `allow_cancel`
- `allow_release`

---

## 5. CLI Command Contracts

Canonical input for complex mutating adapter commands is a JSON file payload. CLI flags are convenience overrides.

All mutating commands require a `graph_revision`.

Commands:

```text
adapter init-profile
adapter validate-profile
adapter list-profiles
adapter get-work --profile <name>
adapter heartbeat --profile <name> --task-id <id> --revision <int>
adapter release-work --profile <name> --task-id <id> --revision <int> --reason <text>
adapter submit-result --profile <name> --result-file <path>
adapter submit-result --profile <name> --task-id <id> --revision <int> --status <success|fail|blocked|skipped|cancelled> --summary <text>
adapter render-context --profile <name> --task-id <id>
```

### 5.1. `render-context`

`render-context` is read-only.

It calls:

```text
graph-engine summarize <task_id>
```

It does not claim tasks and does not mutate state.

---

## 6. Canonical JSON Response Envelope

All CLI outputs use JSON envelopes. Desync warnings returned by the Graph Engine are surfaced in the `warnings` array.

### 6.1. Success Envelope

```json
{
  "ok": true,
  "adapter_version": "1.0.0",
  "profile": "full_exec_agent",
  "actor": "agent_openhands",
  "data": {},
  "warnings": []
}
```

### 6.2. Failure Envelope

```json
{
  "ok": false,
  "adapter_version": "1.0.0",
  "profile": "full_exec_agent",
  "actor": "agent_openhands",
  "error": {
    "code": "CONTEXT_STALE_REFETCH_REQUIRED",
    "source": "graph_engine",
    "message": "Graph revision mismatch. State has changed.",
    "retryable": false,
    "agent_action": "REFETCH_WORK",
    "human_action": "None",
    "details": {
      "current_revision": 45,
      "provided_revision": 44
    }
  }
}
```

---

## 7. Canonical Task Packet Schema

Returned in the `data` field of a successful `get-work` call.

The `graph_revision` returned here is the post-claim current revision.

```json
{
  "adapter_version": "1.0.0",
  "profile": "full_exec_agent",
  "actor": "agent_openhands",
  "graph_revision": 46,
  "task": {
    "id": "write_api_tests",
    "title": "Implement auth tests",
    "description": "Write Jest tests for the JWT auth middleware.",
    "status": "IN_PROGRESS",
    "lease_expires_at": "2026-05-18T20:00:00Z"
  },
  "bounded_context": {
    "parent_chain": [],
    "immediate_dependencies": [
      {
        "id": "auth_middleware",
        "status": "COMPLETED"
      }
    ],
    "dependent_tasks": [],
    "recent_events": [],
    "completed_summaries": []
  },
  "instructions": "Execute tests and verify coverage. Report SUCCESS if tests pass.",
  "reporting_requirements": ["summary", "artifacts"],
  "heartbeat_requirements": {
    "interval_seconds": 300
  },
  "constraints": {
    "read_files": true,
    "write_files": true,
    "execute_shell": true
  }
}
```

---

## 8. Canonical Result Packet Schema

When using `--result-file <path>`, the JSON must conform to this schema.

```json
{
  "adapter_version": "1.0.0",
  "profile": "full_exec_agent",
  "actor": "agent_openhands",
  "task_id": "write_api_tests",
  "graph_revision": 46,
  "status": "success",
  "summary": "Auth tests written and passing. Coverage at 95%.",
  "reason": null,
  "artifacts": [
    "./tests/auth.spec.ts"
  ],
  "evidence": [
    {
      "kind": "test_output",
      "summary": "Jest coverage summary",
      "command": "npm run test:cov",
      "artifact_path": ".agent/adapter_artifacts/coverage.json",
      "metadata": {
        "coverage_percent": 95
      }
    }
  ],
  "raw_agent_output_path": ".agent/adapter_artifacts/agent_run_123.log"
}
```

Status mappings and requirements:

- `success` -> Graph Engine `complete`; requires `summary`.
- `fail` -> Graph Engine `fail`; requires `reason`.
- `blocked` -> Graph Engine `block`; requires `reason`.
- `skipped` -> Graph Engine `skip`; requires `reason`.
- `cancelled` -> Graph Engine `cancel`; requires `reason`.

---

## 9. Graph Engine Interaction Contracts

### 9.1. `get-work`

1. Call `graph-engine next`.
2. If null or empty, return `NO_WORK_AVAILABLE`.
3. Extract `task_id` and pre-claim `graph_revision`.
4. Call `graph-engine claim <task_id> <actor> --revision <pre_claim_revision>`.
5. Extract post-claim `graph_revision` from the success envelope.
6. Call `graph-engine summarize <task_id>`.
7. Construct and return Canonical Task Packet embedding the post-claim revision.

### 9.2. `heartbeat`

```text
graph-engine heartbeat <task_id> <actor> --revision <revision>
```

### 9.3. `submit-result`

1. Validate JSON Result Packet against schema and permissions.
2. Map adapter `status` to Graph Engine mutation command.
3. Call:

```text
graph-engine <mapped_command> <task_id> <actor> --revision <revision> ...
```

4. Return normalized response.

### 9.4. `release-work`

```text
graph-engine release <task_id> <actor> --revision <revision>
```

---

## 10. Failure Behavior for Composed Operations

- **No work available:** Return `NO_WORK_AVAILABLE`.
- **Graph engine binary missing or unresponsive:** Return `GRAPH_ENGINE_UNAVAILABLE`.
- **Graph engine returns nonzero exit:** Return `GRAPH_ENGINE_NONZERO_EXIT`.
- **Graph engine returns malformed JSON:** Return `GRAPH_ENGINE_MALFORMED_JSON`.
- **`next` succeeds but `claim` fails:** Return `CLAIM_FAILED`.
- **`claim` succeeds but `summarize` fails:** Attempt best-effort `graph-engine release` only if claim returned a valid post-claim `graph_revision`. If release fails or no post-claim revision is available, return `SUMMARIZE_FAILED_AFTER_CLAIM` with `TASK_MAY_REMAIN_LEASED` in error details.
- **Stale revision on `submit-result`:** Return `CONTEXT_STALE_REFETCH_REQUIRED`.
- **Permission denied:** Return `PROFILE_PERMISSION_DENIED`.
- **Invalid packet:** Return `INVALID_RESULT_PACKET`.

---

## 11. Error Normalization

```json
{
  "code": "CONTEXT_STALE_REFETCH_REQUIRED",
  "source": "graph_engine",
  "message": "Graph revision mismatch. State has changed.",
  "retryable": false,
  "agent_action": "REFETCH_WORK",
  "human_action": "None",
  "details": {}
}
```

Standard error codes:

- `NO_WORK_AVAILABLE`
- `PROFILE_NOT_FOUND`
- `INVALID_PROFILE`
- `PROFILE_PERMISSION_DENIED`
- `GRAPH_ENGINE_UNAVAILABLE`
- `GRAPH_ENGINE_NONZERO_EXIT`
- `GRAPH_ENGINE_MALFORMED_JSON`
- `CONTEXT_STALE_REFETCH_REQUIRED`
- `CLAIM_FAILED`
- `SUMMARIZE_FAILED_AFTER_CLAIM`
- `INVALID_RESULT_PACKET`
- `ARTIFACT_POLICY_VIOLATION`
- `LEASE_NOT_OWNED`
- `TASK_MAY_REMAIN_LEASED`
- `UNKNOWN_ADAPTER_ERROR`

---

## 12. Adapter Logging Contract

Logs are written to `.agent/adapter_logs.jsonl` for debugging translation logic. These logs are not authoritative task state.

```json
{
  "timestamp": "2026-05-18T20:05:12Z",
  "adapter_version": "1.0.0",
  "profile": "read_only_agent",
  "actor": "agent_claude",
  "command": "submit-result",
  "task_id": "read_logs",
  "event_type": "translation_success",
  "graph_command_invoked": "complete",
  "result": "success",
  "error_code": null,
  "artifact_references": [],
  "metadata": {
    "graph_revision_submitted": 45
  }
}
```

---

## 13. Artifact Handling Contract

- **Referenced in Place:** Large project source files or outputs exceeding `max_copied_artifact_bytes` remain in the project tree and are referenced by path.
- **Copied Artifacts:** Temporary LLM logs, rendered prompts, raw outputs, and adapter diagnostics below configured size limits may be copied into `.agent/adapter_artifacts/`.
- **Size Limits:** Configured by `max_copied_artifact_bytes` and `max_total_copied_bytes`.
- **Path Safety:** The adapter rejects artifact paths traversing outside the project root unless explicitly permitted.
- **No File Rewriting:** The adapter must not blindly move, rewrite, or normalize project files.

---

## 14. Markdown Rendering Contract

When `render-context` formats the Canonical Task Packet JSON into Markdown:

Immutable core, never truncated:

- `task.id`
- `task.title`
- `task.description`
- `graph_revision`
- `lease_expires_at`
- `immediate_dependencies`
- `reporting_requirements`

Truncated context:

- `recent_events`
- `completed_summaries`

By default, `render-context` still returns the standard JSON envelope:

```json
{
  "ok": true,
  "adapter_version": "1.0.0",
  "profile": "full_exec_agent",
  "actor": "agent_openhands",
  "data": {
    "format": "markdown",
    "content": "...",
    "truncated": false
  },
  "warnings": []
}
```

---

## 15. Security and Safety Requirements

- Invoke Graph Engine commands using explicit subprocess argument arrays.
- Never use shell interpolation.
- Do not execute worker-provided shell commands.
- Do not run tests.
- Do not verify code.
- Do not mutate Graph Engine files directly.
- Normalize paths before artifact handling.
- Reject unsafe path traversal unless explicitly configured.

---

## 16. Non-Functional Requirements

- Stateless CLI.
- Local-first.
- Stable JSON.
- No daemon.
- No cloud dependency.
- Low overhead.
- Inspectable logs.
- Multiple profiles without code changes.

---

## 17. Acceptance Test Scenarios

- Validate valid/invalid config parsing.
- `get-work` executes `next`, `claim`, and `summarize`.
- `get-work` returns post-claim revision.
- Composite failure handling: `claim` succeeds, `summarize` fails, best-effort release is attempted only when safe.
- `submit-result` maps statuses correctly.
- Malformed result packets are rejected locally before Graph Engine invocation.
- Stale revision returns `CONTEXT_STALE_REFETCH_REQUIRED`.
- Markdown rendering preserves core fields and truncates only peripheral context.
- Artifact limits enforce `max_copied_artifact_bytes`.
- Adapter logs accurately trace operations.
- Adapter never directly reads Graph Engine state files.
- `heartbeat` and `release-work` require revision.

---

## 18. Out of Scope

V1 excludes:

- HTTP, REST, GraphQL, and MCP.
- Daemon mode.
- Task execution.
- Verification.
- Planning.
- Multi-agent capability-based routing.
- Direct Graph Engine file access.

---

## 19. Resolved Implementation Direction

The v1 implementation will preserve strict module boundaries by compiling a separate Rust binary named `agent-adapter`.

It will communicate with the existing State & Task Graph Engine binary, `agent-graph`, only through subprocess argument arrays.
