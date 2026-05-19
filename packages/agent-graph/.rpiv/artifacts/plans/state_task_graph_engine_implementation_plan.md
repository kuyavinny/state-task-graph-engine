# Implementation Plan: State & Task Graph Engine

The v1 engine will be implemented in **Rust** as a strictly typed, fast-booting CLI binary.

---

## 1. Technology Stack

- **CLI Framework:** `clap`
- **Serialization:** `serde`, `serde_yaml`, `serde_json`
- **Time & Identifiers:** `chrono` or `time`, `uuid`
- **Error Handling:** `thiserror`
- **File I/O:** `tempfile`
- **Testing:** `assert_cmd`, `predicates`

---

## 2. Core Execution Patterns

### 2.1. Central Routine: `load_validate_reconcile()`

Every public command must execute this internal pipeline before operating:

1. **Load**
   - Read `.agent/task_graph.yaml`.
   - Read `.agent/task_events.jsonl`.
   - If invalid YAML / JSON is encountered, halt and return a structured repair-oriented error.
   - Do not silently fix malformed state.

2. **Validate**
   - Run schema checks.
   - Run referential integrity checks.
   - Run cycle detection.

3. **Reconcile: Lazy Evaluation**
   - Evaluate all `IN_PROGRESS` leases.
   - If a lease is expired and `attempts < max_attempts`, clear lease and revert to `READY`.
   - If a lease is expired and `attempts >= max_attempts`, clear lease and transition to `FAILED`.
   - Evaluate all `PENDING` nodes.
   - If all dependencies are `COMPLETED` or `SKIPPED`, promote the node to `READY`.

4. **Persist Reconciliation**
   - If reconciliation modified the graph:
     - Increment `graph_revision`.
     - Write the graph via atomic tempfile rename.
     - Append the respective events before proceeding with the user's command.

---

### 2.2. Attempts vs. Lease Behavior

- `claim()` increments `attempts`.
- Lease expiry does not increment `attempts`.
- Lease expiry only evaluates existing attempts against `max_attempts`.

---

### 2.3. Event Logging Rules

- Successful mutations increment `graph_revision` and append state-change events.
- Rejected writes and validation failures append diagnostic events where `graph_revision_before == graph_revision_after`, when this can be done safely.
- If `task_graph.yaml` cannot be parsed, diagnostic event writing must not worsen state corruption.

---

### 2.4. Standard CLI Output Envelope

All CLI output must be strictly formatted JSON.

#### Success

```json
{
  "ok": true,
  "graph_revision": 43,
  "warnings": [],
  "data": {}
}
```

#### Failure

```json
{
  "ok": false,
  "error": {
    "code": "STALE_REVISION",
    "message": "...",
    "details": {}
  }
}
```

---

### 2.5. Standard Error Codes

- `INVALID_SCHEMA`
- `INVALID_YAML`
- `DUPLICATE_NODE_ID`
- `UNKNOWN_DEPENDENCY`
- `CYCLE_DETECTED`
- `INVALID_TRANSITION`
- `STALE_REVISION`
- `LEASE_NOT_OWNED`
- `TASK_NOT_READY`
- `TASK_NOT_FOUND`
- `MAX_ATTEMPTS_EXCEEDED`
- `EVENT_LOG_DESYNC`
- `ATOMIC_WRITE_FAILED`

---

## 3. Pull Request Sequence

---

## PR 1: Rust CLI Skeleton, Schemas, Init, Atomic I/O, Response Envelope, Error Model

### Goal

Establish the foundation, strict types, and safe file operations.

### Tasks

- Initialize Rust project with `clap` and `serde`.
- Define exact Rust structs / enums for:
  - `Graph`
  - `Node`
  - `Lease`
  - `Evidence`
  - `Artifact`
  - `Event`
  - `Status`
  - `EventAction`
  - `ErrorCode`
  - `ResponseEnvelope`
- Define the `thiserror` enum for the standard error codes.
- Implement the strict JSON response envelope.
- Implement `init` command to scaffold the `.agent/` directory.
- Implement atomic write using `tempfile` and atomic rename.

### Required Tests

- CLI scaffolding works and accepts basic flags.
- `init` creates empty valid `.yaml` and `.jsonl` files.
- JSON success / failure envelopes format correctly.
- Atomic write succeeds without leaving orphaned `.tmp` files.

---

## PR 2: Validation Engine & Repair-Oriented Errors

