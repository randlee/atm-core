---
id: Y.13
title: Notification Boundary Closure And Final Readiness Gate
status: complete
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
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-rusqlite/src/boundary_assembly.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-runtime-test-support/src/lib.rs`
- `crates/atm-daemon/src/tests.rs`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/project-plan.md` (Section 33, `Phase Yc Final Production-Readiness Closure`)
- `docs/phase-Yc/readiness.md`

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
  specifically rechecks the two `Yc` closure invariants before the later
  `Phase Yd` develop-gate closeout proceeds
- the sprint leaves one explicit readiness record artifact,
  `docs/phase-Yc/readiness.md`, naming both `Yc` closure invariants and the
  handoff into `Phase Yd`

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
  - `atm-daemon/src/runtime_health.rs` runtime-health retained runtime wiring
  - `atm-rusqlite` default retained runtime
  - `atm-runtime-test-support` cached test runtimes
  - daemon/unit integration tests that build `LocalServiceRuntime` directly
- update ADR/boundary docs so the production notification path is documented as
  boundary-owned and no longer helper-owned
- update the project plan/status docs so `Yc` closes the two original runtime
  blockers but `Phase Z` stays blocked until the later `Phase Yd`
  develop-gate record says it may begin

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
  - delete retained test/runtime callsites that still construct
    `LocalServiceRuntime` through `new(...)` or
    `new_with_non_claude_outbound(...)`

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
        // `recipient_pane_id` stays as `Option<&str>` for Y.13 because the
        // canonical pane identifier type-normalization line is separate work;
        // this sprint closes notification-boundary ownership, not pane-id
        // type redesign.
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
pub struct LocalFileNotificationSink {
    path: PathBuf,
}

impl LocalFileNotificationSink {
    pub fn at_path(path: PathBuf) -> Self {
        Self { path }
    }
}
// Owned by `atm_core::service_runtime`.
// Role: fallback non-daemon `NotificationSink` adapter for local/test runtime
// assembly. Non-daemon callers construct it explicitly with
// `LocalFileNotificationSink::at_path(path)` and it appends newline-delimited
// serialized `NotificationEvent` payloads to that path, returning typed
// `AtmError` on file open/write failure rather than swallowing the event.
```

```rust
fn notification_event_from_target(
    recipient: &ResolvedRecipient,
    recipient_pane_id: Option<&str>,
    target: &NotificationTarget,
) -> NotificationEvent;
```

## Error Inventory

- `NotificationSink::deliver(...)` returns `AtmError` when:
  - the daemon-backed notification queue is unavailable
  - the daemon-backed notification queue is full and delivery is
    backpressured; this path currently returns
    `AtmErrorCode::DaemonUnavailable` (`ATM_DAEMON_UNAVAILABLE`)
  - the fallback local file sink cannot persist the notification event
- ack-path notification failure policy:
  - notification failure on the ack path degrades to a typed warning entry and
    does not silently drop the event
  - it must not revert the already-valid ack outcome solely because the
    side-effect sink is unavailable
- backpressure handling must log a structured warning/observability event in
  addition to surfacing the typed warning result
- shutdown/drain policy remains bounded:
  - already-accepted queued notification events are drained during normal
    shutdown up to the existing `3s` runtime shutdown deadline
  - if that `3s` deadline is exceeded, the runtime emits a structured warning,
    returns `AtmErrorCode::DaemonUnavailable`
    (`ATM_DAEMON_UNAVAILABLE`), detaches the join helper, and any still-pending
    queued notification events are treated as dropped
  - Y.13 must not introduce an unbounded flush loop
- startup/liveness check:
  - the readiness record must state that the production retained runtime can
    construct a live `NotificationSink` and accept at least one notification
    request on the send/ack path

## This Sprint Does Not Close

- constructor visibility reduction to `pub(crate)` or private factory-only
  assembly
- any new event-family state-machine design beyond the already approved `Phase Y`
  and `Phase Yb` delivery-plan seam
- any new smoke/dogfood execution work inside `Phase Z` itself
- any unrelated daemon transport or roster-store redesign
- post-mortem lint recommendations or rule additions from
  `integrate/phase-Y/.triage/phase-Yb/post-mortem.md`
- eliminating the remaining accepted limitation that synchronous file append
  cannot be interrupted once a notification write has started; Y.13 bounds the
  shutdown drain before each persistence step and leaves a direct stalled-write
  harness as `Y.14` follow-on work

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
    must prove that a warning entry is populated and the failure is not
    swallowed by direct helper-owned behavior
  - `notification_sink_backpressure_does_not_reopen_hook_helper_bypass`
- the sprint leaves one explicit readiness record in the docs that says:
  - `Y.12` closed the Claude recovered-message-set contract
  - `Y.13` closed the notification boundary bypass
  - `Phase Yd` may now consume the focused `Yc` readiness result, but
    `Phase Z` remains blocked until `docs/phase-Yd/readiness.md` says it may
    begin

## Required Validation

- `rg -n "maybe_run_post_send_hook" crates/atm-core/src/delivery_execution.rs`
- `rg -n "fn maybe_run_post_send_hook" crates/atm-core/src/service_runtime.rs`
- `rg -n "pub fn new\\(" crates/atm-core/src/service_runtime.rs`
- `rg -n "new_with_non_claude_outbound" crates/atm-daemon/src/runtime_health.rs`
- `rg -n "new_with_non_claude_outbound" crates`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

## Completion Record

- `Y.13` is complete on
  `feature/pYc-s13-notification-boundary-and-readiness-gate`
- the retained send/ack notification path now translates
  `NotificationTarget -> NotificationEvent` in the shared executor and delivers
  through `NotificationSink::deliver(...)`
- `LocalServiceRuntime` now exposes one approved public constructor,
  `new_with_delivery_boundaries(...)`, and all retained-runtime assembly sites
  were updated to use it
