---
id: AD.29
title: Phase AD Post-Send Smoke And Windows Daemon Depth
status: complete
branch: feature/pAD-s29-phase-ad-post-send-smoke-and-windows-depth
worktree: ../atm-core-worktrees/feature/pAD-s29-phase-ad-post-send-smoke-and-windows-depth
target: integrate/phase-AD
---

# Sprint AD.29 — Phase AD Post-Send Smoke And Windows Daemon Depth

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
- `docs/plans/phase-AD/readiness.md`

## Exact Targets

- `.github/workflows/ci.yml`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/tests.rs`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `reports/smoke/smoke.md`
- `reports/smoke/smoke-thorough.md`
- `docs/atm-daemon/architecture.md`
- `docs/cross-platform-guidelines.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/readiness.md`
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

The accepted smoke/service-hardening ownership after this sprint is:

- `AD.24` owns any shared smoke harness, environment orchestration, or
  cross-branch smoke scaffolding
- `AD.29` consumes that harness and adds only Phase AD end-gate cases
- the accepted Phase AD end-gate matrix must cover:
  - external hook success
  - external hook partial failure
  - built-in fallback when no external hook matches
  - reset-to-default after a prior explicit override
  - explicit disable behavior if the retained product design keeps that state
- Windows daemon integration depth must include the restored same-host local
  IPC cases that are still Unix-only in the reviewed line:
  - dispatcher panic during shutdown
  - injected accept-error handling
  - post-terminate connection rejection

## Paths To Delete

- ad hoc Phase AD smoke checks that prove only one post-send happy path
- Unix-only gating on Windows-capable local IPC depth tests
- duplicate smoke-plan scope that belongs to the sibling `AD.24` harness sprint

## Deliverables

- one authoritative Phase AD smoke matrix proves the repaired post-send states
  end-to-end
- Windows daemon CI covers the remaining local IPC shutdown/error/rejection
  depth cases named above
- readiness evidence cites the accepted smoke/service-hardening lane directly
  instead of scattering proof across unrelated PR notes
- docs distinguish clearly between shared smoke harness ownership (`AD.24`) and
  Phase AD closure-case ownership (`AD.29`)

## This Sprint Does Not Close

- override lifecycle semantics by themselves
- boundary wiring/accounting by themselves
- template-resolution extraction by itself
- the `atm-graft` deadline-race fix by itself

## Acceptance Criteria

- the authoritative smoke lane passes with evidence for all five Phase AD
  post-send cases
- Windows CI runs the repaired local IPC depth cases rather than treating them
  as Unix-only, specifically:
  - dispatcher panic during shutdown
  - injected accept-error handling
  - post-terminate connection rejection
- readiness docs point to one accepted smoke/service-hardening evidence line
  for final closure
- no duplicated smoke scope remains between `AD.24` and `AD.29`

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `just smoke normal`
- `just smoke thorough`
- targeted Windows/local-IPC integration coverage for dispatcher panic during
  shutdown, accept-error injection, and post-terminate connection rejection
- `git diff --check`
