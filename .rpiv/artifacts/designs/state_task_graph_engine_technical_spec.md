# Technical Specification: State & Task Graph Engine

---

## 1. File System & Persistence Contracts

The engine operates locally within the target project directory. It requires no background daemon. It reads and writes strictly to the following canonical v1 formats:

- **Graph State:** `.agent/task_graph.yaml`
  - The only canonical v1 graph format.
  - Chosen for human readability and standard Git conflict resolution.

- **Event Log:** `.agent/task_events.jsonl`
  - The only canonical v1 event format.
  - Append-only and machine-readable.

### 1.1. Atomic Write Requirements

To prevent file corruption during execution crashes, the engine must write graph updates using a temporary file, such as `.agent/task_graph.yaml.tmp`, followed by an atomic OS-level rename operation.

The event log must be appended immediately after a successful graph write.

The `validate()` routine must check for obvious desyncs between the graph revision and the highest event log revision, warning the operator if detected.

---

## 2. Data Schemas

### 2.1. Graph Document: `.agent/task_graph.yaml`

```yaml
schema_version: "1.0"
graph_revision: 42
nodes:
  - id: "string"                    # Unique identifier
    parent_id: "string | null"      # For tree-like UI grouping
    title: "string"                 # Short, human-readable name
    description: "string"           # Detailed task instructions
    priority: 1                     # Integer; higher number = higher priority
    status: "PENDING | READY | IN_PROGRESS | BLOCKED | COMPLETED | FAILED | CANCELLED | SKIPPED"
    dependencies: ["node_1"]        # Array of required node IDs
    created_at: "ISO8601 string"
    updated_at: "ISO8601 string"
    attempts: 0                     # Integer; increments on claim
    max_attempts: 3                 # Integer
    lease:
      claimed_by: "string | null"
      claimed_at: "ISO8601 string | null"
      expires_at: "ISO8601 string | null"
    result_summary: "string | null" # Brief outcome description
    failure_reason: "string | null" # Populated on FAILED
    blocked_reason: "string | null" # Populated on BLOCKED
    skip_reason: "string | null"    # Populated on SKIPPED
    cancel_reason: "string | null"  # Populated on CANCELLED
    evidence: []                    # Array of validation/success signals
    artifacts: []                   # Array of file paths, e.g. ["./logs/err.txt"]
    data: {}                        # Small, serializable key-value context
```

### 2.1.1. Data Constraint

Large outputs must be stored in the file system, and their paths must be referenced in the `artifacts` array. The `data` object must remain small and serializable. It must not store large text blobs, source code dumps, logs, binary data, or long agent transcripts.

---

### 2.2. Event Log Document: `.agent/task_events.jsonl`

Each line is a standalone JSON object.

```json
{
  "event_id": "uuid-string",
  "timestamp": "2026-05-17T23:00:55Z",
  "graph_revision_before": 42,
  "graph_revision_after": 43,
  "node_id": "auth_setup",
  "actor": "worker_claude_1",
  "action": "complete",
  "from_status": "IN_PROGRESS",
  "to_status": "COMPLETED",
  "reason": "Tests passed successfully.",
  "metadata": {}
}
```

The event log must capture:

- State transitions.
- Rejected writes.
- Validation failures.
- Lease expirations.
- Summary updates.
- Conflict detections.
- Initialization events.

---

## 3. State Machine & Validation Rules

### 3.1. Command-Specific Transition Rules

Transitions are strictly bound to explicit commands.

- `claim`: `READY` → `IN_PROGRESS`
- `complete`: `IN_PROGRESS` → `COMPLETED`
  - This means completion was reported by an external actor or verifier and accepted by the engine.
  - The engine itself does not verify output quality.
- `fail`: `IN_PROGRESS` → `FAILED`
- `block`: `IN_PROGRESS` → `BLOCKED`
- `release`: `IN_PROGRESS` → `READY`
- `skip`: `PENDING | READY | BLOCKED` → `SKIPPED`
- `cancel`: Any non-terminal state → `CANCELLED`
- `reopen`: Terminal states or `BLOCKED` → `PENDING` or `READY`, following automatic dependency recalculation.
- `automatic dependency resolution`: `PENDING` → `READY`
  - Triggered internally when all dependencies reach `COMPLETED` or `SKIPPED`.

### 3.2. Lazy Lease Evaluation & Heartbeats

The engine requires no background daemon. Leases are evaluated lazily whenever the engine is invoked for a read or write command.

If the engine observes an `IN_PROGRESS` node where `expires_at` is in the past:

