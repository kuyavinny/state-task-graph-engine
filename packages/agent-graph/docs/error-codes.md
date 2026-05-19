# Error Codes Reference

Complete reference for all error codes returned by `stage`. Every error response uses this structure:

```json
{
  "ok": false,
  "graph_revision": null,
  "warnings": null,
  "data": null,
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable description of what went wrong",
    "details": { ... }
  }
}
```

The `details` object contains structured context specific to each error code.

---

## Validation Errors

### `INVALID_SCHEMA`

**Meaning:** The task graph YAML does not conform to the expected schema.

**Details:** None (message contains the specific validation failure).

**Trigger:** Running `stage validate` on a graph with schema violations, or loading a corrupt graph file.

**Recovery:** Fix the YAML to match the [graph schema](#graph-schema). Ensure all required fields are present and correctly typed.

---

### `INVALID_YAML`

**Meaning:** The YAML file could not be parsed.

**Details:** None (message contains the parser error).

**Trigger:** Loading a file that is not valid YAML syntax.

**Recovery:** Check for YAML syntax errors (indentation, missing quotes, invalid characters). Use `yamllint` or a YAML validator.

---

### `DUPLICATE_NODE_ID`

**Meaning:** A node with this ID already exists in the graph.

**Details:**
```json
{ "id": "TASK-001" }
```

**Trigger:** `stage append-nodes` with a node ID that already exists in the graph.

**Recovery:** Use a different ID, or remove the existing node first via `stage cancel` + `stage reopen` (there is no explicit delete command in v1).

---

### `UNKNOWN_DEPENDENCY`

**Meaning:** A node references a dependency ID that does not exist in the graph.

**Details:**
```json
{ "id": "NONEXISTENT" }
```

**Trigger:** `stage append-nodes` with a node that depends on an ID not yet in the graph.

**Recovery:** Either add the dependency node first, or remove the dependency from the node definition.

---

### `CYCLE_DETECTED`

**Meaning:** The dependency graph contains a cycle.

**Details:** None (message describes the cycle).

**Trigger:** `stage validate` or any command that runs validation (all of them, via `load_validate_reconcile`).

**Recovery:** Remove circular dependencies. A node cannot depend on itself, nor can there be a loop (A→B→C→A).

---

### `VALIDATION_FAILED`

**Meaning:** One or more validation rules failed. This is a composite error containing multiple validation errors.

**Details:**
```json
{
  "count": 2,
  "errors": [
    { "code": "UNKNOWN_DEPENDENCY", "message": "...", "details": { "id": "..." } },
    { "code": "CYCLE_DETECTED", "message": "...", "details": {} }
  ]
}
```

**Trigger:** Running `stage validate` on a graph with multiple validation errors.

**Recovery:** Inspect each error in `details.errors` and fix them individually.

---

## State Transition Errors

### `INVALID_TRANSITION`

**Meaning:** The requested state transition is not allowed by the state machine.

**Details:**
```json
{
  "action": "complete",
  "current_status": "PENDING"
}
```

**Trigger:** Attempting a transition that violates the state machine rules. For example, calling `complete` on a PENDING task (must be IN_PROGRESS first).

**Recovery:** Check the task's current status via `stage status` or `stage summarize`. Follow the valid transition paths:

```
PENDING → READY → IN_PROGRESS → COMPLETED
                              → FAILED
                              → BLOCKED
                              → SKIPPED
                              → CANCELLED (from any state)
               → CANCELLED (from any state)
```

---

### `TASK_NOT_READY`

**Meaning:** The task exists but is not in the READY state.

**Details:**
```json
{ "id": "TASK-001" }
```

**Trigger:** `stage claim` on a task that is PENDING, IN_PROGRESS, COMPLETED, etc.

**Recovery:** Check current status. The task may be waiting for dependencies to complete (PENDING), already claimed (IN_PROGRESS), or finished (COMPLETED/FAILED/etc).

---

### `TASK_NOT_FOUND`

**Meaning:** No node with this ID exists in the graph.

**Details:**
```json
{ "id": "NONEXISTENT" }
```

**Trigger:** Any command that references a node ID that doesn't exist.

**Recovery:** Verify the ID. Use `stage status` to see all node IDs. The task may have been cancelled or the ID may have a typo.

---

### `MAX_ATTEMPTS_EXCEEDED`

**Meaning:** The task has been claimed and failed more than `max_attempts` times. It cannot be claimed again.

**Details:**
```json
{ "id": "TASK-001" }
```

**Trigger:** `stage claim` on a task where `attempts >= max_attempts`.

**Recovery:** Use `stage reopen` to reset the task to PENDING/READY, then claim again. Or use `stage cancel` to permanently remove it.

---

## Concurrency Errors

### `STALE_REVISION`

**Meaning:** The `--revision` you provided doesn't match the current `graph_revision`. Another process has modified the graph since you last read it.

**Details:**
```json
{
  "expected": 5,
  "provided": 3
}
```

**Trigger:** Any mutation command with `--revision` where the graph has changed.

**Recovery:**
1. Re-read the current state: `stage status`
2. Re-evaluate whether your action is still valid
3. Retry with the new revision

**Do not retry more than once without re-reading.**

---

### `LEASE_NOT_OWNED`

**Meaning:** You tried to operate on a task that is claimed by a different actor.

**Details:** None (message contains "Lease not owned by actor").

**Trigger:** Calling `complete`, `fail`, `heartbeat`, `release`, `block`, `skip` on a task claimed by someone else.

**Recovery:** Do not retry. Move to the next task. If you believe the lease is stale, wait for it to expire (TTL-based auto-recovery).

---

## File and I/O Errors

### `FILE_NOT_FOUND`

**Meaning:** The `.agent/` directory or a specified file doesn't exist.

**Details:**
```json
{ "path": "/path/to/file" }
```

**Trigger:** Running any command before `stage init`, or `stage append-nodes --file` with a non-existent file path.

**Recovery:** Run `stage init` first, or check the file path.

---

### `ATOMIC_WRITE_FAILED`

**Meaning:** The engine could not persist the graph or event log using its tempfile + rename strategy.

**Details:**
```json
{ "message": "Could not rename temp file" }
```

**Trigger:** Filesystem permission issues, disk full, or concurrent write conflicts at the OS level.

**Recovery:** Check filesystem permissions on `.agent/`, verify disk space, and ensure no other process is directly writing to `.agent/` files.

---

### `IO_ERROR`

**Meaning:** A general I/O error occurred.

**Details:** None (message contains the OS error).

**Trigger:** File permission issues, disk errors, or other OS-level I/O failures.

**Recovery:** Check filesystem permissions and disk health.

---

### `SERIALIZATION_ERROR`

**Meaning:** A file could not be parsed as valid YAML or JSON.

**Details:** None (message contains the parser error).

**Trigger:** Corrupt `.agent/task_graph.yaml` or `.agent/task_events.jsonl`.

**Recovery:** Inspect the file for corruption. For the event log, you may need to remove the malformed line(s). For the graph, restore from a backup or fix the YAML syntax manually.

---

## Argument Errors

### `INVALID_ARGUMENT`

**Meaning:** A command-line argument failed validation.

**Details:**
```json
{ "message": "ttl_seconds must be greater than 0" }
```

**Trigger:** Invalid argument values such as `--ttl-seconds 0`, missing required arguments, or wrong types.

**Recovery:** Check the command help (`stage <command> --help`) for valid argument ranges.

---

## Event Log Errors

### `EVENT_LOG_DESYNC`

**Meaning:** The event log has more events than the graph revision accounts for. The engine has auto-reconciled, but data may be partially corrupt.

**Details:** None.

**Trigger:** This appears as a **warning** (not an error) in the `warnings` array of a success response. It means the event log end position doesn't align with `graph_revision`.

**Recovery:** Log the warning. The engine continues to operate. Inspect `.agent/task_events.jsonl` for duplicated or malformed entries. This is usually caused by a crash during write.

---

## Internal Errors

### `INTERNAL`

**Meaning:** An unexpected internal error occurred.

**Details:** None.

**Trigger:** This should not happen in normal operation. It indicates a bug in `stage`.

**Recovery:** Check the error message for clues. Report the issue with reproduction steps.

---

### `NOT_IMPLEMENTED`

**Meaning:** The command exists but is not yet implemented.

**Details:** None.

**Trigger:** Calling a command that exists in the CLI but has no implementation.

**Recovery:** This should not occur in the released version. If it does, the command needs to be implemented.

---

## Error Code Quick Reference

| Code | Likely Command | Recovery |
|------|---------------|----------|
| `INVALID_SCHEMA` | validate, any | Fix YAML schema |
| `INVALID_YAML` | any | Fix YAML syntax |
| `DUPLICATE_NODE_ID` | append-nodes | Use unique ID |
| `UNKNOWN_DEPENDENCY` | append-nodes | Add dependency first |
| `CYCLE_DETECTED` | validate, any | Remove circular deps |
| `VALIDATION_FAILED` | validate | Fix each listed error |
| `INVALID_TRANSITION` | complete, fail, block, skip | Check current status |
| `TASK_NOT_READY` | claim | Check dependencies |
| `TASK_NOT_FOUND` | any with node_id | Verify ID exists |
| `MAX_ATTEMPTS_EXCEEDED` | claim | Reopen first |
| `STALE_REVISION` | complete, fail, block, skip, cancel, reopen, append-nodes | Re-read and retry |
| `LEASE_NOT_OWNED` | complete, fail, heartbeat, release | Move to next task |
| `FILE_NOT_FOUND` | any | Run `stage init` |
| `ATOMIC_WRITE_FAILED` | any | Check filesystem |
| `IO_ERROR` | any | Check permissions |
| `SERIALIZATION_ERROR` | any | Check file format |
| `INVALID_ARGUMENT` | any | Check argument values |
| `EVENT_LOG_DESYNC` | any (warning) | Log warning, inspect file |
| `INTERNAL` | any | Report bug |
| `NOT_IMPLEMENTED` | any | Should not occur in release |