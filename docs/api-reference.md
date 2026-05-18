# API Reference

Complete reference for all 16 `stg` commands, including arguments, output format, and error cases.

---

## Response Envelope

Every command returns a JSON envelope on stdout:

### Success

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": { ... },
  "error": null
}
```

### Failure

```json
{
  "ok": false,
  "graph_revision": null,
  "warnings": null,
  "data": null,
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable description",
    "details": { ... }
  }
}
```

- `graph_revision` may be `null` on failure if the graph could not be loaded.
- `warnings` may be non-empty on success. These are informational; `ok: true` always means the command succeeded.
- Exit code is `0` on success, `1` on failure.

---

## Initialization & Inspection

### `stg init`

Initialize a new project. Creates `.agent/` directory with an empty graph and event log.

**Arguments:** None

**Output:**

```json
{
  "ok": true,
  "graph_revision": 0,
  "warnings": [],
  "data": { "initialized": true },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `IO_ERROR` | Cannot create `.agent/` directory (permissions, disk full) |
| `INVALID_ARGUMENT` | `.agent/` directory already exists |

---

### `stg status`

Return the high-level progress of the graph. This is the primary way to get the current `graph_revision` for mutation commands.

**Arguments:** None

**Output:**

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": {
    "revision": 42,
    "node_count": 10,
    "status": {
      "PENDING": 2,
      "READY": 3,
      "IN_PROGRESS": 1,
      "COMPLETED": 4
    }
  },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `FILE_NOT_FOUND` | `.agent/` directory doesn't exist (run `stg init`) |
| `SERIALIZATION_ERROR` | Graph or event log file is corrupt |

---

### `stg next`

Return the highest-priority READY task. Returns `null` data.id if no tasks are available.

**Arguments:** None

**Output (task available):**

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": {
    "id": "TASK-001",
    "title": "Setup Database",
    "priority": 10,
    "status": "READY"
  },
  "error": null
}
```

**Output (no task available):**

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": {
    "message": "No READY tasks available"
  },
  "error": null
}
```

**Priority ordering:** Higher `priority` value first, then earlier `created_at`, then alphabetically by `id`.

---

### `stg validate`

Run all validation checks on the current graph.

**Arguments:** None

**Output (valid):**

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": { "valid": true },
  "error": null
}
```

**Output (invalid):** Returns `VALIDATION_FAILED` error with details array.

**Errors:**

| Code | Condition |
|------|-----------|
| `VALIDATION_FAILED` | One or more validation rules failed. `details.errors` contains each violation. |

---

## State Mutation

All mutation commands that change task state require `--revision` for optimistic concurrency, and `--actor` to identify the worker.

### `stg claim`

Lock a task with a lease. Transitions READY → IN_PROGRESS.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID to claim |
| `--actor` | positional | yes | Worker claiming the task |
| `--ttl-seconds` | flag | yes | Lease duration in seconds |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 43,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "IN_PROGRESS",
    "actor": "worker-1"
  },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `TASK_NOT_FOUND` | Node ID doesn't exist |
| `TASK_NOT_READY` | Task is not in READY state |
| `INVALID_ARGUMENT` | `ttl_seconds` is 0 |

**Notes:**
- Claiming increments the task's `attempts` counter.
- The lease `expires_at` is set to `now + ttl_seconds`.
- After TTL expires, the next read operation clears the lease and reverts the task to READY (if `attempts < max_attempts`) or FAILED (if `attempts >= max_attempts`).

---

### `stg heartbeat`

Extend an active lease. No state transition.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Must match current lease owner |
| `--ttl-seconds` | flag | yes | Additional seconds to extend lease |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 43,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "IN_PROGRESS",
    "actor": "worker-1",
    "lease_expires_at": "2026-05-18T10:15:00Z"
  },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `TASK_NOT_FOUND` | Node ID doesn't exist |
| `LEASE_NOT_OWNED` | `actor` doesn't match `lease.claimed_by` |
| `INVALID_ARGUMENT` | `ttl_seconds` is 0 or task not IN_PROGRESS |

---

### `stg release`

Release a claimed task back to READY. Transitions IN_PROGRESS → READY.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Must match current lease owner |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 44,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "READY"
  },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `TASK_NOT_FOUND` | Node ID doesn't exist |
| `LEASE_NOT_OWNED` | `actor` doesn't match `lease.claimed_by` |

---

### `stg complete`

Mark a task as completed. Transitions IN_PROGRESS → COMPLETED.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Must match current lease owner |
| `--revision` | flag | yes | Current graph revision |
| `--result-summary` | flag | yes | Brief outcome description |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 45,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "COMPLETED"
  },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `TASK_NOT_FOUND` | Node ID doesn't exist |
| `INVALID_TRANSITION` | Task not in IN_PROGRESS state |
| `STALE_REVISION` | Provided revision doesn't match current |
| `LEASE_NOT_OWNED` | `actor` doesn't match `lease.claimed_by` |

---

### `stg fail`

Mark a task as failed. Transitions IN_PROGRESS → FAILED.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Must match current lease owner |
| `--revision` | flag | yes | Current graph revision |
| `--failure-reason` | flag | yes | Description of why it failed |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 46,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "FAILED"
  },
  "error": null
}
```

---

### `stg block`

