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
- `docs/adr/INDEX.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/testing-guidelines.md`
- accepted phase-end fix evidence from:
  - `feature/pY-eop-fix-1` through tip commit `243e473a` (`PY-EOP-FIX-R2`)
- `Y.16` must close first

## Exact Targets

- accepted merge candidate branch:
  - `integrate/phase-Y`
- accepted phase-end fix line source:
  - `feature/pY-eop-fix-1`
- files changed by accepted phase-end fix evidence:
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `crates/atm-core/src/delivery_execution.rs`
  - `crates/atm-daemon/src/reconcile_runtime.rs`
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/composition/lifecycle.rs`
  - `crates/atm-daemon/src/runtime_status.rs`
  - any directly adjacent files required to merge those accepted fixes onto the
    final candidate line cleanly
- the accepted merge candidate branch state itself
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
- treat `PY-EOP-FIX-R2` at `243e473a` as the authoritative superseding tip of
  `feature/pY-eop-fix-1`; `PY-EOP-FIX-1` remains historical context only and
  does not stay as a second independent candidate gate
- validate the accepted candidate line cleanly
- record the accepted candidate commit identity in
  `docs/phase-Yd/readiness.md` so QA does not have to infer which line
  satisfied the candidate gate
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
- git confirms commit `243e473a` (`PY-EOP-FIX-R2`) is an ancestor of the
  accepted merge candidate
- the accepted candidate commit used for the `Y.17` gate is named explicitly
  in `docs/phase-Yd/readiness.md`
- `docs/phase-Yd/readiness.md` is updated with the `Y.17` closure result

## Required Validation

- `git merge-base --is-ancestor 243e473a integrate/phase-Y && echo PASS || echo FAIL`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
