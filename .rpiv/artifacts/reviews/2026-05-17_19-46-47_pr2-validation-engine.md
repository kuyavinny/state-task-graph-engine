---
template_version: 2
date: 2026-05-17T19:46:47-0800
author: kuyavinny
repository: state-task-graph-engine
branch: feat/pr2-validation-engine
commit: 3e28d02
review_type: pr
scope: "feat/pr2-validation-engine vs develop"
scope_strategy: first-parent
in_scope_files_count: 8
status: needs_changes
severity: { critical: 0, important: 4, suggestion: 1 }
verification: { verified: 5, weakened: 0, falsified: 0 }
blockers_count: 0
tags: [code-review, validation-engine]
---

# Code Review — feat/pr2-validation-engine

**Commit:** `3e28d02` · **Status:** `needs_changes` · **Findings:** 0🔴 · 4🟡 · 1🔵 · **Verification:** 5✓ / 0− / 0✗

## Top Blockers

1. **I2** — `src/error.rs:87` — Top-level `InvalidSchema` masks all specific validation error codes
2. **I1** — `src/validate.rs:23-25` — Cycle detection suppressed when unknown dependencies present
3. **I3** — `src/validate.rs:262-265` — Lease guard misses orphaned `claimed_at`/`expires_at`
4. **I4** — `src/validate.rs:297-316` — Terminal-state reason fields not mutually exclusive

---

## Legend

```text
Severity    🔴 fix before merge   🟡 fix soon   🔵 nice to have   💭 discuss
ID prefix   I interaction   Q quality   S security   G gap
Verify      ✓ verified   − weakened (demoted)   ✗ falsified (dropped)
Annotate    [precedent-weighted]   [cascade: <kind>]   [subsumed-by <ID>]
```

---

## 🟡 Important

### I1 🟡 Cycle detection suppressed by referential errors

**Where**
`src/validate.rs:23-25` — `let has_ref_errors = errors.iter().any(|e| e.code == ErrorCode::UnknownDependency);`
`src/validate.rs:26` — `if !has_ref_errors { check_cycles(...) }`

**Code**
```rust
let has_ref_errors = errors
    .iter()
    .any(|e| e.code == ErrorCode::UnknownDependency);
if !has_ref_errors {
    check_cycles(graph, &id_set, &mut errors);
}
```

**Why**
The validation pipeline at `validate.rs:11` runs checks in order: duplicates → referential integrity → cycles → per-node fields. If `check_referential_integrity` (line 49) produces any `UnknownDependency` error, the guard at line 26 skips cycle detection entirely. A graph with both unknown deps AND cycles will only report the unknown deps on the first run. The user fixes those, re-runs, and hits an unexpected `CycleDetected` error — the first validation was incomplete. Consumer at `cli.rs:154` receives `AppError::GraphValidationFailed` with no metadata about which checks actually ran.

**Fix**
Remove the conditional gate — run `check_cycles` unconditionally. Kahn's algorithm is O(V+E) and handles missing nodes gracefully (any dependency not in the graph simply reduces the node's in-degree, so it contributes to `sorted_count < graph.nodes.len()` which triggers the cycle error correctly even when unknown deps are present).

**Alt**
Keep the gate but annotate the returned `ValidationError` list with a metadata field indicating which checks were skipped.

---

### I2 🟡 Top-level `InvalidSchema` masks specific validation error codes [subsumes Q3, Q6]

**Where**
`src/error.rs:87` — `AppError::GraphValidationFailed { .. } => ErrorCode::InvalidSchema,`
`src/validate.rs:49` — `code: ErrorCode::DuplicateNodeId`
`src/validate.rs:55` — `code: ErrorCode::UnknownDependency`
`src/validate.rs:61` — `code: ErrorCode::CycleDetected`
`tests/validate.rs:412` — `assert_eq!(envelope["error"]["code"], "INVALID_SCHEMA")`

**Code**
```rust
// error.rs:87 — single collapsed mapping
AppError::GraphValidationFailed { .. } => ErrorCode::InvalidSchema,

// validate.rs:49 — specific discriminators in sub-errors
code: ErrorCode::DuplicateNodeId
```

**Why**
The `AppError::GraphValidationFailed` error_code mapping returns `InvalidSchema` for ALL validation failures (cycles, duplicates, unknown deps, field errors). Meanwhile, each `ValidationError` in `errors[].code` retains its specific discriminator. The top-level envelope `error.code` says `"INVALID_SCHEMA"` whether the graph has a cycle, a duplicate ID, or a missing title — making it unreliable for programmatic routing. Consumers must dig into `error.details.errors[].code` to distinguish the actual failure type. This is a co-tenant filter gap.

This pattern mirrors the PR#1 finding (error code mapping drift) that required a fixup commit — same defect class surfaced again.

