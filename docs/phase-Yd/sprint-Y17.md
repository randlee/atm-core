---
id: Y.17
title: Thin Liveness Closure And Final Develop Gate
status: planned
branch: feature/pYd-s17-thin-liveness-closure-and-final-develop-gate
worktree: ../atm-core-worktrees/feature/pYd-s17-thin-liveness-closure-and-final-develop-gate
target: integrate/phase-Y
---

# Sprint Y.17 — Thin Liveness Closure And Final Develop Gate

## Goal

- close the remaining minimal operational/liveness gate for `Phase Y`
- leave the final `develop`-gate record
- explicitly unblock `Phase Z` only after the line is ready

## Hard Dependencies

- `docs/phase-Y/issues.md`
- `docs/phase-Yd/plan-phase-Yd.md`
- `docs/phase-Yd/readiness.md`
- `Y.16` must close first

## Exact Targets

- `crates/atm-daemon/src/runtime_health.rs`
- any runtime-owned liveness signal source required by the accepted design
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

- the final `Phase Y` blocker set is closed or explicitly reclassified with
  documented rationale
- any liveness closure uses a thin runtime-owned signal rather than
  compensating logic inside `runtime_health`
- `docs/phase-Yd/readiness.md` says whether `Phase Y` may land on `develop`
- `docs/plan-phase-Z.md` reflects the final `Phase Z` gate state accurately

## Required Validation

- focused readiness validation for the accepted liveness signal
- `cargo test --workspace`
- `git diff --check`
