# State & Task Graph Engine

A Rust CLI for managing DAG-based task graphs for LLM agents. Provides strict state-machine enforcement, optimistic concurrency via graph revision, task claiming with lease recovery, and bounded context views to prevent LLM context bloat.

---

## Documentation

| Document | Description |
|----------|-------------|
| [Installation](docs/installation.md) | Build, install, and verify |
| [API Reference](docs/api-reference.md) | Every command, its arguments, output, and errors |
| [Agent Integration Protocol](docs/agent-integration-protocol.md) | **How autonomous agents and LLM runtimes integrate with `stg`** |
| [Integration Guide](docs/integration-guide.md) | Shell, Python, Node.js, CI/CD, and Git hooks examples |
| [Error Codes](docs/error-codes.md) | All error codes, triggers, and recovery actions |
| [Architecture](docs/architecture.md) | Internal design: modules, pipeline, persistence, state machine |
| [Troubleshooting](docs/troubleshooting.md) | Common issues, diagnosis steps, and fixes |

---

## Quick Start

```bash
# Install
cargo install --path .

# Initialize a new project
stg init

# Load a task plan
stg append-nodes --revision 0 --file plan.yaml

# Check progress
stg status

# Get next task
stg next

# Claim it
stg claim TASK-001 --actor my-agent --ttl-seconds 300

# Complete it
stg complete TASK-001 --actor my-agent --revision 2 --result-summary "Done"

# Get bounded context for an LLM
stg summarize TASK-001 --max-events 10 --max-completed-summaries 5
```

---

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
| `reopen` | Reset a terminal state back to PENDING or READY | Terminal → PENDING/READY |

### Graph Management

| Command | Description |
|---------|-------------|
| `append-nodes` | Add new tasks from a YAML file (revision-gated) |
| `summarize` | Generate a bounded context payload for an LLM |

Full argument details, output formats, and error codes: **[API Reference](docs/api-reference.md)**

---

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

---

## For Agents and Runtimes

If you are building an autonomous agent, LLM-powered runtime, or coding assistant, start with the **[Agent Integration Protocol](docs/agent-integration-protocol.md)**. It specifies:

- The core protocol rules (parse envelope, read-before-write, handle stale revisions)
- The agent task loop (observe → claim → act → report)
- Multi-agent coordination via leases
- How to use `summarize` for bounded LLM context
- Error recovery protocols for every error code
- Framework-agnostic integration patterns (subprocess, CI/CD, agent harness)

---

## For Human Developers

If you are a developer integrating `stg` into scripts, CI/CD, or tooling, see the **[Integration Guide](docs/integration-guide.md)** for Bash, Python, Node.js, and GitHub Actions examples.

---

## Response Envelope

Every command returns JSON:

```json
{
  "ok": true,
  "graph_revision": 42,
  "warnings": [],
  "data": { ... },
  "error": null
}
```

On failure: `"ok": false` with structured `error.code`, `error.message`, and `error.details`.

Full error code reference: **[Error Codes](docs/error-codes.md)**

---

## Development

```bash
cargo build
cargo test          # 120 tests
cargo clippy -- -D warnings
cargo fmt -- --check
```

## Architecture Docs

- **[Architecture](docs/architecture.md)** — Module layout, pipeline, persistence, state machine internals
- **[Troubleshooting](docs/troubleshooting.md)** — Common issues and fixes
- `.rpiv/artifacts/` — PRD, technical spec, implementation plan, code reviews

## License

MIT