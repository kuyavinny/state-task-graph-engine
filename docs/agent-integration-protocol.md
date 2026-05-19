# Agent Integration Protocol

**This document is written for autonomous agents and LLM-powered runtimes.** It specifies how any agent — regardless of language, framework, or architecture — should interact with `stg` (agent-graph).

If you are an agent reading this: follow the protocol below exactly. It is designed to be self-contained and unambiguous.

---

## 1. What Is `stg`?

`stg` is a command-line tool that manages a directed acyclic graph (DAG) of tasks stored on disk. It provides:

- **State machine enforcement** — tasks can only transition along defined edges
- **Optimistic concurrency** — every mutation requires a revision number to detect conflicts
- **Lease-based claiming** — workers lock tasks with TTL-based leases; expired leases are auto-recovered
- **Reconciliation** — on every read, expired leases are cleared and dependency-ready tasks are promoted
- **Bounded context views** — `summarize` returns only the data an LLM needs, not the whole graph

All state lives in two files in the `.agent/` subdirectory of the working directory:
- `.agent/task_graph.yaml` — the source-of-truth graph
- `.agent/task_events.jsonl` — an append-only event log

All output is JSON on stdout. All errors are structured JSON on stdout with exit code 1.

---

## 2. Core Protocol Rules

### Rule 1: Always Parse the Envelope

Every `stg` command outputs a JSON envelope. Parse it before acting:

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
    "code": "STALE_REVISION",
    "message": "Stale revision: expected 5, got 3",
    "details": { "expected": 5, "provided": 3 }
  }
}
```

**Protocol:**
1. Read `ok`. If `false`, read `error.code` to decide what to do.
2. If `ok` is `true`, read `data` for command-specific output.
3. Always capture `graph_revision` from success responses — you will need it for mutation commands.

### Rule 2: Always Read Before Write

Before any mutation (`claim`, `complete`, `fail`, `block`, `skip`, `cancel`, `reopen`, `append-nodes`), you must first read the current graph revision. The canonical way:

```
stg status
```

This returns the current `graph_revision` in the `data` field. Use this value for `--revision` arguments.

**Do not cache revision numbers across commands.** Another agent may have mutated the graph between your reads.

### Rule 3: Handle Stale Revisions Gracefully

If you receive `STALE_REVISION`, it means another agent (or process) modified the graph after you last read it.

**Required recovery:**
1. Re-run `stg status` (or the relevant query command) to get the current revision.
2. Re-evaluate whether your intended action is still valid (the task state may have changed).
3. Retry the mutation with the new revision.

**Do not retry more than once without re-reading the state.**

### Rule 4: Handle `LeaseNotOwned` Gracefully

If you receive `LEASE_NOT_OWNED`, another agent has claimed or is working on this task.

**Required action:**
1. Do not retry the claim.
2. Move to the next available task via `stg next`.
3. Optionally log the conflict for monitoring.

### Rule 5: Check `warnings` on Every Response

Even on success (`ok: true`), the envelope may contain `warnings`. These are non-fatal conditions:

| Warning | Meaning |
|---------|---------|
| `EVENT_LOG_DESYNC` | The event log has more events than the graph revision accounts for. The engine has reconciled and continued, but the log may be partially corrupt. |

If `warnings` is non-empty, log it but do not treat it as a failure.

---

## 3. Agent Workflow

### 3.1 Single-Agent Task Loop

```
┌─────────────────────────────────┐
│  1. stg init                    │  (once, at project start)
└─────────────┬───────────────────┘
              │
              ▼
┌─────────────────────────────────┐
│  2. stg append-nodes            │  (once, load task plan)
│     --revision 0 --file plan   │
└─────────────┬───────────────────┘
              │
              ▼
┌─────────────────────────────────┐
│  3. stg next                    │──────────┐
│     (get highest-priority task) │          │ No task available
└─────────────┬───────────────────┘          │
              │ Task available                 │
              ▼                               │
┌─────────────────────────────────┐          │
│  4. stg claim TASK --actor ME   │          │
│     --ttl-seconds 300           │          │
└─────────────┬───────────────────┘          │
              │ Claimed                       │
              ▼                               │
┌─────────────────────────────────┐          │
│  5. [Do work — call tools,      │          │
│      write code, etc.]          │          │
└─────────────┬───────────────────┘          │
              │ Work done                      │
              ▼                               │
