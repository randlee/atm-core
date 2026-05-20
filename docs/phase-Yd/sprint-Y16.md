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
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `Y.15` must close first

## Exact Targets

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
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
- `docs/phase-Yd/readiness.md` is updated with the `Y.16` closure result

## Required Validation

- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
