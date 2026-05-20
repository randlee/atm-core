---
id: Y.22
title: Reconcile Runtime Cutover
status: planned
branch: feature/pYe-s22-reconcile-runtime-cutover
worktree: ../atm-core-worktrees/feature/pYe-s22-reconcile-runtime-cutover
target: integrate/phase-Y
---

# Sprint Y.22 — Reconcile Runtime Cutover

## Goal

- delete the production shared-state reconcile runtime path
- land the final actor-owned reconcile implementation, including
  fingerprint-registry ownership and bounded shutdown

## Motivation / Problem Statement

`Y.21` defines the reconcile actor contract. `Y.22` exists to delete the old
shared-state runtime path and land the actor-owned reconcile implementation on
the accepted line.

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

ADR-015 ownership in this sprint:

- update the `Decision` section so `ReconcileRuntime` explicitly owns the
  production worker state, including fingerprint-registry ownership and bounded
  shutdown on the final actor runtime
- update the `Implementation Plan` section so `Y.22` is the only sprint that
  deletes the shared-state reconcile runtime path

## Governing Requirements And ADRs

- `REQ-DAEMON-RUNTIME-009`
- `REQ-DAEMON-TEST-004`
- `ADR-015`

## Exact Targets

- `Cargo.toml`
- `crates/atm-daemon/Cargo.toml`
- `crates/atm-daemon/src/worker_support.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
  - update the `DaemonReconcileCoordinatorAdapter` record

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

```rust
#[derive(Debug, Clone, Default)]
pub(crate) struct ReconcileRuntimeStatus {
    started: bool,
    shutdown_requested: bool,
    degraded_message: Option<Arc<str>>,
}
```

There is no surviving production `Mutex<ReconcileState>` or `Condvar` queue
coordination after this sprint closes.

`JoinHandleOwner` continues to live in
`crates/atm-daemon/src/worker_support.rs`; `Y.22` reuses that shared helper
and does not reintroduce a lane-local duplicate.

### Ownership

- the worker owns pending reconcile state, fingerprint registry, debounce
  counters, and completion fanout
- callers own only request submission and one reply receiver
- lifecycle/readiness publication is explicit and separate from reconcile
  coordination state
- `JoinHandleOwner` remains the one allowed narrow mutex helper on this lane;
  its `Mutex<Option<JoinHandle<()>>>` owns only bounded worker join lifecycle
  and must not own fingerprint, debounce, queue, or completion state

### Data Flow

1. caller submits reconcile request over the bounded command channel
2. worker coalesces and debounces
3. worker executes watch/ingress/notification work
4. worker replies to all waiters and updates reconcile runtime status
5. shutdown sends one control command and proves bounded termination

## Deliverables

- production `Mutex<ReconcileState>` and `Condvar` coordination are removed
- pending/completed/waiter tracking is worker-owned actor state only
- the notification fingerprint registry is moved into worker-owned reconcile
  actor state; there is no surviving side mutex for fingerprint ownership
- reconcile coalescing, completion fanout, and bounded shutdown are proven on
  the final actor design
- daemon requirements, architecture, and boundary docs reflect the cutover
  implementation shape for reconcile ownership

## Required Work

- reuse the `arc_swap` dependency introduced in `Y.19`; if `Y.19` is not yet
  on the accepted line, add `arc_swap` to the workspace `Cargo.toml`
  dependency table and to `crates/atm-daemon/Cargo.toml` in this sprint
- replace the production reconcile runtime with the actor-owned runtime shape
  defined in `Y.21`
- move the notification fingerprint registry into worker-owned reconcile actor
  state and delete any surviving side ownership surface
- delete any surviving `Arc<Mutex<NotificationFingerprintRegistry>>` helper,
  field, or side-owned registry path so fingerprint ownership exists only
  inside `ReconcileWorkerState`
- delete production shared-state pending/completed/waiter coordination paths
- prove bounded shutdown on the final actor-owned runtime
- update daemon requirements, architecture, boundaries, and `ADR-015` so the
  accepted reconcile ownership model matches the final cutover implementation

## Paths To Delete

- `crates/atm-daemon/src/reconcile_runtime.rs`
  - delete production `Mutex<ReconcileState>` ownership
  - delete production `Condvar` reconcile coordination
  - delete production shared-state pending/completed/waiter tracking paths
  - delete any surviving `Arc<Mutex<NotificationFingerprintRegistry>>` helper,
    field, or detached side-owned fingerprint registry path

## Acceptance Criteria

- `reconcile_runtime_actor_cutover_removes_shared_state_runtime_path`
- `reconcile_runtime_actor_shutdown_stays_bounded`
- `reconcile_runtime_actor_notification_fingerprint_registry_is_worker_owned`
- all listed deliverables land at a production-ready level for the sprint
  scope; no hybrid production runtime survives after cutover
- the fingerprint registry, debounce state, and completion fanout are owned by
  one worker lane on the final implementation line
- the sprint closes reconcile cutover only and does not absorb phase-end proof
  or final ADR acceptance work

## Closure Invariants

- no production reconcile request path depends on `Mutex<ReconcileState>` or
  `Condvar`
- reconcile coalescing, completion fanout, and notification dedupe are owned
  by one worker lane
- this sprint closes the reconcile runtime cutover itself, not the whole phase

## Explicit Non-Closure

- no phase-end ADR acceptance or readiness closeout in this sprint
- no additional daemon-lane redesign outside reconcile cutover

## Scope Estimate

This sprint is credible only because `Y.21` isolates the actor contract first.
If the team attempts to reintroduce phase-end proof, ADR acceptance, or
cross-phase closeout work into this same sprint, it should split before
implementation.

## Required Validation

- `rg -n 'arc_swap' Cargo.toml crates/atm-daemon/Cargo.toml`
- `rg -n 'struct JoinHandleOwner' crates/atm-daemon/src/worker_support.rs`
- `rg -n 'struct JoinHandleOwner' crates/atm-daemon/src/reconcile_runtime.rs` # expected: zero matches
- `rg -n "Mutex<ReconcileState>|Condvar" crates/atm-daemon/src/reconcile_runtime.rs` # expected: zero matches
- `rg -n 'NotificationFingerprintRegistry' crates/atm-daemon/src/reconcile_runtime.rs`
- `rg -n 'notification_fingerprints: NotificationFingerprintRegistry' crates/atm-daemon/src/reconcile_runtime.rs`
- `rg -n 'Arc<Mutex<NotificationFingerprintRegistry>>' crates/atm-daemon/src/reconcile_runtime.rs` # expected: zero matches
- `cargo test --workspace reconcile_runtime_actor_cutover_removes_shared_state_runtime_path -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_shutdown_stays_bounded -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_notification_fingerprint_registry_is_worker_owned -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
