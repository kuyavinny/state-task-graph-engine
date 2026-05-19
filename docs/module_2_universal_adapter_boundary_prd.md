# Product Requirements Document: Module 2 - Universal Adapter Boundary

---

## 1. Purpose and Problem Statement

The State & Task Graph Engine (Module 1) provides a rigorous, deterministic local state machine for task execution. However, external agent runtimes such as Claude Code, Cursor, OpenHands, or custom shell scripts do not natively speak its language.

If agents integrate directly with the Graph Engine, they must independently manage strict JSON envelopes, optimistic concurrency, stale revisions, lazy lease heartbeats, bounded context handling, and graph-engine command translation. This creates brittle integrations, duplicated effort, and tight coupling.

The **Universal Adapter Boundary** establishes a formal protocol boundary rather than a loose middleware wrapper. It acts as a strict, stateless translation layer that normalizes interactions between diverse agent runtimes and the underlying Graph Engine. By enforcing a standard task lifecycle, standardized JSON-in/JSON-out contracts, and isolated adapter-owned files, it protects the core Graph Engine from runtime-specific assumptions while making it easier to onboard new coding assistants.

The adapter composes low-level Graph Engine commands into single agent-facing operations with explicit failure handling. It does not provide true transactions in v1.

---

## 2. System Actors

- **Human Operator:** Configures adapter profiles through `.agent/adapter.config.yaml`, monitors translation logs, and occasionally intervenes when agents face unresolvable errors.
- **State & Task Graph Engine:** The definitive source of truth for task state, managed purely through its own CLI binary.
- **Universal Adapter Boundary:** This module. A stateless CLI protocol layer that composes graph commands into agent-facing operations and normalizes data formats in transit.
- **External Coding Assistant / Agent Runtime:** The LLM-backed worker environment that interacts with the adapter through canonical JSON or optional templated Markdown.
- **Future Orchestrator / Planner:** An upstream actor that will eventually handle advanced capability-based routing. This is out of scope for v1.

---

## 3. Core Functional Requirements

### 3.1. Canonical v1 Lifecycle

1. **Read Adapter Profile** — Load capability boundaries, permissions, formatting rules, and artifact policy.
2. **Get Work** — Calls Graph Engine `next`, `claim`, and `summarize`; returns a Canonical Task Packet.
3. **Heartbeat** — Extends the lease for long-running tasks.
4. **Submit Result** — Validates the agent's result packet and translates agent status into Graph Engine commands.
5. **Release Work** — Yields a task without marking it terminal.

### 3.2. Explicit v1 Commands

- `adapter init-profile`
- `adapter validate-profile`
- `adapter list-profiles`
- `adapter get-work --profile <name>`
- `adapter heartbeat --task-id <id> --profile <name> --revision <int>`
- `adapter submit-result --profile <name> --result-file <path>`
- `adapter release-work --task-id <id> --profile <name> --revision <int> --reason <text>`
- `adapter render-context --task-id <id> --profile <name>`

Convenience flags may be supported for simple result submissions, but canonical complex input must use JSON file payloads.

### 3.3. Capability Declaration & Validation

Profiles must declare capabilities:

- `read_files`
- `write_files`
- `execute_shell`
- `run_tests`
- `network_access`
- `browser_access`
- `long_running_tasks`
- `max_task_minutes`
- `preferred_format`
- `max_context_chars`

Profiles must declare adapter permissions:

- `allow_claim`
- `allow_submit_success`
- `allow_submit_fail`
- `allow_submit_blocked`
- `allow_skip`
- `allow_cancel`
- `allow_release`

V1 does not perform advanced multi-agent routing. Profiles are used for safety checks, compatibility warnings, permissions, and formatting logic.

### 3.4. Stale Revision Handling

V1 must not auto-merge or auto-submit stale mutating results. If the Graph Engine rejects a payload due to `STALE_REVISION`, the adapter returns `CONTEXT_STALE_REFETCH_REQUIRED` and requires the agent to re-fetch current state.

### 3.5. Error Normalization

Graph Engine errors must be mapped to a standardized, machine-readable Adapter Error schema:

- `code`
- `source`
- `message`
- `retryable`
- `agent_action`
- `human_action`
- `details`

Human-readable or LLM-readable advice may be included, but it must not replace structured data.

---

## 4. Non-Functional Requirements

- **Stateless CLI Execution:** The adapter spins up, executes one command, and exits.
- **JSON-First Architecture:** Input and output are canonically JSON. Markdown is an optional rendering mode.
- **Zero Background Services:** No daemons, HTTP servers, MCP servers, or memory residency in v1.
- **Strict Process Isolation:** The adapter invokes the Graph Engine as an external process through argument arrays.
- **Local-First Operation:** The adapter operates entirely inside the local project environment.
- **Deterministic Output:** Given identical Graph Engine responses and adapter config, output should be stable and machine-parseable.
- **Human Inspectability:** Adapter configuration, logs, and artifacts must remain inspectable through local files.

