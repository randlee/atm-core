---
id: Y.20
title: Notification Runtime Channel Ownership
status: planned
branch: feature/pYe-s20-notification-runtime-channel-ownership
worktree: ../atm-core-worktrees/feature/pYe-s20-notification-runtime-channel-ownership
target: integrate/phase-Y
---

# Sprint Y.20 — Notification Runtime Channel Ownership

## Goal

- replace `NotificationRuntime` shared queue/lifecycle locking with bounded
  channel handoff plus worker-owned drain and persistence state
- keep explicit backpressure and bounded shutdown on the production lane

## Motivation / Problem Statement

`NotificationRuntime` is one background worker lane, but it still exposes its
queue, lifecycle, and degradation state through
`Mutex<NotificationState> + Condvar + VecDeque`.

That design blurs the real ownership model:

- producers should submit work
- the worker should own drain sequencing, persistence state, and degraded-state
  transitions
- callers should not coordinate through one shared mutable queue lock

## Hard Dependencies

- `Y.19` should land first so the phase uses one ownership direction
- `docs/phase-Ye/plan-phase-Ye.md`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

ADR-015 ownership in this sprint:

- update the `Decision` section so `NotificationRuntime` explicitly uses
  bounded command-channel handoff with worker-owned drain/persistence state
- update the `Implementation Plan` section so `Y.20` is the only sprint that
  closes the notification-runtime ownership cutover

## Governing Requirements And ADRs

- `REQ-DAEMON-RUNTIME-004`
- `REQ-DAEMON-RUNTIME-009`
- `REQ-DAEMON-TEST-004`
- `ADR-015`

## Exact Targets

- `Cargo.toml`
- `crates/atm-daemon/Cargo.toml`
- `crates/atm-daemon/src/worker_support.rs`
- `crates/atm-daemon/src/notification_runtime.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
  - update the `DaemonNotificationSinkAdapter` record

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

`JoinHandleOwner` is defined once in
`crates/atm-daemon/src/worker_support.rs` in `Y.20`. `Y.21` and `Y.22`
reuse that helper rather than redefining it in lane-local files.

```rust
use std::sync::mpsc::{Receiver, SyncSender};

pub(crate) enum NotificationCommand {
    Deliver { event: NotificationEvent },
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

```rust
impl NotificationRuntime {
    pub(crate) fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        self.tx
            .try_send(NotificationCommand::Deliver { event })
            .map_err(map_notification_backpressure)
    }
}
```

### Ownership

- producer paths own only bounded `try_send(...)` into the runtime command
  channel
- the worker owns the queue, persistence writes, and drain sequencing
- degraded status is published as immutable status, not inferred by peeking
  into a shared mutable queue/lifecycle lock
- `JoinHandleOwner` is the one allowed narrow mutex helper on this lane; its
  `Mutex<Option<JoinHandle<()>>>` owns only bounded worker join lifecycle and
  must not become a side queue, lifecycle, or degraded-state control plane

### Data Flow

1. runtime starts the worker and retains the bounded sender
2. `deliver(...)` validates lifecycle/degraded status, then `try_send(...)`
   one `NotificationCommand::Deliver`
3. worker receives commands, persists events, and publishes degraded state if
   persistence fails
4. shutdown sends one bounded control command and joins the worker within the
   bounded deadline

## Deliverables

- `NotificationRuntime` no longer uses `Mutex<NotificationState>` or `Condvar`
  for queue ownership
- the bounded command channel replaces the shared mutable `VecDeque` queue and
  preserves explicit backpressure
- degraded notification state is published explicitly and observed by callers
  without queue locking
- shutdown remains bounded and documented on the worker-owned lane
- the production bounded-cap contract remains explicit; if the current `64`
  event capacity changes, the sprint must update daemon boundary docs in the
  same change
- daemon requirements and architecture docs explicitly state that notification
  runtime ownership is channel/worker-based
- `ADR-015` names the bounded notification command channel as the accepted
  design

## Required Work

- reuse the `arc_swap` dependency introduced in `Y.19`; if `Y.19` is not yet
  on the accepted line, add `arc_swap` to the workspace `Cargo.toml`
  dependency table and to `crates/atm-daemon/Cargo.toml` in this sprint
- add `crates/atm-daemon/src/worker_support.rs` and define `JoinHandleOwner`
  there as the one shared worker-join helper for notification and reconcile
  lanes
- if `REQ-DAEMON-RUNTIME-009` and the `ADR-015` worker-lane rule are not yet
  present on the accepted implementation line when this sprint begins, this
  sprint must land them on that line as part of closure
- replace the production `NotificationState` queue/lifecycle coordination
  surface with a bounded command channel owned by the notification worker lane
- keep producer behavior limited to lifecycle checks plus `try_send(...)`
  submission of explicit `NotificationCommand` values
- move queue draining, persistence sequencing, degraded-state transitions, and
  backpressure ownership fully into the worker lane
- preserve and document the production bounded-cap contract; if the channel
  capacity changes, update daemon boundary docs in the same sprint
- make shutdown use one bounded control path that proves the worker can still
  terminate under backpressure
- update daemon requirements, architecture, boundaries, and `ADR-015` so the
  accepted notification ownership rule is worker-owned channel coordination

## Paths To Delete

- `crates/atm-daemon/src/notification_runtime.rs`
  - delete production `Mutex<NotificationState>` ownership
  - delete production `Condvar` queue/lifecycle coordination
  - delete production `VecDeque` queue ownership on the caller-visible path

## Acceptance Criteria

- `notification_runtime_deliver_uses_bounded_command_channel`
- `notification_runtime_persistence_failure_publishes_degraded_status`
- `notification_runtime_shutdown_stays_bounded_after_worker_backpressure`
- all listed deliverables land at a production-ready level for the sprint
  scope; no producer path still mutates queue state directly
- the final production lane has one authoritative backpressure seam through the
  bounded command channel
- `JoinHandleOwner` is explicitly defined as the worker-join ownership helper,
  not left as an undefined placeholder type
- daemon docs and `ADR-015` describe notification runtime ownership as
  channel-in / worker-owned drain and persistence state

## Closure Invariants

- producer paths never mutate the notification queue directly
- bounded backpressure is enforced at the command-channel seam rather than
  through a caller-visible mutable queue lock
- the production notification path preserves bounded backpressure and bounded
  shutdown behavior

## Explicit Non-Closure

- no notification event schema redesign
- no plugin-boundary redesign
- no reconcile actor work in this sprint

## Scope Estimate

This sprint is credibly closable in one sprint because notification delivery is
already one worker lane with one queue and no multi-key completion-routing
protocol.

If the implementation needs to redesign unrelated notification-event semantics
or plugin contracts, the sprint must split before implementation.

## Required Validation

- `rg -n 'arc_swap' Cargo.toml crates/atm-daemon/Cargo.toml`
- `rg -n 'struct JoinHandleOwner' crates/atm-daemon/src/worker_support.rs`
- `rg -n 'worker_support.rs|struct JoinHandleOwner' docs/phase-Ye/sprint-Y20.md docs/phase-Ye/sprint-Y21.md docs/phase-Ye/sprint-Y22.md`
- `rg -n "Mutex<NotificationState>|Condvar|VecDeque" crates/atm-daemon/src/notification_runtime.rs` # expected: zero matches
- `cargo test --workspace notification_runtime_deliver_uses_bounded_command_channel -- --nocapture`
- `cargo test --workspace notification_runtime_persistence_failure_publishes_degraded_status -- --nocapture`
- `cargo test --workspace notification_runtime_shutdown_stays_bounded_after_worker_backpressure -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
