---
template_version: 2
date: 2026-05-17T19:46:47-0800
author: kuyavinny
repository: agent-graph
branch: feat/pr3-reconciliation-engine
commit: 15c46b6
review_type: pr
scope: "feat/pr3-reconciliation-engine vs develop"
scope_strategy: first-parent
in_scope_files_count: 5
status: needs_changes
severity: { critical: 1, important: 3, suggestion: 0 }
verification: { verified: 4, weakened: 0, falsified: 0 }
blockers_count: 1
tags: [code-review, reconciliation-engine]
---

# Code Review — feat/pr3-reconciliation-engine

**Commit:** `37209f5` → `15c46b6` (fix applied) · **Status:** `needs_changes` · **Findings:** 1🔴 · 3🟡 · 0🔵 · **Verification:** 4✓ / 0− / 0✗

## Top Findings

1. **C1 🔴** — Event revision off-by-one causes deterministic desync after every reconcile
2. **I1 🟡** — Known desync does not prevent reconciliation writes/appends
3. **I2 🟡** — DESYNC warning embedded in data, not response envelope
4. **I3 🟡** — Multi-event append can partially commit event log

---

## Legend

```text
Severity    🔴 fix before merge   🟡 fix soon   🔵 nice to have   💭 discuss
ID prefix   I interaction   Q quality   S security   G gap
Verify      ✓ verified   − weakened (demoted)   ✗ falsified (dropped)
```

---

## 🔴 Critical

### C1 🔴 Event revision off-by-one causes deterministic desync after reconciliation

**Where**
`src/reconcile.rs:182` — `graph.graph_revision + events.len() as u64 + 1` (fixed)
`src/reconcile.rs:216` — `graph.graph_revision + events.len() as u64 + 1` (fixed)

**Code** (pre-fix)
```rust
graph.graph_revision + events.len() as u64,  // <-- off by one
```

**Why**
Event creation used `graph.graph_revision + events.len()` as the `graph_revision_after` value, while graph revision was set to `graph.graph_revision + events.len()` after all events were emitted. For a single event: event gets `old_revision`, graph gets `old_revision + 1`. Next run's desync check sees graph revision lagging event log by 1 — produces false `EVENT_LOG_DESYNC` warning after a successful reconcile.

**Fix** (`15c46b6`)
Changed both `make_event` calls to use `graph.graph_revision + events.len() + 1`. First event gets `old_revision + 1`, graph gets `old_revision + N`. Max event revision now matches graph revision exactly.

**Verification** — Verified ✓. Quote matched at reconcile.rs `make_event` call sites; added `no_post_reconcile_desync` test confirming clean desync check after reconcile.

---

## 🟡 Important

### I1 🟡 Known desync doesn't gate persistence

**Where**
`src/reconcile.rs:71-85` — desync check moved before mutations (fixed)

**Why**
Original code checked desync AFTER reconciliation mutations but before persistence. A known-desynced log would have reconciliation mutations applied in memory (leasing changes, status promotions) even though the desync warning was emitted. Violates implementation plan requirement: "Do not blindly append more events to a known-desynced log unless explicitly safe."

**Fix** (`15c46b6`)
`check_revision_desync()` moved to immediately after validation. If desynced, returns graph + warning with no reconciliation or persistence. Added `desync_gates_reconciliation_mutations` test.

**Verification** — Verified ✓. Quote matched at reconcile.rs; desync-gate test passes.

### I2 🟡 DESYNC warning embedded in data, not response envelope

**Where**
`src/cli.rs:192-206` — warnings now use `ResponseEnvelope::ok_with_warnings` (fixed)

**Why**
Desync warning was nested inside `data["warnings"]` as a structured object. The `ResponseEnvelope` already supports envelope-level warnings (`warnings: Option<Vec<String>>`) with an existing `ok_with_warnings()` constructor. Nested warnings are invisible to JSON envelope consumers that route on envelope-level fields.

**Fix** (`15c46b6`)
Status handler now uses `ResponseEnvelope::ok_with_warnings()` with string warnings at the envelope level.

**Verification** — Verified ✓. Quote matched at cli.rs; updated integration test confirms envelope-level warnings.

### I3 🟡 Multi-event append can partially commit event log

**Where**
`src/reconcile.rs` — now uses `io::append_events_batch` (fixed)

**Why**
Original code looped individual `append_event()` calls, each opening and closing the file. If event 2 of 2 failed, the graph was already updated to reflect 2 events but the log only recorded 1. No rollback mechanism exists.

**Fix** (`15c46b6`)
Added `io::append_events_batch()` that serializes all events to strings, then opens the file once and writes all lines. This reduces the commit-window from N file operations to 1.

**Verification** — Verified ✓. Logic confirmed by reading reconcile.rs and io.rs; single-write operation reduces window but doesn't eliminate risk entirely (acknowledged as acceptable vs transactional complexity).

---

## Impact

| Consumer        | Change           | Findings |
| --------------- | ---------------- | -------- |
| `src/cli.rs:170` | status command wired to reconcile | I2 |
| `src/cli.rs:216` | next command wired to reconcile | — |
| `src/reconcile.rs:71` | Desync check before mutations | I1 |
| `src/reconcile.rs:180` | Event revision computation | C1 |
| `src/reconcile.rs:144` | Batch event append | I3 |

---

## Recommendation

All fixes applied in commit `15c46b6`. Ready for squash merge to `develop`.

| # | ID     | Action (applied)                     | Verification |
| - | ------ | ---------------------------------- | ------------ |
| 1 | C1     | `+ 1` in event revision computation | `no_post_reconcile_desync` test |
| 2 | I1     | Move desync check before mutations  | `desync_gates_reconciliation_mutations` test |
| 3 | I2     | Use `ok_with_warnings()` for envelope | Updated integration test |
| 4 | I3     | `io::append_events_batch()` helper  | Existing tests pass |
