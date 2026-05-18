# State & Task Graph Engine

A Rust CLI for managing DAG-based task graphs for LLM agents. Provides strict state-machine enforcement, optimistic concurrency via graph revision, task claiming with lease recovery, and bounded context views to prevent LLM context bloat.

## Installation

```bash
cargo install --path .
```

## Quick Start

```bash
# 1. Initialize a new project
stg init

# 2. View graph status
stg status

# 3. Get the next available task
stg next

# 4. Claim a task (locks it for a specific worker)
stg claim TASK001 --actor worker-1 --ttl-seconds 300

# 5. Complete the task
stg complete TASK001 --actor worker-1 --result-summary "Done"
```

## Architecture

The engine is a CLI binary that maintains two files in `.agent/`:

- `task_graph.yaml` — the source-of-truth graph document
- `task_events.jsonl` — append-only event log for traceability

Every write is atomic (tempfile + rename) and every mutation is guarded by `graph_revision`.

## Commands

### Initialization & Inspection

| Command | Description |
|---------|-------------|
| `init` | Create `.agent/` directory with empty graph and event log |
| `status` | Return the high-level progress of the graph |
| `validate` | Run schema and cycle validation checks |
| `next` | Return the highest-priority READY task |

### State Mutation

| Command | Description | From → To |
|---------|-------------|-----------|
| `claim` | Lock a task with a lease and worker ID | READY → IN_PROGRESS |
| `heartbeat` | Extend an active lease | IN_PROGRESS (no status change) |
| `release` | Release a claimed task back to READY | IN_PROGRESS → READY |
| `complete` | Mark an active task as completed | IN_PROGRESS → COMPLETED |
| `fail` | Mark an active task as failed | IN_PROGRESS → FAILED |
| `block` | Mark an active task as blocked | IN_PROGRESS → BLOCKED |
| `skip` | Intentionally bypass a task | IN_PROGRESS → SKIPPED |
| `cancel` | Cancel a task | Any → CANCELLED |
| `reopen` | Reset a terminal state back to PENDING or READY | COMPLETED/FAILED/BLOCKED/SKIPPED/CANCELLED → PENDING/READY |

### Graph Management

| Command | Description |
|---------|-------------|
| `append-nodes <FILE>` | Add new tasks dynamically from a YAML file (revision-gated) |
| `summarize <NODE_ID>` | Generate a bounded context payload for an LLM |

### Summarize Options

```bash
stg summarize TASK001 \
  --max-events 10 \
  --max-completed-summaries 5 \
  --include-blocked true
```

The summarize command returns a JSON payload containing:
- `active_task` — the target task's core fields
- `parent_chain` — ancestor nodes (root → direct parent)
- `immediate_dependencies` — nodes the active task depends on
- `dependent_tasks` — nodes that depend on the active task
- `blocked_or_failed_related` — sibling nodes in blocked/failed state
- `recent_events` — filtered to this node, reverse chronological
- `completed_summaries` — recent completed/skipped nodes with result summaries
- `operator_notes` — reserved for future operator annotations (`null` in v1)

## State Machine

```
PENDING ──► READY ──► IN_PROGRESS ──► COMPLETED
               │          │              │
               │          ▼              │
               │       BLOCKED           │
               │          │              │
               │          ▼              │
               │        FAILED           │
               │          │              │
               │          ▼              │
               ◄──────  SKIPPED          │
               │                         │
               ◄──────────────────────  CANCELLED
               │
               ◄───── reopen()
```

## Graph Schema

### task_graph.yaml

```yaml
schema_version: "1.0"
graph_revision: 42
nodes:
  - id: "root"
    parent_id: null
    title: "Project Root"
    description: "Top-level task"
    priority: 0
    status: "READY"
    dependencies: []
    created_at: "2026-05-17T00:00:00Z"
    updated_at: "2026-05-17T00:00:00Z"
    attempts: 0
    max_attempts: 3
    lease:
      claimed_by: "worker-1"
      claimed_at: "2026-05-17T01:00:00Z"
      expires_at: "2026-05-17T01:05:00Z"
    result_summary: null
    failure_reason: null
    blocked_reason: null
    skip_reason: null
    cancel_reason: null
    evidence: []
    artifacts: []
    data: null
```

### task_events.jsonl

```json
{"event_id":"evt-1","timestamp":"2026-05-17T00:00:00Z","graph_revision_before":0,"graph_revision_after":1,"node_id":"root","actor":"system","action":"init","from_status":null,"to_status":null,"reason":"Graph initialized","metadata":null}
```

## Response Envelope

Every command returns a JSON envelope:

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": { ... },
  "error": null
}
```

On failure:

```json
{
  "ok": false,
  "graph_revision": null,
  "warnings": null,
  "data": null,
  "error": {
    "code": "TASK_NOT_FOUND",
    "message": "Task TASK999 does not exist",
    "details": {}
  }
}
```

## Development

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

## Architecture Docs

See `.rpiv/artifacts/` for:
- `research/` — PRD
- `designs/` — Technical spec
- `plans/` — Implementation plan
- `reviews/` — Post-merge code reviews

## License

MIT
