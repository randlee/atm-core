---
id: Y.21
title: Reconcile Runtime Actor Foundation
status: draft
branch: feature/pYe-s21-reconcile-runtime-actor-foundation
worktree: ../atm-core-worktrees/feature/pYe-s21-reconcile-runtime-actor-foundation
target: integrate/phase-Ye
---

# Sprint Y.21 — Reconcile Runtime Actor Foundation

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

## Required Deliverables

- the reconcile actor command/reply model is defined and lands with explicit
  types
- debounce, coalescing, and reply fanout are modeled as worker-owned state
  rather than daemon-shared lock state
- the fingerprint registry is planned as worker-owned actor state
- daemon requirements and architecture docs explicitly state that reconcile is
  a worker-owned actor boundary
- `ADR-015` is updated to name reconcile as an actor/channel lane

## Named Acceptance Tests

- `reconcile_runtime_actor_coalesces_identical_requests_into_one_worker_run`
- `reconcile_runtime_actor_fans_one_result_to_all_waiters_for_a_key`
- `reconcile_runtime_actor_preserves_bounded_debounce_extensions`

## Closure Invariants

- the authoritative reconcile contract is command-in / reply-out
- worker-owned state, not daemon-shared lock state, owns debounce and pending
  request coordination
- this sprint closes the actor contract, not yet the full shared-state
  implementation deletion

## Scope Estimate

This sprint intentionally closes only the actor foundation. The reconcile lane
has enough moving parts that forcing contract definition and full cutover into
one sprint is not credible.

If the contract cannot be frozen cleanly in this sprint, the sprint must split
again before implementation.

## Required Validation

- `cargo test --workspace reconcile_runtime_actor_coalesces_identical_requests_into_one_worker_run -- --nocapture`
- `cargo test --workspace reconcile_runtime_actor_fans_one_result_to_all_waiters_for_a_key -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
