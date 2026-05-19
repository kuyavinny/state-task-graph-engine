# Product Requirements Document: State & Task Graph Engine

---

## 1. Purpose and Problem Statement

LLM-based agents natively operate as stateless text predictors. When tackling multi-step engineering tasks, they frequently fall into infinite loops, forget their original objective, or silently drop critical sub-tasks when context windows fill up.

The **State & Task Graph Engine** acts as the definitive source of truth for execution state. It does not generate or decompose tasks itself. Instead, it accepts, validates, persists, mutates, queries, and resumes task graphs produced by external planners, orchestrators, coding assistants, or human operators. It enforces deterministic workflow discipline, prevents simultaneous execution collisions, and bounds context payloads to keep agent prompts lean.

---

## 2. System Actors

- **The Planner / Orchestrator:** Generates the initial task graph or appends new nodes, submitting them to the engine for validation and tracking.
- **The Worker Agent:** Queries the engine for the next available task, claims it, attempts execution, and reports the resulting state transition back to the engine.
- **The Human Operator:** Can inspect the graph, manually alter state, prune branches, or resolve deadlocks directly via file edits or CLI commands.

---

## 3. Core Functional Requirements

### 3.1. Graph Management & Validation

- **Data Model:** The engine must utilize a Directed Acyclic Graph (DAG) for its internal execution model.
  - Hierarchical grouping is supported via `parent_id`.
  - Execution flow and dependencies are represented by a `dependencies` array containing node IDs.

- **Strict Schema Validation:** The engine must aggressively reject malformed inputs. Invalid operations include:
  - Duplicate node IDs.
  - Unresolved or orphaned dependencies.
  - Cycle detection / circular dependencies.
  - Missing required fields, such as `description` or `status`.
  - Malformed timestamps.
  - Invalid state transitions, such as completing a task whose dependencies are not met.

### 3.2. State & Transition Model

Every node must maintain a strictly enforced status. The engine manages the allowable transitions between:

- `PENDING`: Task exists but dependencies are unmet.
- `READY`: All dependencies are `COMPLETED` or `SKIPPED`; available for claiming.
- `IN_PROGRESS`: Currently claimed and actively being worked on.
- `BLOCKED`: Execution halted due to external factors or verifiable failures.
- `COMPLETED`: Successfully executed and externally reported as complete.
- `FAILED`: Execution exhausted attempts without success.
- `CANCELLED`: Aborted by user or orchestrator; prevents downstream execution.
- `SKIPPED`: Bypassed intentionally; satisfies downstream dependency checks.

### 3.3. Task Claiming & Lease Recovery

To prevent abandoned `IN_PROGRESS` tasks from becoming permanent dead states, such as when a worker crashes:

- Nodes must support claiming metadata:
  - `claimed_by`
  - `claimed_at`
  - `lease_expires_at`

- Nodes must track failure loops via:
  - `attempts`
  - `max_attempts`

- If a lease expires without a heartbeat or status update, the engine must release the task back to `READY`, or mark it `FAILED` if `max_attempts` is reached.

### 3.4. Optimistic Concurrency

- The graph must maintain a `graph_revision` integer that increments on every write.
- Agents and actors must supply the revision ID they read when attempting a write.
- If the graph has changed between read and write, the engine must validate the change.
- V1 should strictly reject stale writes rather than automatically merge them.
- Automatic non-conflicting merge support is a future enhancement.

### 3.5. Durable Persistence & Event Logging

- **State Store:** The current graph state must be saved to disk, such as `.agent/task_graph.yaml`, upon every successful mutation.
- **Event Log:** The engine must maintain an append-only `.agent/task_events.jsonl` file. This log preserves the immutable history of every state transition, claim, and error for observability and rollback capability.

### 3.6. Context-Bloat Control: Bounded Views

The engine must never pass the entire graph to an LLM by default. It must provide targeted payload views:

- **Current Task Packet:** The active node, its `parent_id` objective, and immediate dependencies.
- **Surroundings:** Nearby `BLOCKED` or `FAILED` nodes that might impact current execution.
- **History:** The `n` most recent events from the event log.
- **Summaries:** Highly compressed representations of `COMPLETED` branches to maintain situational awareness without token exhaustion.

### 3.7. Query & Action Surface

The engine must expose the following core API / CLI boundaries:

- `init`: Initialize a new graph and event log.
- `validate`: Run schema and cycle checks against a proposed graph.
- `status`: Return the high-level progress of the graph.
- `next`: Return the highest-priority `READY` task.
- `claim`: Lock a task with a lease and worker ID.
- `complete`: Mark an active task as externally completed.
- `fail`: Mark an active task as failed.
- `block`: Mark an active task as blocked.
- `skip`: Intentionally bypass a task.
- `cancel`: Cancel a task.
- `append-node` / `append-nodes`: Add new tasks dynamically.
- `reopen`: Reset a terminal state back to `PENDING` or `READY`.
- `summarize`: Generate the bounded context payload.

---

## 4. Non-Functional Requirements & Constraints

- **Zero-Dependency Local Execution:** Must function entirely locally without reliance on external cloud databases.
- **File-First Integrity:** The flat-file representation must remain cleanly formatted so human operators can resolve concurrency rejections manually via text editor.
- **Fast I/O:** State updates and validation checks must execute with CLI-native latency.
- **Runtime-Agnostic Consumption:** The module must be consumable by different coding assistants, orchestrators, and worker agents through stable file and CLI contracts.

---

## 5. Out of Scope / Anti-Goals

- **Automated Decomposition:** The engine will not prompt an LLM to break down a high-level objective. It only ingests the resulting DAG.
- **Task Execution:** The engine does not execute shell commands or run code.
- **Output Verification:** Validating the quality of a completed task belongs to the Verifier & Evaluation Gate.
- **Cloud State Backend:** V1 does not use external databases, vector stores, cloud services, or background daemons.
