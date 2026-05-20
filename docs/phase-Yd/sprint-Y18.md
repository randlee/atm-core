---
id: Y.18
title: Thin Liveness Closure And Final Develop Gate
status: complete
branch: feature/pYd-s18-thin-liveness-closure-and-final-develop-gate
worktree: ../atm-core-worktrees/feature/pYd-s18-thin-liveness-closure-and-final-develop-gate
target: integrate/phase-Y
---

# Sprint Y.18 — Thin Liveness Closure And Final Develop Gate

## Goal

- close the remaining minimal operational/liveness gate for `Phase Y`
- leave the final `develop`-gate record
- explicitly unblock `Phase Z` only after the line is ready

## Hard Dependencies

- `docs/phase-Y/issues.md`
- `docs/phase-Yd/plan-phase-Yd.md`
- `docs/phase-Yd/readiness.md`
- `docs/adr/INDEX.md`
- `docs/adr/ADR-014-runtime-health-projection-and-liveness-signal-ownership.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/plan-phase-Z.md`
- `boundaries/atm-core/status-source.toml`
- `boundaries/atm-daemon/daemon-status-source.toml`
- `docs/testing-guidelines.md`
- `Y.17` must close first

## Exact Targets

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/notification_runtime.rs`
- any runtime-owned liveness signal source required by the accepted design
- `docs/atm-daemon/boundaries.md`
- `boundaries/atm-daemon/daemon-status-source.toml`
- `docs/phase-Y/issues.md`
- `docs/phase-Yd/readiness.md`
- `docs/project-plan.md`
- `docs/plan-phase-Z.md`

## Deliverables

- the notification-worker liveness blocker is resolved either by:
  - a thin runtime-owned signal that `runtime_health` projects directly
  - or an explicit documented reclassification to non-blocking in
    `docs/phase-Y/issues.md`
- `runtime_health` remains a projection layer, not a compensating recovery
  engine
- one explicit readiness record states whether `Phase Y` may land on `develop`
  and whether `Phase Z` may begin

## Explicit Code Samples

```rust
// `NotificationWorkerLiveness` is a daemon-owned health DTO that lands in
// `crates/atm-daemon/src/runtime_health.rs`.
pub enum NotificationWorkerLiveness {
    Live,
    Degraded,
    Stopped,
}
```

```rust
pub struct RuntimeHealthSnapshot {
    pub notification_worker_liveness: NotificationWorkerLiveness,
}
```

```rust
// New method on `crates/atm-daemon/src/notification_runtime.rs`.
impl NotificationRuntime {
    pub fn worker_liveness(&self) -> NotificationWorkerLiveness;
}
```

```rust
fn project_runtime_health(
    runtime: &NotificationRuntime,
) -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot {
        notification_worker_liveness: runtime.worker_liveness(),
    }
}
```

```rust
// Maximum acceptable complexity boundary for Y.18:
// runtime_health projects one runtime-owned signal directly.
// It does not infer liveness by reconstructing queue, retry, or worker logic.
```

## Required Work

- close or explicitly reclassify the final liveness/readiness blocker from
  `docs/phase-Y/issues.md` without growing logic-heavy inference inside
  `runtime_health`
- update the readiness record with the final `develop`-gate verdict
- update `Phase Z` docs so they remain blocked until that verdict is positive

## This Sprint Does Not Close

- new `Phase Z` rollout execution
- unrelated daemon hardening or broad observability redesign

## Acceptance Criteria

- `rg -n "queue\\.len|retry_count|pending_events" crates/atm-daemon/src/runtime_health.rs`
  returns no matches
- the final `Phase Y` blocker set is closed or explicitly reclassified with
  documented rationale
- exactly one of the following closure paths is taken:
  - thin-signal path
  - reclassification path
- thin-signal path:
  - any liveness closure uses a thin runtime-owned signal rather than
    compensating logic inside `runtime_health`
  - named test proves the projection sources worker liveness from the runtime
    seam rather than a default or inferred unit value:
    - `runtime_health_projects_worker_liveness_from_notification_runtime`
  - named test proves the health projection does not inspect queue internals:
    - `runtime_health_projection_does_not_inspect_queue_internals`
- reclassification path:
  - `docs/phase-Y/issues.md` contains the reclassification entry and rationale
    stating why the issue no longer blocks landing on `develop`
  - `arch-qa` verifies that reclassification entry and rationale
  - named runtime-health liveness tests are not required for this path
- `docs/phase-Yd/readiness.md` says whether `Phase Y` may land on `develop`
- `docs/phase-Yd/readiness.md` names the final accepted candidate line that is
  authorized for merge to `develop`
- `docs/plan-phase-Z.md` reflects the final `Phase Z` gate state accurately

## Required Validation

- `rg -n "queue\\.len|retry_count|pending_events" crates/atm-daemon/src/runtime_health.rs`
- thin-signal path only:
  - `cargo test --workspace runtime_health_projects_worker_liveness_from_notification_runtime`
  - `cargo test --workspace runtime_health_projection_does_not_inspect_queue_internals`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