---

## 5. Interfaces and Contracts

### 5.1. File Ownership & Boundaries

Adapter-owned files:

- `.agent/adapter.config.yaml`
- `.agent/adapter_logs.jsonl`
- `.agent/adapter_artifacts/`

Strict boundary:

- The adapter must not read, parse, or mutate `.agent/task_graph.yaml`.
- The adapter must not read, parse, or mutate `.agent/task_events.jsonl`.
- The adapter must not perform graph/event desync checks itself.
- All task-state interactions must occur through the Graph Engine CLI.

The adapter holds no authoritative task state. It may store configuration, logs, rendered prompts, raw agent output, adapter-owned artifacts, and translation diagnostics. Task state remains exclusively in the State & Task Graph Engine.

### 5.2. Canonical Task Packet Contract

Returned by `get-work`. Canonically JSON.

Required contents:

- adapter version
- profile
- actor
- graph revision
- task ID
- task title
- task description
- task status
- lease expiration
- bounded context inherited from Graph Engine `summarize`
- worker instructions
- reporting requirements
- heartbeat requirements
- profile-derived constraints

Markdown is rendered from the canonical JSON, never the reverse.

### 5.3. Canonical Result Packet Contract

Submitted through `submit-result`.

Required contents:

- adapter version
- profile
- actor
- task ID
- graph revision
- status
- summary
- reason
- artifacts
- evidence
- raw agent output path, if any

Status mapping:

- `success` -> Graph Engine `complete`
- `fail` -> Graph Engine `fail`
- `blocked` -> Graph Engine `block`
- `skipped` -> Graph Engine `skip`
- `cancelled` -> Graph Engine `cancel`

Required fields by status:

- `success` requires `summary`
- `fail` requires `reason`
- `blocked` requires `reason`
- `skipped` requires `reason`
- `cancelled` requires `reason`

### 5.4. Artifact Handling

- Project/source artifacts may be referenced in place after path validation.
- Raw agent outputs, temporary logs, rendered prompts, and adapter-generated diagnostics may be copied into `.agent/adapter_artifacts/`.
- The adapter must not move, rewrite, or normalize project files into adapter storage unless the result packet explicitly marks them as adapter-owned diagnostic artifacts.
- The adapter must enforce path safety and artifact size limits.

---

## 6. Relationship to State & Task Graph Engine

- **Consumes:** The adapter consumes the strict CLI of the Graph Engine.
- **Provides:** External agents consume the Adapter CLI.
- **No Duplication:** The adapter does not calculate dependencies, validate graph cycles, mutate graph files, or enforce graph state transitions directly.
- **No Authoritative State:** The adapter only stores non-authoritative integration artifacts.
- **Trust Boundary:** If the Graph Engine reports a state or warning, the adapter surfaces or normalizes it rather than independently inspecting graph files.

---

## 7. Out of Scope / Anti-Goals

- Not a planner.
- Not a verifier.
- Not a task executor.
- Not a Graph Engine replacement.
- Not a cloud service.
- Not a daemon.
- Not an HTTP/MCP server.
- Not a full multi-agent scheduler.
- Not an advanced capability router in v1.

---

## 8. Resolved Technical Decisions

- **V1 Transport:** Stateless CLI operations with JSON-in/JSON-out.
- **Profile Storage:** `.agent/adapter.config.yaml` with explicit CLI overrides where appropriate.
- **Prompt Formatting:** Templated Markdown rendering from the canonical JSON Task Packet.
- **Logs:** Separate `.agent/adapter_logs.jsonl`.
- **Artifacts:** Adapter-owned diagnostics under `.agent/adapter_artifacts/`; project artifacts referenced in place.
- **Stale Revisions:** Return `CONTEXT_STALE_REFETCH_REQUIRED`; no auto-merge.
- **Binary Boundary:** Separate adapter binary from graph engine binary.

---

## 9. Acceptance Criteria

1. `adapter get-work` chains Graph Engine `next`, `claim`, and `summarize`, then returns a canonical JSON Task Packet.
2. `adapter submit-result` maps all five adapter statuses to the correct Graph Engine commands.
3. Malformed result packets are rejected before any Graph Engine mutation call.
4. `STALE_REVISION` from the Graph Engine is returned as `CONTEXT_STALE_REFETCH_REQUIRED`.
5. Adapter translation events and boundary violations are logged to `.agent/adapter_logs.jsonl`.
6. The adapter never directly opens, reads, parses, or writes `.agent/task_graph.yaml` or `.agent/task_events.jsonl`.
7. The adapter supports at least two distinct capability profiles without binary changes.
8. Markdown rendering preserves core task information while enforcing context limits.
9. Artifact policies reject unsafe paths and oversize copied artifacts.
10. The adapter can be used by external coding assistants through stable JSON contracts.