┌─────────────────────────────────┐          │
│  6. stg complete TASK --actor   │          │
│     ME --revision N             │          │
│     --result-summary "..."      │          │
└─────────────┬───────────────────┘          │
              │                               │
              └───────────────────────────────┘
```

### 3.2 Multi-Agent Coordination

Multiple agents share the same `.agent/` directory. Coordination is automatic via leases and optimistic concurrency.

**Agent A:**
```bash
stg next                          # Returns TASK-001
stg claim TASK-001 --actor A --ttl-seconds 600
# A owns TASK-001 for 600 seconds
stg complete TASK-001 --actor A --revision N --result-summary "Done"
```

**Agent B (concurrent):**
```bash
stg next                          # Returns TASK-002 (TASK-001 is claimed)
stg claim TASK-002 --actor B --ttl-seconds 600
# B works on TASK-002
```

**Agent A (crashes):** After 600 seconds, TASK-001's lease expires. Next time any agent calls `stg next` or `stg status`, the engine's reconciliation step detects the expired lease and resets the task to READY. Another agent can then claim it.

No message queue, no coordination server, no shared state beyond the `.agent/` directory.

---

## 4. Command Reference for Agents

### 4.1 Read Commands (No revision needed)

| Command | Output | Agent Use |
|---------|--------|-----------|
| `stg status` | `{"revision", "node_count", "status": {...}}` | Get current revision for mutations; check overall progress |
| `stg next` | `{"id", "title", "priority", "status"}` or `{"message": "No READY tasks available"}` | Pick the next task to work on |
| `stg validate` | `{"valid": true}` or error with `VALIDATION_FAILED` | Check graph integrity (rarely needed in agent loop) |
| `stg summarize ID` | Bounded context JSON (see §5) | Build focused prompt for LLM |
| `stg init` | `{"initialized": true}` | One-time initialization |

### 4.2 Mutation Commands (Require `--revision`)

These commands require the current `graph_revision` from a prior `stg status` call:

| Command | Required Args | Transition |
|---------|---------------|------------|
| `stg complete ID --actor A --revision N --result-summary S` | actor, revision, result_summary | IN_PROGRESS → COMPLETED |
| `stg fail ID --actor A --revision N --failure-reason S` | actor, revision, failure_reason | IN_PROGRESS → FAILED |
| `stg block ID --actor A --revision N --blocked-reason S` | actor, revision, blocked_reason | IN_PROGRESS → BLOCKED |
| `stg skip ID --actor A --revision N --skip-reason S` | actor, revision, skip_reason | IN_PROGRESS → SKIPPED |
| `stg cancel ID --actor A --revision N --cancel-reason S` | actor, revision, cancel_reason | Any → CANCELLED |
| `stg reopen ID --actor A --revision N` | actor, revision | Terminal → PENDING/READY |
| `stg append-nodes --revision N --file F` | revision, file path | (adds nodes) |

### 4.3 Lease Commands (No revision needed, require `--actor`)

| Command | Required Args | Notes |
|---------|---------------|-------|
| `stg claim ID --actor A --ttl-seconds T` | actor, ttl_seconds | Claims task; sets lease expiry |
| `stg heartbeat ID --actor A --ttl-seconds T` | actor, ttl_seconds | Extends lease by T seconds |
| `stg release ID --actor A` | actor | Releases claim; task reverts to READY |

---

## 5. The `summarize` Command — Building LLM Context

The `summarize` command is designed specifically for LLM integration. It returns a bounded payload containing only the context relevant to a specific task, preventing context window overflow.

### Usage

```bash
stg summarize TASK-001 \
  --max-events 10 \
  --max-completed-summaries 5 \
  --include-blocked true
```

### Output Structure

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": {
    "active_task": {
      "id": "TASK-001",
      "title": "Implement auth module",
      "description": "Add JWT-based authentication",
      "data": null
    },
    "parent_chain": [
      {"id": "root", "title": "Project Root", "status": "COMPLETED"}
    ],
    "immediate_dependencies": [
      {"id": "setup-db", "title": "Setup Database", "status": "COMPLETED"}
    ],
    "dependent_tasks": [
      {"id": "integrate", "title": "Integrate DB + API", "status": "PENDING"}
    ],
    "blocked_or_failed_related": [],
    "recent_events": [
      {
        "event_id": "evt-42",
        "timestamp": "2026-05-18T10:00:00Z",
        "action": "claim",
        "from_status": "READY",
        "to_status": "IN_PROGRESS",
        "reason": "Starting task"
      }
    ],
    "completed_summaries": [
      {
        "id": "setup-db",
        "title": "Setup Database",
        "result_summary": "Database schema created successfully"
      }
    ],
    "operator_notes": null
  }
}
```

