# Implementation Plan: Module 2 - Universal Adapter Boundary

The Universal Adapter Boundary will be implemented as a separate Rust binary named `agent-adapter`.

It relies strictly on subprocess execution through argument arrays to communicate with the existing `agent-graph` binary.

---

## 1. Core Architecture Patterns

### 1.1. Subprocess Execution & Mocking

To prevent `agent-adapter` from reading authoritative graph state files directly, all interactions route through an internal `GraphEngineClient`.

For testing, the subprocess layer is abstracted behind a trait:

```rust
pub trait GraphRunner {
    fn execute(&self, args: &[&str]) -> Result<String, GraphEngineError>;
}
```

Implementations:

- `RealRunner` uses `std::process::Command` and must never use shell interpolation.
- `MockRunner` injects mock JSON responses and simulates graph engine crashes, malformed JSON, nonzero exits, and `STALE_REVISION`.

Typed graph response structs:

- `GraphSuccessEnvelope<T>`
- `GraphFailureEnvelope`
- `GraphNextPayload`
- `GraphClaimPayload`
- `GraphSummarizePayload`

Command-critical fields must be strongly typed. `serde_json::Value` may be used only for pass-through bounded context where appropriate.

### 1.2. `NO_WORK_AVAILABLE` Decision

`NO_WORK_AVAILABLE` is treated as a normalized failure envelope.

Rationale:

- The Success Envelope `data` payload for `get-work` always contains a valid `CanonicalTaskPacket`.
- No work available is not a crash, but it is also not a task packet.
- The failure envelope carries `code: NO_WORK_AVAILABLE`, `retryable: true`, and `agent_action: POLL_LATER`.

### 1.3. Strict File Boundary

The adapter must never directly read, parse, or mutate:

- `.agent/task_graph.yaml`
- `.agent/task_events.jsonl`

All authoritative task-state interaction must occur through `agent-graph`.

A dedicated test must prove the adapter can operate with a fake graph runner while graph state files are missing, unreadable, or poisoned, as long as the graph CLI response is valid.

---

## 2. Pull Request Sequence

---

## PR 1: Foundation, Configuration, and Error Model

### Goal

Establish the CLI binary, file namespace, configuration schemas, and JSON envelopes.

### Tasks

- Initialize `agent-adapter` Rust project.
- Add dependencies:
  - `clap`
  - `serde`
  - `serde_yaml`
  - `serde_json`
  - `thiserror`
- Define Rust structs for:
  - `AdapterConfig`
  - `Profile`
  - `AgentIdentity`
  - `Capabilities`
  - `Permissions`
  - `Policies`
  - `ArtifactPolicy`
- Define:
  - `SuccessEnvelope`
  - `FailureEnvelope`
  - `AdapterError`
  - standard adapter error codes
- Implement commands:
  - `init-profile`
  - `validate-profile`
  - `list-profiles`
- Ensure configuration is loaded from `.agent/adapter.config.yaml`.
- Ensure `init-profile` creates:
  - `.agent/adapter.config.yaml`
  - `.agent/adapter_artifacts/`

### Testing

- `init-profile` creates config file and `.agent/adapter_artifacts/`.
- `validate-profile` passes on valid YAML.
- `validate-profile` fails with structured JSON on invalid YAML.
- `list-profiles` extracts profile names and identities.
- All outputs use strict JSON envelopes.

### Definition of Done

The binary compiles, reads and writes only its adapter-specific configuration/artifact files, and outputs valid JSON envelopes.

---

## PR 2: Subprocess Execution, Mocking, and Logging

### Goal

Build the secure bridge to `agent-graph` and establish translation logging.

### Tasks

- Implement `GraphRunner` trait.
- Implement `RealRunner` using `std::process::Command`.
- Implement `MockRunner`.
- Implement `GraphEngineClient`.
- Implement strongly typed graph response parsing:
  - `GraphSuccessEnvelope<T>`
  - `GraphFailureEnvelope`
  - `GraphNextPayload`
  - `GraphClaimPayload`
  - `GraphSummarizePayload`
- Implement `AdapterLogger`.
- Write logs to `.agent/adapter_logs.jsonl`.

### Testing

