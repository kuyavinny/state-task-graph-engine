# PR#8 Code Review: CLI Docs, Examples, Fixtures, & Full Integration Test Matrix

**Date:** 2026-05-18  
**Reviewer:** Claude  
**Branch:** `feat/pr8-docs-integration`  
**Base:** `develop`  
**Commit:** `820bd9b`  
**Fix Commit:** `post-review fix`

---

## 🔴 Critical

None

---

## 🟡 Important

### I1 — Fixture inconsistency: graph_revision/event_revisions mismatch

**Severity:** 🟡 → Fixed  
**Files:** `fixtures/sample_graph.yaml` (original), `fixtures/sample_events.jsonl` (original)  

**Finding:**  
- `fixtures/sample_graph.yaml` declared `graph_revision: 3`  
- `fixtures/sample_events.jsonl` final event had `graph_revision_after: 4`  
- `fixtures/sample_graph.yaml` showed `setup-api` as `PENDING` when its dependency `root` was `COMPLETED` — reconciliation would have promoted it to `READY`  
- `integrate` showed `status: READY` when dependencies `setup-db` and `setup-api` were not `COMPLETED` — should be `PENDING`

**Fix:**  
- Updated `graph_revision` from `3` → `4` to match event log  
- Fixed `setup-api` from `PENDING` → `READY` (dependency `root` = `COMPLETED`)  
- Fixed `integrate` from `READY` → `PENDING` (dependencies not COMPLETED)  
- Fixed event #4 action from `claim` (w/ no status change) → `reconcile` with `from_status: PENDING`, `to_status: READY`

---

## 🔵 Suggestions

### S1 — README error codes at risk of drift

**Files:** `README.md`  

README documents specific error codes (`TASK_NOT_FOUND`, `STALE_REVISION`, etc.) inline. If `ErrorCode` enum changes in a later PR, README becomes stale without CI checks. Historical pattern: PR#7 fixed `EventLogParseError`'s mapping after a code review found the mismatch.

**Recommendation:** Future PR touching `ErrorCode` should add a CI check or at minimum a manual README update as review action.

---

## Verification

- Pre-adjudication findings: 1 (I1)
- Post-fix: I1 resolved, 0 open findings
- Tests: 120 passing (78 unit + 10 init + 23 reconcile + 9 validate)

## Status: ✅ Approved

Fix applied directly on `develop` as follow-up. No runtime code changes.