### Bounding Parameters

| Parameter | Default | Purpose |
|-----------|---------|---------|
| `--max-events` | 10 | Cap recent events to prevent context bloat |
| `--max-completed-summaries` | 5 | Cap completed task summaries |
| `--include-blocked` | true | Include blocked/failed sibling nodes |

For tasks with long histories, reduce `--max-events` to 3-5. For root tasks with many dependents, reduce `--max-completed-summaries` to 3.

---

## 6. Error Recovery Protocols

### 6.1 Automatic Recovery (Handled by Engine)

| Condition | Engine Action | Agent Impact |
|-----------|---------------|-------------|
| Lease expired | Auto-clear lease, revert to READY | Task becomes available on next `stg next` |
| Dependencies completed | Auto-promote PENDING → READY | Task appears on `stg next` |
| Event log desync | Log warning, continue | Check `warnings` array; log but don't fail |

### 6.2 Agent Recovery Actions

| Error Code | Meaning | Required Action |
|-----------|---------|-----------------|
| `STALE_REVISION` | Another process modified the graph | Re-read state (`stg status`), re-evaluate, retry |
| `LEASE_NOT_OWNED` | You don't own this task's lease | Move to next task; do not retry claim |
| `TASK_NOT_FOUND` | Node ID doesn't exist | Verify ID; check if task was cancelled |
| `TASK_NOT_READY` | Task is not in READY state | Check `stg status`; dependency may not be met |
| `INVALID_TRANSITION` | State transition not allowed | Read current status; adjust workflow |
| `MAX_ATTEMPTS_EXCEEDED` | Task has been retried too many times | Human intervention required |
| `CYCLE_DETECTED` | Dependency cycle in graph | Fix graph structure; remove circular dependency |
| `UNKNOWN_DEPENDENCY` | Dependency references non-existent node | Fix or remove the dependency |
| `DUPLICATE_NODE_ID` | Node ID already exists | Use a different ID |
| `VALIDATION_FAILED` | Multiple validation errors | Inspect `error.details.errors` array |
| `FILE_NOT_FOUND` | `.agent/` directory not initialized | Run `stg init` first |
| `EVENT_LOG_DESYNC` | Event log out of sync with graph | Warning; engine auto-reconciles. Log it |
| `INVALID_ARGUMENT` | CLI argument validation failed | Check command syntax |
| `SERIALIZATION_ERROR` | File corruption (invalid YAML/JSON) | Check `.agent/` files for corruption |
| `IO_ERROR` | Filesystem error | Check permissions, disk space |
| `ATOMIC_WRITE_FAILED` | Could not write tempfile + rename | Check filesystem permissions |
| `INTERNAL` | Unexpected error | Log and report; should not occur in normal operation |

### 6.3 Retry Strategy

```
max_retries = 1  (for STALE_REVISION only)

on STALE_REVISION:
  1. stg status  →  get fresh revision
  2. re-evaluate whether action is still valid
  3. retry once with fresh revision
  4. if STALE_REVISION again → log and stop

on any other error:
  do not retry; log and handle per table above
```

---

## 7. Prompt Templates for LLM Agents

### 7.1 Task Execution Prompt

When an LLM agent picks up a task, inject context from `summarize`:

```
You are an autonomous agent working on task: {active_task.title}

Task description: {active_task.description}

Dependencies (completed work this task builds on):
{immediate_dependencies}

Recent activity:
{recent_events}

Completed work summaries:
{completed_summaries}

Dependent tasks (what depends on your work):
{dependent_tasks}

{if blocked_or_failed_related is non-empty}
Blocked or failed related tasks:
{blocked_or_failed_related}
{/if}

Execute the task, then report success or failure.
```

### 7.2 Planning Prompt (Building a Task Graph)

To create a task plan:

```yaml
# plan.yaml
- id: "TASK-001"
  parent_id: null
  title: "Setup database"
  description: "Create schema and seed data"
  priority: 10
  status: "PENDING"
  dependencies: []
  created_at: "2026-05-18T00:00:00Z"
  updated_at: "2026-05-18T00:00:00Z"
  attempts: 0
  max_attempts: 3
  lease: { claimed_by: null, claimed_at: null, expires_at: null }
  result_summary: null
  failure_reason: null
  blocked_reason: null
  skip_reason: null
  cancel_reason: null
  evidence: []
  artifacts: []
  data: null

- id: "TASK-002"
  parent_id: null
  title: "Build API layer"
  description: "REST endpoints for the application"
  priority: 8
  status: "PENDING"
  dependencies: ["TASK-001"]
  # ... (same fields as above)
```

Then: `stg append-nodes --revision 0 --file plan.yaml`

### 7.3 Summarize-Driven Replanning Prompts

When an agent reports a task as BLOCKED or FAILED:

```
The task "{active_task.title}" has been marked as {status}.
Reason: {failure_reason / blocked_reason}

Dependent tasks affected:
{dependent_tasks}

Suggest one of:
1. Fix the blocker and reopen the task
2. Skip the task and adjust dependents
3. Cancel the task and its dependents

Current graph status:
{stg status output}
```

---

## 8. Integration Patterns

### 8.1 Shell Subprocess (Any Language)

```python
import subprocess, json

def stg(*args):
    """Call stg and return parsed JSON. Raises on non-zero exit."""
    result = subprocess.run(
        ["stg"] + list(args),
        capture_output=True, text=True
    )
    envelope = json.loads(result.stdout)
    if not envelope["ok"]:
        raise RuntimeError(f"stg error: {envelope['error']['code']}: {envelope['error']['message']}")
    return envelope

revision = stg("status")["data"]["revision"]
stg("claim", "TASK-001", "--actor", "my-agent", "--ttl-seconds", "600")
stg("complete", "TASK-001", "--actor", "my-agent", "--revision", str(revision), "--result-summary", "Done")
```

### 8.2 Node.js

```javascript
const { execSync } = require('child_process');

function stg(...args) {
    const output = execSync(`stg ${args.join(' ')}`, { encoding: 'utf8' });
    const envelope = JSON.parse(output);
    if (!envelope.ok) {
        throw new Error(`${envelope.error.code}: ${envelope.error.message}`);
    }
    return envelope;
}

const revision = stg('status').data.revision;
stg('claim', 'TASK-001', '--actor', 'my-agent', '--ttl-seconds', '600');
stg('complete', 'TASK-001', '--actor', 'my-agent', '--revision', String(revision), '--result-summary', 'Done');
```

### 8.3 CI/CD (GitHub Actions)

```yaml
- name: Claim and execute next task
  env:
    ACTOR: ci-runner-${{ github.run_id }}
  run: |
    # Get next task
    NEXT=$(stg next)
    TASK_ID=$(echo "$NEXT" | jq -r '.data.id')

    if [ "$TASK_ID" = "null" ]; then
      echo "No tasks available"
      exit 0
    fi

    # Get current revision
    REVISION=$(stg status | jq -r '.data.revision')

    # Claim
    stg claim "$TASK_ID" --actor "$ACTOR" --ttl-seconds 3600

    # ... do work ...

    # Complete
    stg complete "$TASK_ID" --actor "$ACTOR" --revision "$REVISION" --result-summary "CI passed"
```

### 8.4 Agent Harness Protocol (Framework-Agnostic)

Any agent framework (LangChain, AutoGPT, CrewAI, custom) can integrate via this protocol:

```
1. INITIALIZE
   stg init
   stg append-nodes --revision 0 --file plan.yaml

2. LOOP:
   a. OBSERVE:   stg next                    → pick task
   b. CONTEXT:   stg summarize <ID>           → get bounded context
   c. CLAIM:     stg claim <ID> --actor <ME> --ttl-seconds <T>
   d. ACT:       [agent does work using LLM, tools, etc.]
   e. REPORT:    stg complete|fail|block <ID> --actor <ME> --revision <N> --result-summary|failure-reason|blocked-reason <MSG>

3. ON ERROR:
   STALE_REVISION → re-read (stg status), re-evaluate, retry once
   LEASE_NOT_OWNED → move to next task
   All others → log and stop

4. ON CRASH:
   Lease expires after TTL; task auto-reverts to READY for another agent
```