- `MockRunner` parses simulated graph success envelopes.
- `MockRunner` parses simulated graph failure envelopes.
- Malformed graph JSON maps to `GRAPH_ENGINE_MALFORMED_JSON`.
- Nonzero graph exit maps to `GRAPH_ENGINE_NONZERO_EXIT`.
- `RealRunner` passes argument arrays without shell interpolation.
- Logging appends valid JSONL entries.
- Adapter behavior does not depend on reading graph state files directly.

### Definition of Done

The adapter can invoke mock graph commands, normalize responses, and log translation activity without directly touching task graph state files.

---

## PR 3: Task Acquisition: `get-work`

### Goal

Implement the multi-step `get-work` composition.

### Tasks

- Define `CanonicalTaskPacket`.
- Implement `get-work` command using `GraphEngineClient`.
- Flow:
  1. Call `next`.
  2. If no work exists, return `NO_WORK_AVAILABLE`.
  3. Extract `task_id` and pre-claim `graph_revision`.
  4. Call `claim <task_id> <actor> --revision <pre_claim_revision>`.
  5. Extract post-claim `graph_revision`.
  6. Call `summarize <task_id>`.
  7. Assemble and return `CanonicalTaskPacket`.
- Implement composite failure behavior:
  - If `claim` succeeds but `summarize` fails, attempt `release <task_id> --revision <post_claim_revision>` only if claim returned a valid post-claim revision.
  - If no post-claim revision exists or release fails, return `SUMMARIZE_FAILED_AFTER_CLAIM` with `TASK_MAY_REMAIN_LEASED`.

### Testing

- Simulate `NO_WORK_AVAILABLE` from `next`.
- Simulate successful `next -> claim -> summarize`.
- Verify returned task packet contains post-claim revision.
- Simulate claim failure after next.
- Simulate summarize failure after claim.
- Verify release is attempted only when post-claim revision is available.
- Verify no release is attempted if post-claim revision is unavailable.

### Definition of Done

`get-work` reliably composes underlying graph commands into a single JSON task packet or a normalized failure response.

---

## PR 4: Task Mutations & Result Submission

### Goal

Map agent outputs to graph mutations safely.

### Tasks

- Define `CanonicalResultPacket`.
- Implement file-based ingestion:

```text
adapter submit-result --profile <name> --result-file <path>
```

- Implement convenience overrides:

```text
adapter submit-result --profile <name> --task-id <id> --revision <int> --status <success|fail|blocked|skipped|cancelled> --summary <text>
```

- Validate result packet before graph calls:
  - `success` requires `summary`.
  - `fail`, `blocked`, `skipped`, and `cancelled` require `reason`.
  - `task_id` must match CLI `--task-id` if both are supplied.
  - `profile` must match CLI `--profile` if both are supplied.
  - `actor` must match resolved profile actor if supplied.
  - `graph_revision` is required.
  - status must be one of `success`, `fail`, `blocked`, `skipped`, `cancelled`.
- Implement permission validation.
- Map statuses to graph commands:
  - `success` -> `complete`
  - `fail` -> `fail`
  - `blocked` -> `block`
  - `skipped` -> `skip`
  - `cancelled` -> `cancel`
- Implement `heartbeat`.
- Implement `release-work`.
- Enforce revisions on all mutating commands.
- Intercept graph `STALE_REVISION` and map it to `CONTEXT_STALE_REFETCH_REQUIRED`.

### Testing

- Valid result file maps to correct graph command.
- Convenience flags map to correct graph command.
- Missing required summary/reason fails locally.
- Status without permission returns `PROFILE_PERMISSION_DENIED`.
- Stale revision maps to `CONTEXT_STALE_REFETCH_REQUIRED`.
- Malformed result packet is rejected before graph invocation.
- `heartbeat` without revision fails locally.
- `release-work` without revision fails locally.
- `heartbeat` passes revision to graph command.
- `release-work` passes revision to graph command.

### Definition of Done

Agents can report results, heartbeat, and release tasks through validated adapter contracts without bypassing graph-engine rules.

---

## PR 5: Artifact Handling & Policies

### Goal

Manage outputs generated by agents without corrupting the project namespace.

### Tasks

