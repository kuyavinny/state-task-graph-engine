# Architecture

Internal design of the State & Task Graph Engine. This document is for contributors, integrators, and anyone who needs to understand how the engine works under the hood.

---

## Overview

`stg` is a single-binary Rust CLI that manages a DAG-based task graph stored on a local filesystem. It has no external dependencies (no database, no message queue, no network layer). All state lives in two files:

```
.agent/
├── task_graph.yaml      # Source-of-truth graph document
└── task_events.jsonl    # Append-only event log
```

---

## Module Layout

```
src/
├── main.rs          # Entry point, delegates to Cli::run()
├── cli.rs           # clap CLI definitions and command handlers
├── model.rs         # Data types: Graph, Node, Status, Lease, Event, ErrorCode
├── io.rs            # File I/O: read/write YAML, JSONL, init_graph, read_events
├── response.rs      # ResponseEnvelope: success/failure JSON structure
├── error.rs         # AppError enum with error_code() and details() mappings
├── reconcile.rs     # Core engine: load_validate_reconcile + all mutation functions
└── validate.rs      # 12 validation rules: schema, referential integrity, cycles
```

---

## The Central Pipeline: `load_validate_reconcile()`

Every public command (except `init`) goes through this pipeline:

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  Read Files  │───►│   Validate   │───►│  Reconcile   │───►│   Return     │
│  (YAML+JSONL)│    │  (12 rules)  │    │  (leases+deps)│   │  (Graph+Wns) │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
```

### 1. Read

- Read `.agent/task_graph.yaml` and deserialize to `Graph`.
- Read `.agent/task_events.jsonl` and deserialize each line to `Event`.
- If either file is corrupt, return a repair-oriented error.
- The event log is optional on read (missing file → empty vec), but required on write.

### 2. Validate

Run all 12 validation rules:

| # | Rule | What It Checks |
|---|------|---------------|
| V1 | Duplicate IDs | No two nodes share an `id` |
| V2 | Self-dependency | No node lists itself in `dependencies` |
| V3 | Unknown dependency | Every dependency ID exists in the graph |
| V4 | Cycle detection | No cycles in the dependency DAG |
| V5 | Status validity | All statuses are valid `Status` enum values |
| V6 | Lease consistency | If `claimed_by` is set, `claimed_at` and `expires_at` must also be set |
| V7 | Parent exists | Every `parent_id` references an existing node |
| V8 | Priority range | Priority is a non-negative integer |
| V9 | Max attempts | `max_attempts` is > 0 |
| V10 | Required fields | Title, description, created_at, updated_at are non-empty |
| V11 | Timestamp format | created_at and updated_at are valid ISO 8601 |
| V12 | Lease timestamps | If lease fields are set, timestamps are valid ISO 8601 |

Validation errors are collected and returned together (not fail-fast).

### 3. Reconcile

Two lazy evaluations run on every read:

**Lease Expiry:**
- For every IN_PROGRESS node, check if `lease.expires_at < now`.
- If expired AND `attempts < max_attempts`: clear lease, set status to READY.
- If expired AND `attempts >= max_attempts`: clear lease, set status to FAILED.
- Generate appropriate events (LEASE_EXPIRED or MAX_ATTEMPTS_EXCEEDED).

**Dependency Resolution:**
- For every PENDING node, check if ALL dependencies are COMPLETED or SKIPPED.
- If so, promote to READY.
- Generate a DEPENDENCY_RESOLVED event.

If reconciliation modified anything, persist the updated graph before proceeding.

---

## Persistence: Atomic Writes

All writes use a tempfile + rename strategy for crash safety:

```
1. Write to .agent/task_graph.yaml.tmp
2. fsync the temp file
3. Rename .agent/task_graph.yaml.tmp → .agent/task_graph.yaml
```

**Write ordering is critical:** Events are written BEFORE the graph. This ensures:
- If the event write succeeds but the graph write fails, the next `load_validate_reconcile` will detect more events than the graph revision accounts for (`EVENT_LOG_DESYNC` warning) and auto-reconcile.
- If the graph write succeeds but the event write somehow fails, the graph is consistent but the event log may be missing the latest event (less dangerous, recoverable).

---

## Event Log

Every state change generates an event appended to `.agent/task_events.jsonl`:

| Event Action | Trigger |
|-------------|---------|
| `init` | `stg init` |
| `claim` | `stg claim` |
| `heartbeat` | `stg heartbeat` |
| `release` | `stg release` |
| `complete` | `stg complete` |
| `fail` | `stg fail` |
| `block` | `stg block` |
| `skip` | `stg skip` |
| `cancel` | `stg cancel` |
| `reopen` | `stg reopen` |
| `append_nodes` | `stg append-nodes` |
| `lease_expired` | Reconciliation (auto) |
| `dependency_resolved` | Reconciliation (auto) |

Events are append-only. The engine never modifies or deletes existing events.

---

## Mutation Flow

Every mutation follows this pattern:

```
1. load_validate_reconcile()  →  get (Graph, Vec<ReconciliationWarning>)
2. Apply mutation (claim, complete, etc.)
3. Increment graph_revision
4. Generate events
5. Write events (append to JSONL)
6. Write graph (atomic tempfile + rename)
7. Return ResponseEnvelope with new revision
```

For `--revision`-gated commands (complete, fail, block, skip, cancel, reopen, append-nodes), step 2 includes a revision check: if the provided revision doesn't match the current `graph_revision`, return `STALE_REVISION` immediately.

---

## State Machine

```
                  ┌─────────────────────────────────────────────┐
                  │                                             │
                  ▼                                             │
              PENDING ──► READY ──► IN_PROGRESS ──► COMPLETED   │
                  │           │          │                     │
                  │           │          ├─► BLOCKED ─────────┤
                  │           │          │                     │
                  │           │          ├─► FAILED ──────────┤
                  │           │          │                     │
                  │           │          └─► SKIPPED ──────────┤
                  │           │                                │
                  │           └────────────────────────────────► CANCELLED
                  │                                            │
                  └─────────────────── reopen() ────────────────┘
