# Integration Guide

How to integrate `stage` into human-facing workflows: scripts, CI/CD pipelines, and development tooling.

For agent-specific integration, see [Agent Integration Protocol](agent-integration-protocol.md).

---

## Installation

### Build from Source

```bash
git clone https://github.com/kuyavinny/agent-system-os.git
cd agent-system-os
cargo install --path packages/agent-graph
```

Binary installs to `~/.cargo/bin/stage`. Make sure `~/.cargo/bin` is on your `PATH`.

### Verify Installation

```bash
stage --version
stage --help
```

---

## Quick Start

### 1. Initialize a Project

```bash
mkdir my-project && cd my-project
stage init
```

This creates `.agent/task_graph.yaml` and `.agent/task_events.jsonl`.

### 2. Create a Task Plan

Create `plan.yaml`:

```yaml
- id: "setup-repo"
  parent_id: null
  title: "Initialize Repository"
  description: "Create repo structure, add README, configure CI"
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

- id: "setup-db"
  parent_id: null
  title: "Setup Database"
  description: "Create schema and seed data"
  priority: 8
  status: "PENDING"
  dependencies: ["setup-repo"]
  created_at: "2026-05-18T00:00:01Z"
  updated_at: "2026-05-18T00:00:01Z"
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

- id: "setup-api"
  parent_id: null
  title: "Build API Layer"
  description: "Create REST endpoints"
  priority: 7
  status: "PENDING"
  dependencies: ["setup-repo"]
  created_at: "2026-05-18T00:00:02Z"
  updated_at: "2026-05-18T00:00:02Z"
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

- id: "integrate"
  parent_id: null
  title: "Integrate DB + API"
  description: "Wire API endpoints to database layer"
  priority: 5
  status: "PENDING"
  dependencies: ["setup-db", "setup-api"]
  created_at: "2026-05-18T00:00:03Z"
  updated_at: "2026-05-18T00:00:03Z"
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

### 3. Load the Plan

```bash
stage append-nodes --revision 0 --file plan.yaml
```

### 4. Work Through Tasks

```bash
# Check overall progress
stage status

# Get next task (highest priority READY task)
stage next

# Claim it
stage claim setup-repo --actor devbot --ttl-seconds 600

# ... do the work ...

# Complete it
REVISION=$(stage status | jq -r '.data.revision')
stage complete setup-repo --actor devbot --revision "$REVISION" --result-summary "Repo initialized"

# Dependencies auto-promote: setup-db and setup-api become READY
stage next  # Returns "setup-db" (priority 8 > setup-api priority 7)
```

### 5. Handle Failures

```bash
stage claim setup-db --actor devbot --ttl-seconds 600

# ... work fails ...

REVISION=$(stage status | jq -r '.data.revision')
stage fail setup-db --actor devbot --revision "$REVISION" --failure-reason "Schema migration conflict"

# Later, try again:
stage reopen setup-db --actor devbot --revision "$(stage status | jq -r '.data.revision')"
stage claim setup-db --actor devbot --ttl-seconds 600
```

---

## Integration Patterns

### Bash Scripting

```bash
#!/bin/bash
set -euo pipefail

ACTOR="bot-$(hostname)"

# Get next task
NEXT=$(stage next)
TASK_ID=$(echo "$NEXT" | jq -r '.data.id // empty')

if [ -z "$TASK_ID" ]; then
    echo "No tasks available"
    exit 0
fi

echo "Working on: $TASK_ID"

# Claim it
stage claim "$TASK_ID" --actor "$ACTOR" --ttl-seconds 300

# Do work...
do_work "$TASK_ID"

# Complete it
REVISION=$(stage status | jq -r '.data.revision')
stage complete "$TASK_ID" --actor "$ACTOR" --revision "$REVISION" --result-summary "Completed successfully"
```

### Python Automation

```python
import subprocess
import json

def stage(*args):
    """Call stage CLI and return parsed response."""
    result = subprocess.run(
        ["stage"] + list(args),
        capture_output=True, text=True
    )
    envelope = json.loads(result.stdout)

    if not envelope["ok"]:
        error = envelope["error"]
        raise STGError(error["code"], error["message"], error.get("details", {}))

    return envelope

class STGError(Exception):
    def __init__(self, code, message, details):
        self.code = code
        self.message = message
        self.details = details
        super().__init__(f"{code}: {message}")

# Usage
status = stage("status")
print(f"Graph revision: {status['data']['revision']}")
print(f"Tasks: {status['data']['status']}")

next_task = stage("next")
if next_task["data"].get("id"):
    task_id = next_task["data"]["id"]
    stage("claim", task_id, "--actor", "my-bot", "--ttl-seconds", "600")
    # ... do work ...
    revision = stage("status")["data"]["revision"]
    stage("complete", task_id, "--actor", "my-bot",
        "--revision", str(revision),
        "--result-summary", "Task completed")
```

### Node.js Integration

```javascript
const { execSync } = require('child_process');

function stage(...args) {
    const output = execSync(`stage ${args.join(' ')}`, { encoding: 'utf8' });
    const envelope = JSON.parse(output);
    if (!envelope.ok) {
        const { code, message } = envelope.error;
        throw new Error(`STG ${code}: ${message}`);
    }
    return envelope;
}

