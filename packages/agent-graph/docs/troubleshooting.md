# Troubleshooting

Common issues, their causes, and how to fix them.

---

## "File not found" or `.agent/` doesn't exist

**Symptom:**
```json
{"ok":false,"error":{"code":"FILE_NOT_FOUND","message":"...","details":{"path":"..."}}}
```

**Cause:** You haven't run `stage init` in this directory yet.

**Fix:**
```bash
stage init
```

This creates `.agent/task_graph.yaml` and `.agent/task_events.jsonl`.

---

## "Stale revision" error

**Symptom:**
```json
{"ok":false,"error":{"code":"STALE_REVISION","message":"Stale revision: expected 5, got 3","details":{"expected":5,"provided":3}}}
```

**Cause:** Another process or agent modified the graph between your last read and your write attempt.

**Fix:**
1. Re-read the current state:
   ```bash
   stage status
   ```
2. Verify your intended action is still valid (the task state may have changed).
3. Retry the mutation with the new revision:
   ```bash
   stage complete TASK-001 --actor my-bot --revision 5 --result-summary "Done"
   ```

**Do not retry more than once without re-reading.** If you get `STALE_REVISION` twice in a row, another agent is actively working on the graph. Wait and re-evaluate.

---

## Task stuck in PENDING status

**Symptom:** `stage next` returns "No READY tasks available" but tasks are PENDING.

**Cause:** The task has dependencies that haven't completed yet. PENDING tasks are promoted to READY only when ALL their dependencies are COMPLETED or SKIPPED.

**Diagnosis:**
```bash
stage summarize STUCK-TASK-ID
```

Check `immediate_dependencies` — these are the nodes blocking the promotion. At least one of them is not COMPLETED or SKIPPED.

**Fix:**
1. Complete, skip, or cancel the blocking dependency task.
2. Or restructure the graph to remove the dependency.

---

## "Lease not owned by actor" error

**Symptom:**
```json
{"ok":false,"error":{"code":"LEASE_NOT_OWNED","message":"Lease not owned by actor","details":{}}}
```

**Cause:** You're trying to operate on a task claimed by a different agent.

**Fix:**
- **Don't retry.** Another agent owns this task.
- Use `stage next` to find an available task.
- If you believe the lease is stale, wait for it to expire (check `lease.expires_at` in the graph).

---

## "Task not ready" error

**Symptom:**
```json
{"ok":false,"error":{"code":"TASK_NOT_READY","message":"Task not ready: TASK-001","details":{"id":"TASK-001"}}}
```

**Cause:** You tried to `claim` a task that isn't in the READY state. It might be PENDING (dependencies not met), IN_PROGRESS (already claimed), or in a terminal state.

**Diagnosis:**
```bash
stage status
```

Check the status breakdown. The task might be:
- **PENDING:** Dependencies not yet completed. Complete its dependencies first.
- **IN_PROGRESS:** Already claimed by another agent. Wait or move on.
- **COMPLETED/FAILED/etc.:** Already in a terminal state. Use `stage reopen` if needed.

---

## "Invalid transition" error

**Symptom:**
```json
{"ok":false,"error":{"code":"INVALID_TRANSITION","message":"Invalid state transition: cannot complete on PENDING","details":{"action":"complete","current_status":"PENDING"}}}
```

**Cause:** The state transition you attempted is not allowed. For example, you can't `complete` a PENDING task — it must be IN_PROGRESS first.

**Fix:** Follow the valid state machine transitions:
```
PENDING → READY → IN_PROGRESS → COMPLETED/FAILED/BLOCKED/SKIPPED
```
1. Check current status: `stage status`
2. If PENDING, wait for dependencies to complete (auto-promotion to READY).
3. If READY, `claim` it first to get IN_PROGRESS.
4. Then you can `complete`, `fail`, `block`, or `skip`.

---

## "Max attempts exceeded" error

**Symptom:**
```json
{"ok":false,"error":{"code":"MAX_ATTEMPTS_EXCEEDED","message":"Max attempts exceeded for task: TASK-001","details":{"id":"TASK-001"}}}
```

**Cause:** The task has been claimed and failed more times than `max_attempts` allows. The engine won't accept another `claim`.

**Fix:**
```bash
# Reopen the task to reset it
stage reopen TASK-001 --actor my-bot --revision "$(stage status | jq -r '.data.revision')"

# Then claim again
stage claim TASK-001 --actor my-bot --ttl-seconds 300
```

---

## "Cycle detected" error

**Symptom:**
```json
{"ok":false,"error":{"code":"CYCLE_DETECTED","message":"Cycle detected in dependencies","details":{}}}
```

**Cause:** The dependency graph contains a loop (e.g., A→B→C→A).

**Fix:**
1. Inspect the graph to find the cycle:
   ```bash
   cat .agent/task_graph.yaml | grep -A2 "dependencies:"
   ```