### Goal

Guarantee graph integrity and prevent endless loops.

### Tasks

- Implement Kahn’s Algorithm for cycle detection.
- Implement referential integrity checks:
  - all IDs in `dependencies` must exist.
  - all IDs in `parent_id` must exist.
- Ensure validation failures return exact error codes such as `INVALID_YAML` or `INVALID_SCHEMA` with actionable details.
- Validate required fields.
- Validate invalid status values.
- Validate timestamp formats.
- Validate `attempts >= 0`.
- Validate `max_attempts >= 1`.
- Validate `attempts <= max_attempts`, unless explicitly allowed for historical failed nodes.
- Validate priority is a valid integer.
- Validate lease consistency.
- Validate terminal-state reason requirements.

### Lease Consistency Rules

- `IN_PROGRESS` requires:
  - `lease.claimed_by`
  - `lease.claimed_at`
  - `lease.expires_at`
- Non-`IN_PROGRESS` nodes should not retain active lease fields.

### Required Tests

- Valid DAG passes.
- Cycle in dependencies is detected and rejected.
- Unknown dependency ID is rejected.
- Duplicate node ID is rejected.
- Invalid YAML syntax returns a structured, non-panicking error.
- Invalid lease state is rejected.
- Terminal state without required reason is rejected.

---

## PR 3: Reconciliation Engine, Dependency Readiness, & Lazy Lease Expiry

### Goal

Implement the `load_validate_reconcile()` pipeline.

### Tasks

- Build the central internal routine.
- Implement lazy lease logic.
- Implement automatic `PENDING` → `READY` promotion.
- Implement the graph / event revision consistency check.
- Return `EVENT_LOG_DESYNC` as a warning in the JSON response envelope.
- Do not blindly append more events to a known-desynced log unless the behavior is explicitly safe.

### Required Tests

- Expired lease returns to `READY` if `attempts < max_attempts`.
- Expired lease becomes `FAILED` if `attempts >= max_attempts`.
- `PENDING` becomes `READY` only when all dependencies are `COMPLETED` or `SKIPPED`.
- Graph / event revision desync produces a warning but does not panic.

---

## PR 4: `append-nodes` and Revision-Gated Mutations

### Goal

Allow the graph to grow safely without race conditions.

### Tasks

- Implement `append-nodes` command to ingest new tasks dynamically.
- V1 input method:

```text
append-nodes --revision <n> --file <path>
```

- Implement strict revision checking:
  - `request.revision != current_graph.revision` yields `STALE_REVISION`.
- Validate appended nodes before commit.
- Recalculate `PENDING` / `READY` after append.
- Increment `graph_revision` on successful mutation.
- Emit node-append events.

### Required Tests

- Append valid nodes succeeds and increments revision.
- Append nodes that create a cycle fails validation.
- Stale revision is strictly rejected.
- Append from file works with multiline node descriptions.

---

## PR 5: State Commands & Lease Ownership

### Goal

Expose explicit state mutation API.

### Tasks

- Implement commands:
  - `claim`
  - `heartbeat`
  - `release`
  - `complete`
  - `fail`
  - `block`
  - `skip`
  - `cancel`
  - `reopen`
- Ensure `claim` increments `attempts`.
- Enforce lease ownership:
  - only the `claimed_by` actor can `heartbeat`, `release`, `complete`, `fail`, or `block`.
- `skip`, `cancel`, and `reopen` require actor and revision.
- If the node is `IN_PROGRESS` and leased to another actor, reject `skip`, `cancel`, or `reopen` with `LEASE_NOT_OWNED`.

### Command-Specific Required Fields

- `complete` requires `result_summary`.
- `fail` requires `failure_reason`.
- `block` requires `blocked_reason`.
- `skip` requires `skip_reason`.
- `cancel` requires `cancel_reason`.
- `reopen` requires `reason`.

### Required Tests

- `claim` increments attempts and sets lease correctly.
- Non-owner cannot execute `heartbeat`, `release`, or `complete` on a leased task.
- Invalid state transitions, such as completing a `PENDING` task, are rejected with `INVALID_TRANSITION`.
- Required command fields are enforced.
- Leased `IN_PROGRESS` node cannot be skipped, cancelled, or reopened by a non-owner.

---

## PR 6: Query Commands and Deterministic `next()` Ordering

### Goal

Provide the agent with its next immediate action.

### Tasks

