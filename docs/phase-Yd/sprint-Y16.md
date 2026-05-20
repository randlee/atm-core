---
id: Y.16
title: Retained-Runtime Composition Closure
status: planned
branch: feature/pYd-s16-retained-runtime-composition-closure
worktree: ../atm-core-worktrees/feature/pYd-s16-retained-runtime-composition-closure
target: integrate/phase-Y
---

# Sprint Y.16 — Retained-Runtime Composition Closure

## Goal

- close the remaining production composition blocker on the `Phase Y` line

## Hard Dependencies

- `docs/phase-Y/issues.md`
- `docs/phase-Yd/plan-phase-Yd.md`
- `docs/phase-Yd/readiness.md`
- `docs/adr/INDEX.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `boundaries/atm-core/notification-sink.toml`
- `boundaries/atm-daemon/daemon-notification-sink.toml`
- `boundaries/atm-daemon/runtime-lifecycle-daemon.toml`
- `docs/testing-guidelines.md`
- `Y.15` must close first

## Exact Targets

- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/notification_runtime.rs`
- any directly supporting daemon/runtime assembly files required to install the
  live production `NotificationSink`
- `docs/phase-Y/issues.md`
- `docs/phase-Yd/readiness.md`
- `docs/project-plan.md`

## Deliverables

- daemon retained-runtime composition installs the live production
  `NotificationSink`
- the blocker inventory and readiness record explicitly record the `Y.16`
  closure result

## Required Work

- close the retained-runtime composition blocker recorded in
  `docs/phase-Y/issues.md`
- update the blocker inventory and readiness record to reflect the closure
  state
- keep `Phase Z` blocked while the later `Y.17` and `Y.18` closures remain
  open

## This Sprint Does Not Close

- accepted phase-end fix candidate absorption
- final liveness/readiness closure
- final `develop`-gate authorization itself
- any broad `Phase Z` rollout or dogfood execution work

## Acceptance Criteria

- the retained-runtime composition blocker assigned to `Y.16` in
  `docs/phase-Y/issues.md` is closed or explicitly reclassified with
  documented rationale
- the production retained-runtime path installs the live `NotificationSink`
  without fallback/helper-owned bypass behavior
- `rg -n "DaemonNotificationSink::new" crates/atm-daemon/src/composition.rs`
  returns at least one match on the production factory path
- named test proves production retained-runtime composition installs the live
  notification sink:
  - `production_runtime_installs_daemon_notification_sink`
- `docs/phase-Yd/readiness.md` is updated with the `Y.16` closure result

## Explicit Code Samples

```rust
// Owned by `atm_daemon::composition`.
pub fn build_production_runtime(
    mail_store: Arc<dyn MailStore + Send + Sync>,
    task_store: Arc<dyn TaskStore + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
) -> LocalServiceRuntime {
    LocalServiceRuntime::new_with_delivery_boundaries(
        mail_store,
        task_store,
        roster_store,
        non_claude_outbound,
        notification_sink,
    )
}
```

```rust
let notification_sink: Arc<dyn NotificationSink + Send + Sync> =
    Arc::new(DaemonNotificationSink::new(notification_runtime.clone()));

let runtime = atm_daemon::composition::build_production_runtime(
    mail_store,
    task_store,
    roster_store,
    non_claude_outbound,
    notification_sink,
);
```

## Required Validation

- `rg -n "DaemonNotificationSink::new" crates/atm-daemon/src/composition.rs`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
