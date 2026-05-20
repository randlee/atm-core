---
id: Y.17
title: Candidate Closure
status: planned
branch: feature/pYd-s17-candidate-closure
worktree: ../atm-core-worktrees/feature/pYd-s17-candidate-closure
target: integrate/phase-Y
---

# Sprint Y.17 — Candidate Closure

## Goal

- produce the accepted `Phase Y` merge candidate line with the required
  end-of-phase fixes present and validation-clean

## Hard Dependencies

- `docs/phase-Y/issues.md`
- `docs/phase-Yd/plan-phase-Yd.md`
- `docs/phase-Yd/readiness.md`
- `Y.16` must close first

## Exact Targets

- any files required to absorb the accepted phase-end fix line on the final
  merge candidate
- `docs/phase-Y/issues.md`
- `docs/phase-Yd/readiness.md`
- `docs/project-plan.md`

## Deliverables

- the final accepted merge candidate includes the required phase-end fix line
- the accepted merge candidate passes the required validation stack
- the blocker inventory and readiness record explicitly record the `Y.17`
  closure result

## Required Work

- verify the accepted merge candidate includes the end-of-phase fix line, not
  just the original `Phase Y` integration baseline
- validate the accepted candidate line cleanly
- update the blocker inventory and readiness record to reflect the closure
  state
- keep `Phase Z` blocked while the final `Y.18` gate remains open

## This Sprint Does Not Close

- final liveness/readiness closure
- final `develop`-gate authorization itself
- any broad `Phase Z` rollout or dogfood execution work

## Acceptance Criteria

- the accepted merge candidate includes the required phase-end fix line and is
  validation-clean for the `Y.17` scope
- `docs/phase-Yd/readiness.md` is updated with the `Y.17` closure result

## Required Validation

- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
