# Backlog — PR#6 Review Suggestions

## 🔵 Q1 — Double node lookup in claim()
**Where**: `src/reconcile.rs:433-437` and `src/reconcile.rs:466`
**Issue**: Two lookups for the same node: first immutable (for dep check), then mutable (for mutation).
**Fix**: Use `node_idx` to get mutable ref directly (`&mut graph.nodes[node_idx]`) instead of calling `find_node_mut` again.
**Priority**: Low

---

## 🔵 Q2 — Unnecessary deps.clone() in claim dependency check
**Where**: `src/reconcile.rs:442`
**Issue**: Clone forced by borrow checker to avoid overlapping borrows.
**Fix**: Extract unresolved dep IDs using index-based access without cloning full deps list.
**Priority**: Low

---

## 🔵 Q5 — I3 creates check-ordering divergence vs sibling functions
**Where**: `src/reconcile.rs:633` vs `src/reconcile.rs:522`, `src/reconcile.rs:569`
**Issue**: `apply_simple_transition` (lease-first) vs `heartbeat`/`release` (status-first).
**Fix**: Apply I3 fix uniformly to all functions, or document the intentional divergence.
**Priority**: Low