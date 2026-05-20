---
id: Y.20
title: Notification Runtime Channel Ownership
status: draft
branch: feature/pYe-s20-notification-runtime-channel-ownership
worktree: ../atm-core-worktrees/feature/pYe-s20-notification-runtime-channel-ownership
target: integrate/phase-Y
---

# Sprint Y.20 — Notification Runtime Channel Ownership

## Motivation / Problem Statement

`NotificationRuntime` is one background worker lane, but it still exposes its
queue, lifecycle, and degradation state through
`Mutex<NotificationState> + Condvar + VecDeque`.

That design blurs the real ownership model:

- producers should submit work
- the worker should own the queue and persistence state
- callers should not coordinate through one shared mutable queue lock

## Hard Dependencies

- `Y.19` should land first so the phase uses one ownership direction
- `docs/phase-Ye/plan-phase-Ye.md`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

## Exact Targets

- `crates/atm-daemon/src/notification_runtime.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

## Proposed Design

### Types

```rust
use std::sync::mpsc::{Receiver, SyncSender};

pub(crate) enum NotificationCommand {
    Deliver(NotificationEvent),
    Shutdown,
}

#[derive(Debug, Clone)]
pub(crate) struct NotificationRuntimeStatus {
    started: bool,
    degraded_message: Option<Arc<str>>,
}

#[derive(Clone)]
pub(crate) struct NotificationRuntime {
    tx: SyncSender<NotificationCommand>,
    status: Arc<ArcSwap<NotificationRuntimeStatus>>,
    worker: Arc<JoinHandleOwner>,
    observability: SubsystemObservability,
}
```

### Ownership

- producer paths own only bounded `try_send(...)` into the runtime command
  channel
- the worker owns the queue, persistence writes, and drain sequencing
- degraded status is published as immutable status, not inferred by peeking
  into a shared mutable queue/lifecycle lock

### Data Flow

1. runtime starts the worker and retains the bounded sender
2. `deliver(...)` validates lifecycle/degraded status, then `try_send(...)`
   one `NotificationCommand::Deliver`
3. worker receives commands, persists events, and publishes degraded state if
   persistence fails
4. shutdown sends one bounded control command and joins the worker within the
   bounded deadline

## Required Deliverables

- `NotificationRuntime` no longer uses `Mutex<NotificationState>` or `Condvar`
  for queue ownership
- the queue is owned by the bounded command channel rather than a shared
  mutable `VecDeque`
- degraded notification state is published explicitly and observed by callers
  without queue locking
- shutdown remains bounded and documented on the worker-owned lane
- daemon requirements and architecture docs explicitly state that notification
  runtime ownership is channel/worker-based
- `ADR-015` names the bounded notification command channel as the accepted
  design

## Named Acceptance Tests

- `notification_runtime_deliver_uses_bounded_command_channel`
- `notification_runtime_persistence_failure_publishes_degraded_status`
- `notification_runtime_shutdown_stays_bounded_after_worker_backpressure`

## Closure Invariants

- producer paths never mutate the notification queue directly
- queue ownership belongs to the worker lane, not to a daemon-shared lock
- the production notification path preserves bounded backpressure and bounded
  shutdown behavior

## Scope Estimate

This sprint is credibly closable in one sprint because notification delivery is
already one worker lane with one queue and no multi-key completion-routing
protocol.

If the implementation needs to redesign unrelated notification-event semantics
or plugin contracts, the sprint must split before implementation.

## Required Validation

- `rg -n "Mutex<NotificationState>|Condvar|VecDeque" crates/atm-daemon/src/notification_runtime.rs`
- `cargo test --workspace notification_runtime_deliver_uses_bounded_command_channel -- --nocapture`
- `cargo test --workspace notification_runtime_persistence_failure_publishes_degraded_status -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
