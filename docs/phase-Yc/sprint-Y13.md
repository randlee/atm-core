---
id: Y.13
title: Notification Boundary Closure And Final Readiness Gate
status: planned
branch: feature/pYc-s13-notification-boundary-and-readiness-gate
worktree: ../atm-core-worktrees/feature/pYc-s13-notification-boundary-and-readiness-gate
target: integrate/phase-Y
---

# Sprint Y.13 — Notification Boundary Closure And Final Readiness Gate

## Goal

- close the remaining architectural bypass in production notification execution
- make the delivery executor use `NotificationSink` on the live runtime path
- end the `Yc` line with one focused production-readiness proof for the merged
  `Phase Y` baseline

## Hard Dependencies

- `docs/phase-Yc/sprint-Y12.md` must close first
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/phase-Yc/plan-phase-Yc.md`
- the authoritative implementation baseline is `integrate/phase-Y` plus the
  landed `Y.12` branch

## Exact Targets

- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/boundary/mod.rs`
- `crates/atm-core/src/delivery_plan.rs`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/notification_runtime.rs`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/project-plan.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint, the
sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- `delivery_execution.rs` no longer calls `maybe_run_post_send_hook(...)`
  directly on the production path
- the shared notification executor uses `NotificationSink::deliver(...)` through
  a typed `NotificationEvent` translation seam
- notification failure semantics are explicit and tested; unavailable or
  backpressured notification delivery must degrade through typed warnings or
  errors rather than hidden helper behavior
- the integrated `Phase Y` line gets a focused production-readiness gate that
  specifically rechecks the two `Yc` closure invariants before `Phase Z`
  resumes

## Required Work

- install `NotificationSink` into the retained runtime/executor path used by
  send and ack delivery execution
- replace the direct `maybe_run_post_send_hook(...)` call in
  `PostSendNotificationExecutor` with typed `NotificationTarget ->
  NotificationEvent` translation plus `NotificationSink::deliver(...)`
- make notification degradation behavior explicit in the shared executor path;
  do not bury it inside `service_runtime.rs`
- update ADR/boundary docs so the production notification path is documented as
  boundary-owned and no longer helper-owned
- update the project plan/status docs so `Phase Z` smoke is blocked until the
  `Yc` readiness gate passes and allowed again once it does

## Explicit Code Samples

If the sprint introduces or changes important traits, features, enums, protocol
types, boundary contracts, or execution seams, this section must include
explicit code samples or signatures showing the intended end state.

```rust
pub(crate) trait PostSendNotificationExecutor {
    fn deliver_notifications(
        &self,
        warnings: &mut Vec<WarningEntry>,
        recipient: &ResolvedRecipient,
        recipient_pane_id: Option<&str>,
        notifications: &[NotificationTarget],
    );
}
```

```rust
pub trait NotificationSink: sealed::Sealed {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError>;
}
```

```rust
fn notification_event_from_target(
    recipient: &ResolvedRecipient,
    recipient_pane_id: Option<&str>,
    target: &NotificationTarget,
) -> NotificationEvent;
```

## This Sprint Does Not Close

- any new event-family state-machine design beyond the already approved `Phase Y`
  and `Phase Yb` delivery-plan seam
- any new smoke/dogfood execution work inside `Phase Z` itself
- any unrelated daemon transport or roster-store redesign

## Acceptance Criteria

- `rg -n "maybe_run_post_send_hook" crates/atm-core/src/delivery_execution.rs`
  returns no matches
- the production notification path is reachable only through
  `NotificationSink::deliver(...)` from the shared executor seam
- named tests prove notification translation and degradation behavior:
  - `delivery_notifications_use_notification_sink_boundary`
  - `notification_sink_failure_is_explicit_in_delivery_warnings`
  - `notification_sink_backpressure_does_not_reopen_hook_helper_bypass`
- the sprint leaves one explicit readiness record in the docs that says:
  - `Y.12` closed the Claude recovered-message-set contract
  - `Y.13` closed the notification boundary bypass
  - `Phase Z` smoke may resume only after both proofs pass

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