// Process tasks in a loop
function processNextTask(actor) {
    const next = stage('next');
    if (!next.data.id) return false;

    const taskId = next.data.id;
    stage('claim', taskId, '--actor', actor, '--ttl-seconds', '600');

    try {
        // ... do work ...
        const revision = stage('status').data.revision;
        stage('complete', taskId, '--actor', actor, '--revision', String(revision),
            '--result-summary', 'Completed');
        return true;
    } catch (e) {
        const revision = stage('status').data.revision;
        stage('fail', taskId, '--actor', actor, '--revision', String(revision),
            '--failure-reason', e.message);
        return true;
    }
}
```

### GitHub Actions

```yaml
name: Task Runner

on:
  workflow_dispatch:
  schedule:
    - cron: '*/15 * * * *'  # Every 15 minutes

jobs:
  run-task:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install stage
        run: cargo install --path packages/agent-graph

      - name: Initialize (first time only)
        run: |
          if [ ! -d .agent ]; then
            stage init
            stage append-nodes --revision 0 --file plan.yaml
          fi

      - name: Claim and execute task
        env:
          ACTOR: ci-${{ github.run_id }}
        run: |
          NEXT=$(stage next)
          TASK_ID=$(echo "$NEXT" | jq -r '.data.id // empty')

          [ -z "$TASK_ID" ] && echo "No tasks" && exit 0

          stage claim "$TASK_ID" --actor "$ACTOR" --ttl-seconds 3600

          # Do work...
          ./run-task.sh "$TASK_ID"

          REVISION=$(stage status | jq -r '.data.revision')
          stage complete "$TASK_ID" --actor "$ACTOR" \
            --revision "$REVISION" \
            --result-summary "CI pass ${{ github.run_number }}"
```

### Git Hooks Integration

Use `stage` to track task progress alongside commits:

```bash
# .git/hooks/pre-commit
#!/bin/bash
# Auto-update task status before committing
TASK_ID=$(stage next | jq -r '.data.id // empty')
if [ -n "$TASK_ID" ]; then
    echo "Currently working on: $TASK_ID"
fi
```

```bash
# .git/hooks/post-commit
#!/bin/bash
# After committing, mark the current task as complete if it was claimed by this repo
STATUS=$(stage status)
REVISION=$(echo "$STATUS" | jq -r '.data.revision')
# Find any IN_PROGRESS task claimed by "git-$(whoami)"
# (requires jq filtering of stage status output)
```

---

## Working with the Event Log

The event log (`.agent/task_events.jsonl`) is append-only. Each line is a JSON object recording every state transition:

```json
{"event_id":"evt-1","timestamp":"2026-05-18T10:00:00Z","graph_revision_before":0,"graph_revision_after":1,"node_id":"root","actor":"system","action":"init","from_status":null,"to_status":null,"reason":"Graph initialized","metadata":null}
```

### Querying Events

```bash
# Count events per action
cat .agent/task_events.jsonl | jq -r '.action' | sort | uniq -c

# Find all events for a specific task
cat .agent/task_events.jsonl | jq 'select(.node_id == "TASK-001")'

# Find all events by a specific actor
cat .agent/task_events.jsonl | jq 'select(.actor == "worker-1")'

# Get the latest event
tail -1 .agent/task_events.jsonl | jq .
```

### Event Actions

| Action | Meaning |
|--------|---------|
| `init` | Graph initialized |
| `claim` | Task claimed by a worker |
| `heartbeat` | Lease extended |
| `release` | Lease released |
| `complete` | Task completed |
| `fail` | Task marked failed |
| `block` | Task marked blocked |
| `skip` | Task skipped |
| `cancel` | Task cancelled |
| `reopen` | Task reopened from terminal state |
| `append_nodes` | New nodes added |
| `lease_expired` | Lease auto-expired (reconciliation) |
| `dependency_resolved` | PENDING → READY (reconciliation) |

---

## Using `summarize` for LLM Context

The `summarize` command provides a bounded view of the graph around a specific task. This is useful for injecting task context into LLM prompts without overflowing the context window.

```bash
# Get context for a task with default limits
stage summarize TASK-001

# Reduce context size for tasks with long histories
stage summarize TASK-001 --max-events 3 --max-completed-summaries 2

# Exclude blocked/failed tasks from context
stage summarize TASK-001 --include-blocked false
```

See the [Agent Integration Protocol](agent-integration-protocol.md#5-the-summarize-command--building-llm-context) for detailed usage patterns.

---

## Shared Repository Setup

When multiple agents work on the same project:

1. **Commit `.agent/` to git** (optional but recommended for visibility)
2. `.gitignore` should NOT exclude `.agent/` if you want shared state
3. Use `--actor` consistently — each agent/human should have a unique actor name
4. Set reasonable `--ttl-seconds` values:
   - Interactive work: 3600 (1 hour)
   - CI jobs: 1800 (30 minutes)
   - Fast automated tasks: 300 (5 minutes)

### Multi-Agent Etiquette

- **Always check `stage next`** before claiming — another agent may have claimed a task between your reads
- **Always use `--actor`** — never leave it blank or reuse another agent's name
- **Set realistic TTL** — a 1-hour TTL for a 5-minute task blocks other agents
- **Release tasks you can't complete** — `stage release TASK-ID --actor YOUR-NAME`
- **Don't edit `.agent/` files directly** — always use `stage` commands