2. Remove the circular dependency by editing the node YAML in `plan.yaml` and re-appending, or using `stage cancel` on one node and recreating it without the circular dependency.

---

## "Unknown dependency" error

**Symptom:**
```json
{"ok":false,"error":{"code":"UNKNOWN_DEPENDENCY","message":"Unknown dependency: NONEXISTENT","details":{"id":"NONEXISTENT"}}}
```

**Cause:** A node references a dependency ID that doesn't exist in the graph.

**Fix:** Either:
1. Add the dependency node first: `stage append-nodes --revision N --file file-with-dependency.yaml`
2. Or remove the dependency from the node definition

---

## EVENT_LOG_DESYNC warning

**Symptom:**
```json
{"ok":true,"warnings":["EVENT_LOG_DESYNC"],"data":{...}}
```

**Cause:** The event log has more events than the graph revision accounts for. This happens when a write is interrupted (crash, power loss) after events are appended but before the graph is updated.

**Impact:** The engine auto-reconciles. The graph is consistent. Operations continue normally.

**Action:**
- **Log the warning** for monitoring.
- Inspect `.agent/task_events.jsonl` for the last few entries.
- If the warning persists across multiple operations, the event log may have duplicate entries. You can safely remove duplicate lines (same `event_id`).
- If you want to force a clean state: back up `.agent/task_events.jsonl`, then re-initialize with `stage init` and `stage append-nodes`.

---

## Corrupt YAML/JSONL files

**Symptom:**
```json
{"ok":false,"error":{"code":"SERIALIZATION_ERROR","message":"..."}}
```
or
```json
{"ok":false,"error":{"code":"INVALID_YAML","message":"..."}}
```

**Cause:** `.agent/task_graph.yaml` or `.agent/task_events.jsonl` is corrupted.

**Diagnosis:**
```bash
# Check YAML validity
python3 -c "import yaml; yaml.safe_load(open('.agent/task_graph.yaml'))"

# Check JSONL validity
cat .agent/task_events.jsonl | python3 -c "import sys, json; [json.loads(line) for line in sys.stdin]"
```

**Fix for YAML:**
- Fix the YAML syntax errors manually.
- If unrecoverable, restore from a backup or re-create the graph.

**Fix for JSONL:**
- Remove malformed lines from `.agent/task_events.jsonl`.
- Remove duplicate entries (same `event_id`).
- The engine will auto-reconcile on next read.

---

## Task won't promote from PENDING to READY

**Symptom:** A task stays PENDING even though you think its dependencies are completed.

**Diagnosis:**
```bash
stage summarize TASK-ID
```

Check `immediate_dependencies` — at least one is not COMPLETED or SKIPPED. Only COMPLETED and SKIPPED count as "satisfied" dependencies. BLOCKED, FAILED, or CANCELLED dependencies do NOT satisfy the requirement.

**Fix:**
- Complete the blocking dependency.
- Or `skip` the blocking dependency (SKIPPED satisfies the dependency requirement).
- Or `cancel` the stuck dependency and add a replacement node.

---

## Revision keeps changing between reads

**Symptom:** You read `graph_revision: 5`, then immediately read `5` again, but get `6`.

**Cause:** Another agent or process is actively modifying the graph between your reads. This is normal in multi-agent setups.

**Fix:** This is expected behavior. Always capture the revision from `stage status` immediately before a mutation, and use it right away. Don't batch reads and writes across time gaps.

---

## Lease expiry is not immediate

**Symptom:** A task is claimed by another agent whose lease should have expired, but it still shows as IN_PROGRESS.

**Cause:** Lease expiry is lazy — it only happens during `load_validate_reconcile`, which runs on every read. If nobody is reading the graph, the lease isn't checked.

**Fix:**
- Run any `stage` command (e.g., `stage status`) to trigger reconciliation.
- The expired lease will be cleared automatically.

---

## `append-nodes` file not found

**Symptom:**
```json
{"ok":false,"error":{"code":"FILE_NOT_FOUND","message":"...","details":{"path":"nodes.yaml"}}}
```

**Cause:** The `--file` path doesn't exist or isn't readable.

**Fix:**
- Use an absolute path: `stage append-nodes --revision 0 --file /absolute/path/to/nodes.yaml`
- Or make sure the relative path is correct from the directory where you're running `stage`.

---

## All tasks show as PENDING and next returns "No READY tasks available"

**Symptom:** `stage status` shows all PENDING, `stage next` returns null.

**Cause:** You've just loaded tasks with `append-nodes`. All newly created tasks start as PENDING. The engine promotes PENDING → READY only when all dependencies are satisfied.

**But:** If a task has NO dependencies (it's a root task), it should be promoted immediately to READY.

**If root tasks are stuck PENDING:**
1. Run `stage status` — this triggers reconciliation and should promote them.
2. If they're still PENDING after `stage status`, check the YAML: ensure `dependencies: []` is present and empty (not null).
3. If the issue persists, run `stage validate` to check for schema errors.