```

### Transition Rules

| From | To | Command | Conditions |
|------|----|---------|------------|
| PENDING | READY | (reconciliation) | All dependencies COMPLETED or SKIPPED |
| READY | IN_PROGRESS | `claim` | Lease acquired |
| IN_PROGRESS | COMPLETED | `complete` | Actor owns lease, valid revision |
| IN_PROGRESS | FAILED | `fail` | Actor owns lease, valid revision |
| IN_PROGRESS | BLOCKED | `block` | Actor owns lease, valid revision |
| IN_PROGRESS | SKIPPED | `skip` | Actor owns lease, valid revision |
| IN_PROGRESS | READY | `release` | Actor owns lease |
| Any | CANCELLED | `cancel` | Valid revision |
| COMPLETED/FAILED/BLOCKED/SKIPPED/CANCELLED | PENDING or READY | `reopen` | Valid revision |

### Lease Mechanics

- `claim` sets `lease.claimed_by`, `lease.claimed_at`, `lease.expires_at` and increments `attempts`.
- `heartbeat` extends `lease.expires_at` by `ttl_seconds` from now.
- `release` clears all lease fields and reverts status to READY (does NOT decrement `attempts`).
- On read, expired leases are auto-cleared. If `attempts < max_attempts` → READY. If `attempts >= max_attempts` → FAILED.

---

## Bounded Context: `summarize()`

The `summarize` command produces a bounded payload for LLM consumption:

```
Input: graph, events, node_id, max_events, max_completed_summaries, include_blocked

Output:
  active_task:              {id, title, description, data}
  parent_chain:             [ancestors from root to direct parent]
  immediate_dependencies:   [nodes this task depends on]
  dependent_tasks:          [nodes that depend on this task]
  blocked_or_failed_related:[siblings in BLOCKED/FAILED state] (if include_blocked)
  recent_events:            [last N events for this node] (capped at max_events)
  completed_summaries:      [recent COMPLETED/SKIPPED nodes] (capped at max_completed_summaries)
  operator_notes:            null (reserved for future use)
```

Key design decision: `summarize` does NOT call an LLM, parse large artifacts, or inject the full graph. It selects bounded data from existing graph fields and recent events.

---

## Response Envelope Structure

All commands output the same JSON structure:

```rust
struct ResponseEnvelope<T: Serialize> {
    ok: bool,                    // true on success, false on error
    graph_revision: Option<u64>, // current revision (null on early errors)
    warnings: Option<Vec<String>>, // non-fatal warnings (null on error)
    data: Option<T>,             // command-specific output (null on error)
    error: Option<ErrorBody>,    // error details (null on success)
}

struct ErrorBody {
    code: ErrorCode,    // SCREAMING_SNAKE_CASE enum
    message: String,    // Human-readable description
    details: Value,      // Structured context (varies by error code)
}
```

On success: `ok=true`, `data` populated, `error=null`, `warnings` may be non-empty.
On failure: `ok=false`, `data=null`, `error` populated, `warnings=null`.

---

## Validation Engine

The `validate.rs` module provides 12 rules. Validation is run on every `load_validate_reconcile` call, ensuring the graph is always in a valid state before any mutation.

Validation is **collective**, not fail-fast: all violations are collected before returning. This allows integrators to fix multiple issues at once.

---

## Testing Strategy

| Layer | Count | Framework |
|-------|-------|-----------|
| Unit tests | 78 | `#[test]` inline in modules |
| Init integration | 10 | `assert_cmd` + `assert_fs` |
| Reconcile integration | 23 | `assert_cmd` + `assert_fs` |
| Validation integration | 9 | `assert_cmd` + `assert_fs` |
| **Total** | **120** | |

All integration tests exercise the CLI binary as a subprocess, verifying real file I/O, argument parsing, and JSON output. No mocking.

---

## Performance Characteristics

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `load_validate_reconcile` | O(N + E + D) | N=nodes, E=events, D=deps |
| `validate` | O(N + E) | Cycle detection via DFS |
| `summarize` | O(N + E) | Bounded subset selection |
| File I/O | O(N + E) | Full read/write on every command |

The engine is designed for graphs of up to ~10,000 nodes. For very large graphs, consider sharding into multiple `.agent/` directories.