This protocol is **stateless** between iterations. Each loop iteration begins with `stg next`, which triggers reconciliation and surfaces the latest state. No in-memory state is required.

---

## 9. File Format Reference

### 9.1 Task Graph YAML

```yaml
schema_version: "1.0"
graph_revision: 42
nodes:
  - id: "unique-task-id"
    parent_id: null             # or parent task ID
    title: "Human-readable title"
    description: "What this task does"
    priority: 5                 # Higher = more urgent
    status: "PENDING"           # PENDING|READY|IN_PROGRESS|BLOCKED|FAILED|COMPLETED|SKIPPED|CANCELLED
    dependencies: []            # List of node IDs this task depends on
    created_at: "2026-05-18T00:00:00Z"
    updated_at: "2026-05-18T00:00:00Z"
    attempts: 0                 # Incremented on claim
    max_attempts: 3             # Max retries before auto-fail
    lease:
      claimed_by: null          # Actor who claimed
      claimed_at: null          # ISO 8601 timestamp
      expires_at: null          # ISO 8601 timestamp
    result_summary: null        # Set on COMPLETED
    failure_reason: null        # Set on FAILED
    blocked_reason: null        # Set on BLOCKED
    skip_reason: null           # Set on SKIPPED
    cancel_reason: null          # Set on CANCELLED
    evidence: []                # List of evidence strings
    artifacts: []               # List of artifact paths/URLs
    data: null                  # Arbitrary JSON value
```

### 9.2 Event Log JSONL

Each line is a JSON object:

```json
{
  "event_id": "evt-uuid",
  "timestamp": "2026-05-18T10:00:00Z",
  "graph_revision_before": 3,
  "graph_revision_after": 4,
  "node_id": "TASK-001",
  "actor": "worker-1",
  "action": "claim",
  "from_status": "READY",
  "to_status": "IN_PROGRESS",
  "reason": "Starting task",
  "metadata": null
}
```

### 9.3 Node YAML for `append-nodes`

When adding new nodes, create a YAML file containing a list:

```yaml
- id: "new-task-1"
  parent_id: null
  title: "New Task"
  description: "Description"
  priority: 5
  status: "PENDING"
  dependencies: ["existing-task-id"]
  created_at: "2026-05-18T00:00:00Z"
  updated_at: "2026-05-18T00:00:00Z"
  attempts: 0
  max_attempts: 3
  lease: { claimed_by: null, claimed_at: null, expires_at: null }
  result_summary: null
  failure_reason: null
  blocked_reason: null
  skip_reason: null
  cancel_reason: null
  evidence: []
  artifacts: []
  data: null
```

All fields are required. Use `null` for optional fields.

---

## 10. Concurrency Guarantees

| Guarantee | Mechanism |
|-----------|-----------|
| **Optimistic concurrency** | Every mutation checks `graph_revision`. If stale, returns `STALE_REVISION`. |
| **Atomic writes** | All file writes use tempfile + rename. No partial writes on crash. |
| **Auto-recovery** | Expired leases are cleared and tasks reverted to READY on the next read. |
| **Event ordering** | Events are appended atomically after graph write. Graph revision always matches the last event. |
| **No distributed lock** | Multiple agents can safely call `stg` concurrently on the same `.agent/` directory. The last write wins on revision; earlier writers get `STALE_REVISION`. |

---

## 11. Anti-Patterns to Avoid

1. **Don't cache task state.** Always read fresh via `stg next` or `stg status` before acting.
2. **Don't edit `.agent/` files directly.** Always use `stg` commands. Direct edits bypass validation, reconciliation, and event logging.
3. **Don't ignore `warnings`.** Even on success, check `warnings`. A desync warning means the event log may be partially corrupt.
4. **Don't retry on `LEASE_NOT_OWNED`.** Another agent owns that task. Move on.
5. **Don't hardcode revision numbers.** Always get the current revision from `stg status` or a prior success response.
6. **Don't set `ttl-seconds` to 0.** A lease with TTL=0 expires immediately. The minimum useful value is 60.
7. **Don't skip `stg init`.** The `.agent/` directory must exist before any other command works.