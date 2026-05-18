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
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-rusqlite/src/boundary_assembly.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-runtime-test-support/src/lib.rs`
- `crates/atm-daemon/src/tests.rs`
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
- delete `LocalServiceRuntime::new(...)` and
  `LocalServiceRuntime::new_with_non_claude_outbound(...)`
- replace the current multiple-constructor surface with one public composition
  constructor that requires the full delivery-boundary set needed by the
  retained runtime
- update every cross-crate runtime assembly site to use that one constructor:
  - `atm-daemon` production retained runtime
  - `atm-rusqlite` default retained runtime
  - `atm-runtime-test-support` cached test runtimes
  - daemon/unit integration tests that build `LocalServiceRuntime` directly
- update ADR/boundary docs so the production notification path is documented as
  boundary-owned and no longer helper-owned
- update the project plan/status docs so `Phase Z` smoke is blocked until the
  `Yc` readiness gate passes and allowed again once it does

## Paths To Delete

- `crates/atm-core/src/delivery_execution.rs`
  - delete the blanket `PostSendNotificationExecutor` implementation path that
    calls `self.maybe_run_post_send_hook(...)` directly
- `crates/atm-core/src/service_runtime.rs`
  - delete `RetainedServiceRuntime::maybe_run_post_send_hook(...)` from the
    retained runtime trait once the shared executor no longer depends on it
  - delete the `LocalServiceRuntime` implementation of
    `maybe_run_post_send_hook(...)` as a production delivery path
  - delete `LocalServiceRuntime::new(...)`
  - delete `LocalServiceRuntime::new_with_non_claude_outbound(...)`
- `crates/atm-daemon/src/runtime_health.rs`
  - delete the retained-runtime factory constructor path that installs
    `LocalServiceRuntime::new_with_non_claude_outbound(...)` without also
    wiring the production `NotificationSink`
- `crates/atm-rusqlite/src/boundary_assembly.rs`
  - delete constructor callsites that still assemble `LocalServiceRuntime`
    without an explicit `NotificationSink`
- `crates/atm-runtime-test-support/src/lib.rs`
  - delete constructor callsites that still assemble `LocalServiceRuntime`
    through the legacy constructor surface
- `crates/atm-daemon/src/tests.rs`
  - delete constructor callsites that still assemble `LocalServiceRuntime`
    through the legacy constructor surface
- `crates/atm-rusqlite/src/lib.rs`
  - delete retained test/runtime constructor callsites that still depend on the
    legacy constructor surface

## Approved Surviving Paths

- production notification delivery may survive only through:
  - `atm_core::boundary::NotificationSink::deliver(...)`
  - `atm_daemon::boundary_adapters::DaemonNotificationSink`
  - `atm_daemon::notification_runtime::NotificationRuntime::deliver(...)`
- reconcile/runtime notification side effects already using
  `NotificationSink::deliver(...)` remain valid and must not be rewritten back
  to helper-owned hooks
- daemon retained-runtime composition must survive as one explicit constructor
  that installs:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
  - `NonClaudeOutbound`
  - `NotificationSink`
- non-daemon local runtime assembly may survive only through that same
  constructor shape with explicit fallback/test adapters supplied by the caller
- the surviving constructor remains public because current runtime assembly is
  cross-crate:
  - `atm-daemon` composes the production retained runtime
  - `atm-rusqlite` composes the default retained runtime
  - `atm-runtime-test-support` composes cached test runtimes
- `Y.13` does not close constructor visibility reduction; it closes
  constructor-shape narrowing to one approved public composition path

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
pub struct LocalServiceRuntime {
    pub(crate) mail_store: Arc<dyn MailStore + Send + Sync>,
    pub(crate) task_store: Arc<dyn TaskStore + Send + Sync>,
    pub(crate) roster_store: Arc<dyn RosterStore + Send + Sync>,
    pub(crate) non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    pub(crate) notification_sink: Arc<dyn NotificationSink + Send + Sync>,
}
```

```rust
pub fn new_with_delivery_boundaries(
    mail_store: Arc<dyn MailStore + Send + Sync>,
    task_store: Arc<dyn TaskStore + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
) -> Self;
```

```rust
pub struct LocalFileNotificationSink;
```

```rust
fn notification_event_from_target(
    recipient: &ResolvedRecipient,
    recipient_pane_id: Option<&str>,
    target: &NotificationTarget,
) -> NotificationEvent;
```

## This Sprint Does Not Close

- constructor visibility reduction to `pub(crate)` or private factory-only
  assembly
- any new event-family state-machine design beyond the already approved `Phase Y`
  and `Phase Yb` delivery-plan seam
- any new smoke/dogfood execution work inside `Phase Z` itself
- any unrelated daemon transport or roster-store redesign
- post-mortem lint recommendations or rule additions from
  `integrate/phase-Y/.triage/phase-Yb/post-mortem.md`

## Acceptance Criteria

- `rg -n "maybe_run_post_send_hook" crates/atm-core/src/delivery_execution.rs`
  returns no matches
- `rg -n "fn maybe_run_post_send_hook" crates/atm-core/src/service_runtime.rs`
  returns no matches
- `rg -n "pub fn new\\(" crates/atm-core/src/service_runtime.rs`
  returns no matches
- `rg -n "new_with_non_claude_outbound" crates/atm-daemon/src/runtime_health.rs`
  returns no matches
- `rg -n "new_with_non_claude_outbound" crates` returns no matches
- the production notification path is reachable only through
  `NotificationSink::deliver(...)` from the shared executor seam
- `LocalServiceRuntime` exposes exactly one approved public constructor for the
  retained delivery-boundary composition shape
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
