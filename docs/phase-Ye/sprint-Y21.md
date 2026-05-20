---
id: Y.21
title: Reconcile Runtime Actor Foundation
status: planned
branch: feature/pYe-s21-reconcile-runtime-actor-foundation
worktree: ../atm-core-worktrees/feature/pYe-s21-reconcile-runtime-actor-foundation
target: integrate/phase-Y
---

# Sprint Y.21 — Reconcile Runtime Actor Foundation

## Goal

- freeze the reconcile actor command/reply contract
- move the authoritative reconcile ownership model to worker-owned actor state
  without claiming the final shared-state cutover in the same sprint

## Motivation / Problem Statement

`ReconcileRuntime` already behaves like an actor:

- callers submit reconcile requests
- one worker coalesces and debounces
- one worker executes ingress/watch/notification work
- callers receive one result

But the implementation still exposes this through
`Mutex<ReconcileState> + Condvar`, shared pending/completed maps, and shared
waiter tracking.

That is too much ownership change to close credibly in one cutover sprint
without first freezing the actor contract.

## Hard Dependencies

- `Y.20` should land first so both worker lanes use the same ownership pattern
- `docs/phase-Ye/plan-phase-Ye.md`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

ADR-015 ownership in this sprint:

- update the `Decision` section so `ReconcileRuntime` explicitly owns the
  command-in / reply-out actor contract and worker-owned debounce/coalescing
  semantics
- update the `Implementation Plan` section so `Y.21` closes the reconcile
  actor contract only, while `Y.22` closes the production cutover

## Governing Requirements And ADRs

- `REQ-DAEMON-RUNTIME-009`
- `REQ-DAEMON-TEST-004`
- `ADR-015`

## Exact Targets

- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

## Proposed Design

### Types

```rust
use std::thread::JoinHandle;

#[derive(Debug)]
pub(crate) struct JoinHandleOwner {
    join_handle: Mutex<Option<JoinHandle<()>>>,
}

impl JoinHandleOwner {
    pub(crate) fn join_with_deadline(
        &self,
        deadline: Duration,
    ) -> Result<(), AtmError>;
}
```

```rust
use std::sync::mpsc::{Receiver, SyncSender};

pub(crate) enum ReconcileCommand {
    Reconcile {
        request: ReconcileRequest,
        reply_tx: SyncSender<Result<ReconcileResult, AtmError>>,
    },
    Shutdown,
}

struct ReconcileWorkerState {
    pending_epoch: u64,
    pending: HashMap<ReconcileKey, PendingReconcile>,
    pending_order: VecDeque<ReconcileKey>,
    notification_fingerprints: NotificationFingerprintRegistry,
}
```

```rust
struct PendingReconcile {
    request: ReconcileRequest,
    replies: Vec<SyncSender<Result<ReconcileResult, AtmError>>>,
}
```

```rust
impl ReconcileRuntime {
    pub(crate) fn reconcile(
        &self,
        request: ReconcileRequest,
    ) -> Result<ReconcileResult, AtmError>;
}
```

### Ownership

- callers own request construction and one reply channel
- the worker owns debounce state, coalescing, pending order, fingerprint
  registry, and completion fanout
- there is no daemon-shared `completed` map or waiter registry after the full
  cutover

### Data Flow

1. caller sends `ReconcileCommand::Reconcile { request, reply_tx }`
2. worker coalesces by `ReconcileKey`
3. worker applies bounded debounce and executes reconcile work
4. worker fans one `ReconcileResult` or typed failure back to all waiting
   reply channels for that key

## Deliverables

- the reconcile actor command/reply model is defined and lands with explicit
  types
- debounce, coalescing, and reply fanout are modeled as worker-owned state
  rather than daemon-shared lock state
- daemon requirements and architecture docs explicitly state that reconcile is
  a worker-owned actor boundary
- `ADR-015` is updated to name reconcile as an actor/channel lane

## Required Work

- define explicit reconcile command and reply types for the final worker-owned
  actor contract
- model debounce, coalescing, pending order, and completion fanout as
  authoritative worker-owned state in the contract docs and implementation seam
- land reply-path expectations so callers own one request plus one reply
  receiver rather than shared waiter tracking
- update daemon requirements, architecture, boundaries, and `ADR-015` so the
  accepted reconcile ownership rule is actor/channel based
- leave the production shared-state delete work and final fingerprint-registry
  cutover to `Y.22` only

## Acceptance Criteria

- `reconcile_runtime_actor_coalesces_identical_requests_into_one_worker_run`
- `reconcile_runtime_actor_fans_one_result_to_all_waiters_for_a_key`
- `reconcile_runtime_actor_preserves_bounded_debounce_extensions`
- all listed deliverables land at a production-ready level for the sprint
  scope; the actor contract is no longer ambiguous or optional
- the reconcile lane has one authoritative command-in / reply-out model before
  cutover begins as the frozen target contract for `Y.22`, even though the
  production shared-state runtime path is not yet deleted in `Y.21`
- the sprint does not claim deletion of the production shared-state path or
  final fingerprint-registry ownership closure
- the sprint explicitly defines `JoinHandleOwner` and
  `ReconcileWorkerState`; neither remains an undefined placeholder in the
  accepted contract

## Closure Invariants

- the reconcile actor contract is frozen as command-in / reply-out for the
  final cutover target
- worker-owned state is the target ownership model for debounce and pending
  request coordination, but `Y.21` does not yet claim deletion of the
  production shared-state runtime path
- this sprint closes the actor contract, not yet the full shared-state
  implementation deletion

## Explicit Non-Closure

- no deletion of the production shared-state reconcile path yet
- no production fingerprint-registry cutover yet; the final worker-owned
  registry implementation closes in `Y.22`
- no phase-end closure proof
- no daemon transport or notification-boundary redesign

## Scope Estimate

This sprint intentionally closes only the actor foundation. The reconcile lane
has enough moving parts that forcing contract definition and full cutover into
one sprint is not credible.

If the contract cannot be frozen cleanly in this sprint, the sprint must split
again before implementation.

## Required Validation

- `rg -n 'struct ReconcileWorkerState' crates/atm-daemon/src/reconcile_runtime.rs`
- `rg -n 'struct JoinHandleOwner' crates/atm-daemon/src/reconcile_runtime.rs`
- `rg -n 'struct JoinHandleOwner' docs/phase-Ye/sprint-Y20.md docs/phase-Ye/sprint-Y21.md`
- `cargo test --workspace reconcile_runtime_actor_coalesces_identical_requests_into_one_worker_run -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_fans_one_result_to_all_waiters_for_a_key -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_preserves_bounded_debounce_extensions -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