- Implement `status` command.
- Implement `next` command.
- Ensure both commands call `load_validate_reconcile()` before returning.
- Implement strict sorting logic:
  1. `priority` descending.
  2. `created_at` ascending.
  3. `id` ascending.

### Required Tests

- `next()` ordering is completely deterministic across multiple test runs.
- `status` accurately counts nodes across all states.
- `status` and `next` do not return stale lease states.

---

## PR 7: Bounded Context Payload `summarize()`

### Goal

Feed the agent runtime only what it needs to avoid context bloat.

### Tasks

- Implement `summarize` command.
- CLI signature:

```text
summarize <node_id> --max-events <n> --max-completed-summaries <n> --include-blocked <true|false>
```

- Ensure `summarize` calls `load_validate_reconcile()` before returning.
- Extract bounded data from:
  - active node
  - parent chain
  - immediate dependencies
  - dependent tasks
  - related blocked or failed nodes
  - recent event entries
  - existing `result_summary` fields
- Ensure this command is a pure data-extraction routine:
  - no LLM calls
  - no parsing of large artifacts
  - no full-graph injection by default

### Required Tests

- `summarize()` output remains strictly bounded regardless of how large the underlying graph is.
- `summarize()` respects `max_events`.
- `summarize()` respects `max_completed_summaries`.
- `completed_summaries` correctly extracts `result_summary` fields.
- `summarize()` does not include the full graph by default.

---

## PR 8: CLI Docs, Examples, Fixtures, & Full Integration Test Matrix

### Goal

Finalize the v1 product for integration with coding assistants.

### Tasks

- Write `README.md`.
- Ensure CLI `--help` documentation is useful.
- Create sample `task_graph.yaml` fixtures.
- Create sample `task_events.jsonl` fixtures.
- Implement end-to-end integration tests using `assert_cmd` and `predicates`.
- Add adapter-facing contract notes.

### Required Tests

- Full lifecycle simulation:
  1. `init`
  2. `append-nodes`
  3. `next`
  4. `claim`
  5. `complete`
  6. `next`
  7. `summarize`

- Validate the output envelope and file system state at every step.

---

# Final Implementation Constraints

1. PR 1 must define all core structs / enums:
   - `Graph`
   - `Node`
   - `Lease`
   - `Evidence`
   - `Artifact`
   - `Event`
   - `Status`
   - `EventAction`
   - `ErrorCode`
   - `ResponseEnvelope`

2. The success response envelope must include warnings:

```json
{
  "ok": true,
  "graph_revision": 43,
  "warnings": [],
  "data": {}
}
```

3. Validation must cover:
   - required fields
   - duplicate IDs
   - dependency references
   - parent references
   - cycle detection
   - timestamp format
   - attempts / max_attempts validity
   - priority validity
   - status validity
   - terminal-state reason fields
   - lease consistency

4. Lease consistency rules:
   - `IN_PROGRESS` requires `claimed_by`, `claimed_at`, and `expires_at`.
   - Non-`IN_PROGRESS` nodes should not retain active lease fields.
   - `claim()` increments attempts.
   - lease expiry never increments attempts.

5. If `task_graph.yaml` is malformed and cannot be parsed, return a structured repair-oriented error. Append a diagnostic event only if it can be done safely without worsening state corruption.

6. `EVENT_LOG_DESYNC` should be returned as a warning in the JSON envelope. Do not blindly append more events to a known-desynced log unless the behavior is explicitly safe.

7. `append-nodes` v1 should accept node input from a file:

```text
append-nodes --revision <n> --file <path>
```

8. `skip`, `cancel`, and `reopen` require actor and revision. If the node is `IN_PROGRESS` and leased to another actor, reject the command with `LEASE_NOT_OWNED`.

9. Command-specific required fields:
   - `complete` requires `result_summary`.
   - `fail` requires `failure_reason`.
   - `block` requires `blocked_reason`.
   - `skip` requires `skip_reason`.
   - `cancel` requires `cancel_reason`.
   - `reopen` requires `reason`.

10. `status`, `next`, `summarize`, and all mutation commands must call `load_validate_reconcile()` before returning results or applying user-requested changes.

11. `summarize` must expose:

```text
summarize <node_id> --max-events <n> --max-completed-summaries <n> --include-blocked <true|false>
```

12. `summarize` must not call an LLM, parse large artifacts, or inject the full graph by default. It only selects bounded data from graph fields and recent event entries.
