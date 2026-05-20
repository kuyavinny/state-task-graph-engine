# agent-workflow

Workflow & Harness Controller — Module 3 of agent-system-os.

## Overview

Orchestrates structured, repeatable workflow harnesses over Module 1 (State & Task Graph Engine) and Module 2 (Universal Adapter Boundary). Loads declarative workflow definitions, manages per-run state, enforces phase entry/exit criteria and approval gates, and routes all task mutations through `agent-adapter`.

## CLI Commands

| Command | Description |
|---------|-------------|
| `agent-workflow init-run --workflow <id> --profile <name>` | Initialize a new workflow run |
| `agent-workflow step --run-id <id>` | Dispatch next task (if entry criteria met) |
| `agent-workflow step --run-id <id> --result-file <path>` | Submit result and advance phase |
| `agent-workflow step --run-id <id> --approve APPROVED --reason "ok"` | Resolve approval gate |
| `agent-workflow status --run-id <id>` | Show current run status |
| `agent-workflow list-runs [--workflow <id>]` | List all runs |
| `agent-workflow cancel-run --run-id <id>` | Cancel a run |
| `agent-workflow show-phase --run-id <id>` | Show current phase details |
| `agent-workflow validate --workflow <id>` | Validate workflow definition |

## Architecture

- `src/model.rs` — Workflow definition, phases, criteria structs
- `src/criteria/` — Evaluation engine (graph_state, artifact, result, operator, time)
- `src/run_state.rs` — Per-run state, approval records, phase history
- `src/adapter_client.rs` — Subprocess wrapper for `agent-adapter`
- `src/graph_client.rs` — Read-only `stage status`/`stage validate`
- `src/step_dispatch.rs` — Task acquisition and dispatch
- `src/step_intake.rs` — Result submission and phase advancement
- `src/approval.rs` — Operator approval gate handling
- `src/verification.rs` — Module 5 placeholder (returns VERIFIER_UNAVAILABLE)

## Key Constraints

- All task mutations route through `agent-adapter` (never `stage` mutations directly)
- Result packets are parsed read-only for criteria only (never modified or re-normalized)
- Graph status is normalized via `CriteriaContext` (never raw Module 1 internals)
- Active task leases must be released before cancellation
- Atomic writes for `run_state.json` (temp file + rename)

## Error Codes

| Code | Meaning |
|------|---------|
| `WORKFLOW_DEFINITION_NOT_FOUND` | Workflow file missing |
| `INVALID_WORKFLOW_DEFINITION` | Schema or validation error |
| `UNSUPPORTED_CRITERION` | Future hook or unknown criterion |
| `WORKFLOW_ALREADY_STOPPED` | Run is completed/failed/cancelled |
| `WORKFLOW_PAUSED` | Pending approval or deferred |
| `PHASE_ENTRY_CRITERIA_NOT_MET` | Entry criteria unsatisfied |
| `PHASE_ENTRY_CRITERIA_INVALID` | Unknown key/operator in criterion |
| `VERIFIER_UNAVAILABLE` | Module 5 not installed |
| `TIMEOUT_EXPIRED` | Phase or workflow timeout |
| `MAX_RETRY_EXCEEDED` | Retry threshold reached |
| `RESULT_SUBMISSION_BLOCKED` | Exit criteria failed before adapter call |
| `ADAPTER_SUBMIT_FAILED` | Adapter returned error |
| `CANNOT_RELEASE_TASK` | Active task lease release failed |
| `BINARY_NOT_FOUND` | `stage` or `agent-adapter` binary not found |

## Testing

```bash
cargo test --workspace
cargo clippy -p agent-workflow -- -D warnings
```

Test count: 104+ unit tests across workspace.