- If `attempts < max_attempts`:
  - Clear the lease.
  - Transition the node to `READY`.
  - Append a lease-expiration event.

- If `attempts >= max_attempts`:
  - Clear the lease.
  - Transition the node to `FAILED`.
  - Append a lease-expiration / failure event.

### 3.2.1. Heartbeat

Actors can invoke the `heartbeat` command to extend `expires_at` on a lease they own.

### 3.2.2. Attempts Rule

- `claim()` increments `attempts`.
- Lease expiry does not increment `attempts`.
- Lease expiry only evaluates existing `attempts` against `max_attempts`.

---

## 3.3. Concurrency Control: Strict V1 Optimistic Concurrency

- Every write request must supply the `graph_revision` it believes is current.
- If `request.revision != current_graph.revision`, the engine strictly rejects the write with `STALE_REVISION`.
- Automatic merging of non-conflicting changes is deferred beyond v1.

---

## 3.4. Human-Edit Recovery

Every CLI / API command strictly reloads and validates `task_graph.yaml` before operating.

If a human operator introduces malformed YAML, a cycle, invalid state, missing required field, or invalid transition condition, the engine must fail immediately with structured, repair-oriented validation errors.

The engine must never silently fix invalid human edits.

---

## 4. API & CLI Action Contracts

The generic `update_status` command is not part of the external API. External consumers must use explicit intent-based commands.

### 4.1. Core Commands

- `init()`
  - Initializes `.agent/` directory.
  - Writes empty graph and event log.

- `validate()`
  - Parses graph.
  - Runs cycle detection.
  - Checks referential integrity.
  - Checks schema adherence.

- `status()`
  - Returns aggregate counts and overall graph health.

- `next()`
  - Evaluates lazy leases.
  - Reconciles dependency readiness.
  - Returns the highest-priority `READY` task.

- `append-nodes(revision, nodes: list)`
  - Ingests new nodes.
  - Validates DAG.
  - Applies `PENDING` / `READY` states.

### 4.2. State Commands Requiring Revision ID

- `claim(node_id, actor, ttl_seconds)`
  - Increments attempts.
  - Sets lease.
  - Moves node to `IN_PROGRESS`.

- `heartbeat(node_id, actor, ttl_seconds)`
  - Extends active lease.

- `release(node_id, actor)`
  - Clears lease.
  - Moves node to `READY`.

- `complete(node_id, actor, revision, result_summary, artifacts[])`

- `fail(node_id, actor, revision, failure_reason)`

- `block(node_id, actor, revision, blocked_reason)`

- `skip(node_id, actor, revision, skip_reason)`

- `cancel(node_id, actor, revision, cancel_reason)`

- `reopen(node_id, actor, revision)`
  - Clears terminal state and reason fields.
  - Recalculates `PENDING` / `READY`.

---

## 5. Deterministic Ordering & Querying

When `next()` is called, the engine filters for `READY` nodes and sorts them deterministically using the following cascading logic:

1. `priority`: descending; higher numbers execute first.
2. `created_at`: ascending; older tasks execute first.
3. `id`: ascending; alphabetical fallback for absolute determinism.

---

## 6. Context View Payload: `summarize`

### 6.1. Signature

```text
summarize(node_id: string, max_events: int = 10, max_completed_summaries: int = 5, include_blocked: bool = true)
```

### 6.2. Purpose

Returns a bounded, strictly controlled payload to prevent LLM context bloat.

### 6.3. Response Shape

```json
{
  "graph_revision": 45,
  "active_task": {
    "id": "...",
    "title": "...",
    "description": "...",
    "data": {}
  },
  "parent_chain": [
    {
      "id": "parent_objective",
      "title": "..."
    }
  ],
  "immediate_dependencies": [
    {
      "id": "dep_1",
      "status": "COMPLETED",
      "result_summary": "..."
    }
  ],
  "dependent_tasks": [
    {
      "id": "downstream_1",
      "title": "..."
    }
  ],
  "blocked_or_failed_related": [
    {
      "id": "sibling_task",
      "status": "BLOCKED",
      "blocked_reason": "..."
    }
  ],
  "recent_events": [
    {
      "timestamp": "...",
      "action": "...",
      "reason": "..."
    }
  ],
  "completed_summaries": [
    {
      "id": "...",
      "title": "...",
      "result_summary": "..."
    }
  ],
  "operator_notes": "Extracted from top-level graph data, if any."
}
```

### 6.4. Summarization Constraint

`summarize()` must not call an LLM, parse large artifacts, or inject the full graph by default. It only selects bounded data from graph fields and recent event entries.