**Fix**
Map `GraphValidationFailed` to a dedicated `ErrorCode::ValidationFailed` instead of reusing `InvalidSchema`. Or, remove the collapsed mapping and let the consumer inspect `errors[].code` directly at the top level (e.g., when there's exactly one error, promote its code to the envelope level).

---

### I3 🟡 Lease guard only checks `claimed_by`, allows orphaned `claimed_at`/`expires_at` [subsumes Q4]

**Where**
`src/validate.rs:262-265` — `_ => { if node.lease.claimed_by.is_some() { ... } }`
`src/model.rs:86-90` — `pub claimed_by: Option<String>, pub claimed_at: Option<String>, pub expires_at: Option<String>`

**Code**
```rust
// validate.rs:262-265 — non-InProgress guard
_ => {
    if node.lease.claimed_by.is_some() {
        errors.push(/* lease.claimed_by should be None */);
    }
}
```

**Why**
The lease-consistency check's non-`InProgress` branch (`validate.rs:262`) only validates `claimed_by.is_some()`. It does not check `claimed_at` or `expires_at`. Since all three are independent `Option<String>` fields, a node with `status: Ready` and `lease.claimed_at: Some("2026-05-17T00:00:00Z")` but `lease.claimed_by: None` passes validation — orphaned timestamps without a claimant. The `InProgress` branch (line 251) correctly checks all three fields, making the asymmetry a clear gap.

**Fix**
Extend the `_` arm to also check `claimed_at.is_some()` and `expires_at.is_some()`, emitting a `ValidationError` for each residual field.

---

### I4 🟡 Terminal-state reason guards not mutually exclusive [subsumes Q5]

**Where**
`src/validate.rs:297-316` — terminal-state reason match arms
`src/model.rs:45-49` — `pub result_summary: Option<String>, pub failure_reason: Option<String>, ...`

**Code**
```rust
// validate.rs:299 — only checks the matching field is present
Status::Completed if node.result_summary.is_none() => {
    errors.push(missing_reason(node, "result_summary"));
}
Status::Failed if node.failure_reason.is_none() => {
    errors.push(missing_reason(node, "failure_reason"));
}
```

**Why**
Each terminal-status guard checks only that its matching reason field is present (e.g., `Completed` needs `result_summary`). None check that contradictory fields are absent. A `Completed` node can carry `failure_reason: Some("engine crashed")` alongside `result_summary: Some("done")` and pass validation. All five reason fields are independently optional, so any combination is accepted. This allows semantically impossible state pollution.

**Fix**
Add guard clauses in each arm that reject the presence of non-matching reason fields. For example, `Status::Completed` should reject `failure_reason`, `blocked_reason`, `skip_reason`, and `cancel_reason` being `Some`.

---

## 🔵 Suggestions

### Q2 🔵 Missing explicit lower-bound guard on `attempts == 0`

**Where**
`src/validate.rs:199` — `if node.attempts > node.max_attempts && node.status != Status::Failed {`

**Fix**
Add an explicit guard: `if node.attempts == 0 { errors.push(...) }`. Low risk — `u32` enforces non-negative and `max_attempts >= 1` catches the zero-attempt case indirectly.

---

## Precedents

| Commit    | Subject          | Follow-ups                                              |
| --------- | ---------------- | ------------------------------------------------------- |
| `c93a105` | PR#1 CLI skeleton + init | 1 fixup (`5a98583`) within 30 min — error enrichment retrofitted |
| `3e28d02` | PR#2 Validation Engine (this commit) | None yet (committed minutes ago) |

**Recurring lessons (most → least frequent)**

1. **Error enrichment must be exhaustive from the start.** PR#1's `details()` returned empty objects for 8 of 16 variants; fixed in a follow-up. I2 (top-level `InvalidSchema` collapsing specificity) is a recurrence of the same defect class.

2. **Error code mapping drift is the most common bug class.** PR#1 collapsed `Io`/`Serialization` under wrong codes (fixup). PR#2 collapses all validation discriminators under `InvalidSchema`. Each layer that adds a new error discriminator needs an explicit mapping review.

3. **Validation contracts without consumers are forward debt.** All 7 predicate coherence rows from PR#2's validation have zero consumers — the state-transition handlers are still stubs. These predicates are correct by design but untestable until PR#3+.

---

## Recommendation

| # | ID     | Action                      | Alt / Note        |
| - | ------ | --------------------------- | ----------------- |
| 1 | I2     | Map `GraphValidationFailed` to dedicated `ErrorCode::ValidationFailed`, or promote single-error code to envelope | Collapsing under `InvalidSchema` repeats PR#1 fixup pattern |
| 2 | I1     | Remove conditional gate on cycle detection; run `check_cycles` unconditionally | O(V+E), handles unknown deps gracefully |
| 3 | I3     | Extend non-InProgress lease guard to check `claimed_at` and `expires_at` | Match `InProgress` arm's completeness |
| 4 | I4     | Add mutual-exclusion guards for contradictory terminal-state reason fields | Each arm should reject non-matching reason fields |
| 5 | Q2     | Add explicit `attempts == 0` guard | Low priority, `u32`+`max_attempts >= 1` already covers it |
