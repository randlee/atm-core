---
id: Y.15
title: Production Notification Boundary Closure
status: planned
branch: feature/pYd-s15-production-notification-boundary-closure
worktree: ../atm-core-worktrees/feature/pYd-s15-production-notification-boundary-closure
target: integrate/phase-Y
---

# Sprint Y.15 — Production Notification Boundary Closure

## Goal

- close the production notification boundary bypass on the `Phase Y` line

## Hard Dependencies

- `docs/phase-Y/issues.md`
- `docs/phase-Yd/plan-phase-Yd.md`
- `docs/phase-Yd/readiness.md`
- `docs/phase-Yc/plan-phase-Yc.md`
- `docs/adr/INDEX.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `boundaries/atm-core/notification-sink.toml`
- `boundaries/atm-daemon/daemon-notification-sink.toml`
- `docs/testing-guidelines.md`
- `Y.14` must close first

## Exact Targets

- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/service_runtime.rs`
- any directly supporting `atm-core` files required to route notifications
  through the approved boundary cleanly
- `docs/phase-Y/issues.md`
- `docs/phase-Yd/readiness.md`
- `docs/project-plan.md`

## Deliverables

- production send/ack notification execution uses `NotificationSink` with no
  direct helper bypass
- the blocker inventory and readiness record explicitly record the `Y.15`
  closure result

## Relationship To Phase Yc

`Y.13` already defined and first proved the `NotificationSink` boundary-closure
shape on the focused `Yc` line.

`Y.15` does not silently reopen that design. It re-proves the same invariant on
the final accepted `Phase Y` candidate line after later accepted line-state
changes. `Y.15` is therefore a final-candidate boundary re-proof sprint, not a
new notification ownership redesign.

## Required Work

- close the production notification execution blocker recorded in
  `docs/phase-Y/issues.md`
- update the blocker inventory and readiness record to reflect the closure
  state
- keep `Phase Z` blocked while the later `Y.16` through `Y.18` closures remain
  open

## This Sprint Does Not Close

- daemon retained-runtime `NotificationSink` installation
- accepted phase-end fix candidate absorption
- final `develop`-gate authorization
- unrelated daemon transport or roster-store redesign

## Acceptance Criteria

- the production notification boundary blocker assigned to `Y.15` in
  `docs/phase-Y/issues.md` is closed or explicitly reclassified with
  documented rationale
- the production notification path executes only through
  `NotificationSink::deliver(...)` and no direct helper bypass remains on the
  accepted line
- `rg -n "maybe_run_post_send_hook" crates/atm-core/src/delivery_execution.rs`
  returns no matches
- `rg -n "fn maybe_run_post_send_hook" crates/atm-core/src/service_runtime.rs`
  returns no matches
- the final accepted `Phase Y` merge candidate is boundary-clean for the
  `Y.15` scope
- `docs/phase-Yd/readiness.md` is updated with the `Y.15` closure result

## Explicit Code Samples

```rust
pub trait NotificationSink: sealed::Sealed {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError>;
}
```

```rust
fn deliver_notifications(
    notification_sink: &dyn NotificationSink,
    event: NotificationEvent,
) -> Result<(), AtmError> {
    notification_sink.deliver(event)
}
```

```rust
// Required call-site shape on the production path:
notification_sink.deliver(notification_event)?;
// Forbidden bypass shape:
// maybe_run_post_send_hook(...)
```

## Required Validation

- `rg -n "maybe_run_post_send_hook" crates/atm-core/src/delivery_execution.rs`
- `rg -n "fn maybe_run_post_send_hook" crates/atm-core/src/service_runtime.rs`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