Mark a task as blocked. Transitions IN_PROGRESS → BLOCKED.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Must match current lease owner |
| `--revision` | flag | yes | Current graph revision |
| `--blocked-reason` | flag | yes | Description of what's blocking |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 47,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "BLOCKED"
  },
  "error": null
}
```

---

### `stg skip`

Intentionally bypass a task. Transitions IN_PROGRESS → SKIPPED.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Must match current lease owner |
| `--revision` | flag | yes | Current graph revision |
| `--skip-reason` | flag | yes | Why the task is being skipped |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 48,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "SKIPPED"
  },
  "error": null
}
```

**Note:** SKIPPED tasks are treated like COMPLETED for dependency resolution. Tasks depending on a SKIPPED task will be promoted to READY.

---

### `stg cancel`

Cancel a task from any state. Transitions Any → CANCELLED.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Actor cancelling |
| `--revision` | flag | yes | Current graph revision |
| `--cancel-reason` | flag | yes | Reason for cancellation |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 49,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "CANCELLED"
  },
  "error": null
}
```

**Note:** CANCELLED is a terminal state. Use `reopen` to restore it. Tasks depending on a CANCELLED task will NOT be promoted to READY.

---

### `stg reopen`

Reset a terminal state back to PENDING or READY. Transitions COMPLETED/FAILED/BLOCKED/SKIPPED/CANCELLED → PENDING/READY.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `node_id` | positional | yes | Task ID |
| `--actor` | positional | yes | Actor reopening |
| `--revision` | flag | yes | Current graph revision |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 50,
  "warnings": [],
  "data": {
    "node_id": "TASK-001",
    "status": "READY"
  },
  "error": null
}
```

**Note:** Reopen sets status based on dependency state:
- If all dependencies are COMPLETED or SKIPPED → READY
- Otherwise → PENDING

---

## Graph Management

### `stg append-nodes`

Add new tasks from a YAML file. Requires the current graph revision.

**Arguments:**

| Arg | Type | Required | Description |
|-----|------|----------|-------------|
| `--revision` | flag | yes | Current graph revision |
| `--file` | flag | yes | Path to YAML file containing node list |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 51,
  "warnings": [],
  "data": {
    "revision": 51,
    "node_count": 15,
    "events_generated": 5
  },
  "error": null
}
```

**Errors:**

| Code | Condition |
|------|-----------|
| `STALE_REVISION` | Provided revision doesn't match current |
| `DUPLICATE_NODE_ID` | A new node ID already exists in the graph |
| `UNKNOWN_DEPENDENCY` | A new node depends on an ID not in the graph |
| `INVALID_SCHEMA` | YAML doesn't match node schema |
| `FILE_NOT_FOUND` | The `--file` path doesn't exist |

**Note:** The YAML file must contain a list of nodes (not a graph). See [Agent Integration Protocol §9.3](agent-integration-protocol.md#93-node-yaml-for-append-nodes) for the format.

---

### `stg summarize`

Generate a bounded context payload for a specific task. Designed for LLM integration.

**Arguments:**

| Arg | Type | Required | Default | Description |
|-----|------|----------|---------|-------------|
| `node_id` | positional | yes | — | Task ID to summarize |
| `--max-events` | flag | no | 10 | Max recent events for this node |
| `--max-completed-summaries` | flag | no | 5 | Max completed/skipped task summaries |
| `--include-blocked` | flag | no | true | Include blocked/failed related tasks |

**Output:**

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": {
    "active_task": {
      "id": "TASK-001",
      "title": "Setup Database",
      "description": "Create schema and seed data",
      "data": null
    },
    "parent_chain": [],
    "immediate_dependencies": [],
    "dependent_tasks": [
      {"id": "TASK-002", "title": "Build API", "status": "PENDING"}
    ],
    "blocked_or_failed_related": [],
    "recent_events": [...],
    "completed_summaries": [],
    "operator_notes": null
  },
  "error": null
}
```

**Note:** This command does NOT call an LLM. It selects bounded data from the graph and event log. The payload is designed to be injected into an LLM prompt.

See [Agent Integration Protocol §5](agent-integration-protocol.md#5-the-summarize-command--building-llm-context) for detailed usage patterns.

---

## Lease Commands Quick Reference

| Command | State Transition | Revision Required | Actor Required | TTL Required |
|---------|-----------------|-------------------|---------------|-------------|
| `claim` | READY → IN_PROGRESS | No | Yes | Yes |
| `heartbeat` | — | No | Yes | Yes |
| `release` | IN_PROGRESS → READY | No | Yes | No |
| `complete` | IN_PROGRESS → COMPLETED | Yes | Yes | No |
| `fail` | IN_PROGRESS → FAILED | Yes | Yes | No |
| `block` | IN_PROGRESS → BLOCKED | Yes | Yes | No |
| `skip` | IN_PROGRESS → SKIPPED | Yes | Yes | No |
| `cancel` | Any → CANCELLED | Yes | Yes | No |
| `reopen` | Terminal → PENDING/READY | Yes | Yes | No |

---

## Lease Enforcement

All lease commands (`claim`, `heartbeat`, `release`) and state transitions (`complete`, `fail`, `block`, `skip`, `cancel`, `reopen`) verify that the `--actor` matches `lease.claimed_by` when the task is IN_PROGRESS. The only exceptions are:

- `claim`: sets a new lease (required to match if already claimed)
- `cancel`: can override any lease (no actor check required)
- `reopen`: clears the lease on the reopened task

If `--actor` doesn't match the current `lease.claimed_by`, the command returns `LEASE_NOT_OWNED`.