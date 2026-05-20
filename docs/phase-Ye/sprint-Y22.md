---
id: Y.22
title: Reconcile Runtime Cutover And Proof
status: draft
branch: feature/pYe-s22-reconcile-runtime-cutover-and-proof
worktree: ../atm-core-worktrees/feature/pYe-s22-reconcile-runtime-cutover-and-proof
target: integrate/phase-Ye
---

# Sprint Y.22 — Reconcile Runtime Cutover And Proof

## Motivation / Problem Statement

`Y.21` defines the reconcile actor contract. `Y.22` exists so the phase closes
only after the old shared-state runtime path is deleted and the final actor
design is proven on the accepted line.

Without a dedicated cutover sprint, the reconcile lane is the most likely part
of `Phase Ye` to claim closure while still carrying the old lock-heavy
implementation in parallel.

## Hard Dependencies

- `Y.21` must close first
- `docs/phase-Ye/plan-phase-Ye.md`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

## Exact Targets

- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/phase-Ye/issues.md`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/project-plan.md`

## Proposed Design

### Types

`Y.22` closes the `Y.21` actor model by deleting the production shared-state
surface:

```rust
pub(crate) struct ReconcileRuntime {
    tx: SyncSender<ReconcileCommand>,
    status: Arc<ArcSwap<ReconcileRuntimeStatus>>,
    worker: Arc<JoinHandleOwner>,
    observability: SubsystemObservability,
}
```

There is no surviving production `Mutex<ReconcileState>` or `Condvar` queue
coordination after this sprint closes.

### Ownership

- the worker owns pending reconcile state, fingerprint registry, debounce
  counters, and completion fanout
- callers own only request submission and one reply receiver
- lifecycle/readiness publication is explicit and separate from reconcile
  coordination state

### Data Flow

1. caller submits reconcile request over the bounded command channel
2. worker coalesces and debounces
3. worker executes watch/ingress/notification work
4. worker replies to all waiters and updates runtime status publication
5. shutdown sends one control command and proves bounded termination

## Required Deliverables

- production `Mutex<ReconcileState>` and `Condvar` coordination are removed
- pending/completed/waiter tracking is worker-owned actor state only
- phase-end validation proves reconcile coalescing, completion fanout, and
  bounded shutdown on the final actor design
- `docs/phase-Ye/issues.md` is updated to mark all three lock-removal items
  closed
- `ADR-015` is accepted and reflected in daemon requirements and architecture
  docs

## Named Acceptance Tests

- `reconcile_runtime_actor_cutover_removes_shared_state_runtime_path`
- `reconcile_runtime_actor_shutdown_stays_bounded`
- `reconcile_runtime_actor_notification_fingerprint_registry_is_worker_owned`

## Closure Invariants

- no production reconcile request path depends on `Mutex<ReconcileState>` or
  `Condvar`
- reconcile coalescing, completion fanout, and notification dedupe are owned
  by one worker lane
- `Phase Ye` closes only when all three daemon ownership redesigns are proven
  on the accepted line

## Scope Estimate

This sprint is credible only because `Y.21` isolates the actor contract first.
If the team attempts to reintroduce additional daemon-runtime redesign in this
same sprint, it should split before implementation.

## Required Validation

- `rg -n "Mutex<ReconcileState>|Condvar" crates/atm-daemon/src/reconcile_runtime.rs`
- `cargo test --workspace reconcile_runtime_actor_cutover_removes_shared_state_runtime_path -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_shutdown_stays_bounded -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_notification_fingerprint_registry_is_worker_owned -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