- Parse artifact paths from `CanonicalResultPacket`.
- Normalize and validate all artifact paths.
- Reject path traversal outside project root unless explicitly allowed.
- Enforce `max_copied_artifact_bytes` and `max_total_copied_bytes`.
- Distinguish project artifacts from adapter artifacts:
  - Project/source artifacts are referenced in place after path validation.
  - Adapter-owned artifacts, such as raw agent outputs, rendered prompts, temporary logs, and debug traces, may be copied into `.agent/adapter_artifacts/`.
- Do not copy, move, rewrite, or normalize project source files into adapter storage unless the result packet explicitly marks them as adapter-owned diagnostic artifacts.
- Return `ARTIFACT_POLICY_VIOLATION` for unsafe or oversized copied artifacts.

### Testing

- Path outside project root returns `ARTIFACT_POLICY_VIOLATION`.
- Copied adapter artifact exceeding individual byte limit is rejected.
- Copied adapter artifacts exceeding total command byte limit are rejected.
- Valid adapter artifact is copied safely.
- Project/source artifact is referenced in place and not copied.
- Adapter does not rewrite or move project files.

### Definition of Done

Artifacts are either safely referenced or safely copied according to policy, without corrupting the project namespace.

---

## PR 6: Markdown Rendering: `render-context`

### Goal

Translate canonical JSON into prompt-friendly Markdown while preserving JSON-first output contracts.

### Tasks

- Implement `render-context`.
- Call:

```text
graph-engine summarize <task_id>
```

- Render Markdown using manual formatting or a small templating crate.
- Preserve immutable core fields:
  - task ID
  - title
  - description
  - graph revision
  - lease expiration
  - immediate dependencies
  - reporting requirements
- Truncate only peripheral context:
  - recent events
  - completed summaries
- Enforce `max_context_chars`.
- Return standard JSON envelope by default.

### Testing

- Massive summarize payload is rendered under `max_context_chars`.
- Core task fields are never removed.
- Recent events and completed summaries are truncated first.
- JSON envelope remains valid.
- Truncation warning appears when truncation occurs.

### Definition of Done

The adapter can render bounded Markdown suitable for LLM context windows while preserving machine-readable CLI output.

---

## PR 7: E2E Integration Tests & Finalization

### Goal

Prove CLI contracts from a black-box perspective.

### Tasks

- Use `assert_cmd` and `predicates` to test compiled `agent-adapter`.
- Prefer a small Rust fake `agent-graph` test binary or cross-platform test harness.
- Bash script fake is allowed only for optional Unix smoke tests.
- Verify `agent-adapter` interacts with fake `agent-graph` through actual subprocess execution.
- Finalize `README.md`.
- Document:
  - JSON contracts
  - errors
  - CLI flags
  - adapter profile examples
  - artifact policy
  - no-direct-graph-file-access rule

### Testing

- End-to-end CLI invocation of:
  - `init-profile`
  - `validate-profile`
  - `list-profiles`
  - `get-work`
  - `heartbeat`
  - `submit-result`
  - `release-work`
  - `render-context`
- Fake graph returns:
  - task from `next`
  - no work
  - claim success
  - claim failure
  - summarize failure
  - stale revision
  - malformed JSON
  - nonzero exit
- Adapter commands work with graph state files missing, unreadable, or poisoned when fake graph responses are valid.

### Definition of Done

All PRD acceptance criteria are met, E2E tests prove CLI behavior, and `agent-adapter` is ready to be consumed by external frameworks such as Claude Code, Codex, OpenHands, or custom shell agents.

---

## 3. First Build Prompt

```text
Implement PR 1 only from the approved Module 2 Implementation Plan.

Scope:
- Create separate Rust binary `agent-adapter`.
- Add dependencies: clap, serde, serde_yaml, serde_json, thiserror.
- Define config/profile/capability/permission structs.
- Define success/failure JSON envelopes.
- Define adapter error model and standard codes.
- Implement `init-profile`, `validate-profile`, and `list-profiles`.
- Create `.agent/adapter.config.yaml` and `.agent/adapter_artifacts/`.
- Do not implement graph subprocess calls yet.
- Do not read or mutate `.agent/task_graph.yaml` or `.agent/task_events.jsonl`.
- Add tests listed under PR 1.

Report:
- Files created/modified.
- Commands run.
- Test results.
- Any deviations from the plan.
```
