---
id: AD.5
title: Notification Runtime Removal And Post-Send NotificationSink Detachment
status: planned
branch: feature/pAD-s5-notification-runtime-removal-and-post-send-detachment
worktree: ../atm-core-worktrees/feature/pAD-s5-notification-runtime-removal-and-post-send-detachment
target: integrate/phase-AD
---

# Sprint AD.5 — Notification Runtime Removal And Post-Send `NotificationSink` Detachment

## Goal

- remove daemon notification queue/worker delivery and the generic
  `NotificationSink` abstraction from the post-send line

## Hard Dependencies

- `AD.2` complete
- `AD.4` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-core/src/boundary/mod.rs`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/service_runtime.rs`
- `boundaries/atm-core/notification-sink.toml`
- `boundaries/atm-daemon/daemon-notification-sink.toml`
- `boundaries/atm-daemon/daemon-non-claude-outbound.toml`
- `docs/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`

## Paths To Delete

- `crates/atm-daemon/src/notification_runtime.rs`
- `crates/atm-daemon/src/notification_runtime_tests.rs`
- daemon-owned `NotificationSink` implementations that exist only for runtime
  queue/worker delivery

## Interfaces To Add Or Modify

```rust
fn append_notification_log(event: &PostSendHookEvent) -> Result<(), AtmError>;
```

```rust
persist_message(...)?;
if recipient_has_post_send_hook {
    if let Err(error) = post_send_hook_emitter.emit(&event) {
        log_post_send_failure(&error);
        warnings.push(render_post_send_warning(&error));
    }
}
append_notification_log(&event)?;
```

- modify the post-send path so it no longer depends on `NotificationSink`
- if retained notification logging still exists, define one direct append helper
  at the event site rather than one queue/worker subsystem
- modify daemon composition and boundary wiring so no accepted startup path
  constructs notification-runtime worker state
- modify notification-related boundary TOMLs so deleted worker/runtime
  components are no longer declared active composition roots and non-Claude
  outbound contracts no longer imply notification-runtime fallback

## Obsolescence Instructions

- `NotificationSink`, `DaemonNotificationSink`, `LocalFileNotificationSink`,
  and any queue-owned notification helper become obsolete for post-send
  behavior in this sprint
- if a retained `NotificationSink` surface cannot be deleted immediately, mark
  it `Phase AD obsolete: non-post-send residual only`, keep it off the accepted
  post-send path, and forbid new production call sites

## Deliverables

- post-send no longer routes through `NotificationSink`
- the daemon no longer ships or starts a notification worker just to append
  notification events
- if notification logging remains, it is a direct append at the event site
- any retained `NotificationSink` surface is explicitly documented as
  non-post-send residual scope outside this sprint

## Required Work

- delete the queue/worker notification runtime and its boundary adapters
- remove `NotificationSink` from the post-send execution path
- if retained event logging still has operational value, replace subsystem
  delivery with a direct append helper on the event path
- keep sender warning ownership direct at the post-send call site

## This Sprint Does Not Close

- local tmux emitter implementation
- graft emitter implementation
- Claude inbox nudge deletion

## Acceptance Criteria

- no accepted daemon composition path starts or references the notification
  runtime worker
- post-send warning/logging behavior does not depend on `NotificationSink`
- any retained notification log append is synchronous and directly testable
- any retained `NotificationSink` surface is explicitly documented as
  non-post-send residual scope and is absent from the accepted post-send path
- no notification-related boundary TOML still declares a deleted notification
  worker composition root or a `NotificationSink` fallback on the accepted
  post-send path
- `docs/architecture.md`, `docs/atm-daemon/architecture.md`, and
  `docs/atm-daemon/requirements.md` no longer describe the notification worker
  as an accepted production subsystem

## Required Validation

- targeted send-path and daemon composition regression tests
- targeted boundary-lint / boundary-grep gates for notification boundary TOMLs
- `test ! -e crates/atm-daemon/src/notification_runtime.rs`
- `test ! -e crates/atm-daemon/src/notification_runtime_tests.rs`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `git diff --check`
