---
id: AD.29
title: Phase AD Post-Send Smoke Matrix Closeout
status: planned
branch: feature/pAD-s29-phase-ad-post-send-smoke-matrix
worktree: ../atm-core-worktrees/feature/pAD-s29-phase-ad-post-send-smoke-matrix
target: integrate/phase-AD
---

# Sprint AD.29 — Phase AD Post-Send Smoke Matrix Closeout

## Goal

- close the phase-end proof gap with one authoritative smoke/service-hardening
  lane that demonstrates the repaired post-send matrix and the remaining
  Windows daemon integration depth cases on the same accepted evidence line

## Hard Dependencies

- `AD.24` sibling smoke-harness plan accepted
- `AD.25` complete
- `AD.26` complete
- `AD.27` complete
- `AD.28` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

`AD.28` is a functional dependency, not just numeric merge order: this smoke
matrix includes graft-backed post-send delivery, and that case would remain
flaky until the host-nudge timing race from `AD.28` is closed.

## Exact Targets

- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `reports/smoke/smoke.md`
- `reports/smoke/smoke-thorough.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD29.md`

## Interfaces To Add Or Modify

The authoritative Phase AD post-send smoke matrix after this sprint is:

```rust
pub enum PhaseAdPostSendSmokeCase {
    ExternalHookSuccess,
    ExternalHookPartialFailure,
    BuiltInFallback,
    OverrideResetToDefault,
    OverrideDisabled,
}
```

The accepted smoke ownership after this sprint is:

- `AD.24` owns any shared smoke harness, environment orchestration, or
  cross-branch smoke scaffolding
- `AD.29` consumes that harness and adds only Phase AD end-gate cases
- the accepted Phase AD end-gate matrix must cover:
  - external hook success
  - external hook partial failure
  - built-in fallback when no external hook matches
  - reset-to-default after a prior explicit override
  - explicit disable behavior if the retained product design keeps that state

## Paths To Delete

- ad hoc Phase AD smoke checks that prove only one post-send happy path
- duplicate smoke-plan scope that belongs to the sibling `AD.24` harness sprint

## Deliverables

- one authoritative Phase AD smoke matrix proves the repaired post-send states
  end-to-end
- readiness evidence cites the accepted smoke/service-hardening lane directly
  instead of scattering proof across unrelated PR notes
- docs distinguish clearly between shared smoke harness ownership (`AD.24`) and
  Phase AD closure-case ownership (`AD.29`)

## This Sprint Does Not Close

- override lifecycle semantics by themselves
- boundary wiring/accounting by themselves
- template-resolution extraction by itself
- the `atm-graft` deadline-race fix by itself
- Windows daemon integration-depth closure from `RSH-AD-END-001`

## Acceptance Criteria

- the authoritative smoke lane passes with evidence for all five Phase AD
  post-send cases
- readiness docs point to one accepted smoke/service-hardening evidence line
  for final closure
- no duplicated smoke scope remains between `AD.24` and `AD.29`

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `just smoke normal`
- `just smoke thorough`
- `git diff --check`
