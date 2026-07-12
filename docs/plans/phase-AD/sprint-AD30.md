---
id: AD.30
title: Windows Daemon Integration-Depth Coverage Closeout
status: complete
branch: feature/pAD-s30-windows-daemon-integration-depth
worktree: ../atm-core-worktrees/feature/pAD-s30-windows-daemon-integration-depth
target: integrate/phase-AD
---

# Sprint AD.30 — Windows Daemon Integration-Depth Coverage Closeout

## Goal

- close `RSH-AD-END-001` by restoring the missing Windows daemon integration
  depth cases for local IPC shutdown, accept-error handling, and
  post-terminate rejection

## Hard Dependencies

- `AD.17` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- ATM message `01KX1P4D0SEZXWW90VW2F7FF27` from `quality-mgr`,
  `2026-07-08`, subject `PHASE-AD-END-QA FINAL VERDICT`

## Exact Targets

- `.github/workflows/ci.yml`
- `.triage/phase-AD/direct-fix-track.md`
- `CHANGELOG.md`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/tests.rs`
- `docs/atm-daemon/architecture.md`
- `docs/cross-platform-guidelines.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/readiness.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD30.md`

## Interfaces To Add Or Modify

The accepted Windows daemon integration-depth matrix after this sprint is:

```rust
pub enum WindowsLocalIpcDepthCase {
    DispatcherPanicDuringShutdown,
    AcceptErrorInjection,
    PostTerminateConnectionRejection,
}
```

Required runtime/test meaning after this sprint:

- the restored Windows daemon lane covers the same accepted shutdown/error
  contract family already exercised on Unix for these cases
- Windows coverage must use the accepted local IPC injection hooks rather than
  platform-specific test-only behavior that changes the contract under test
- the repaired tests fail fast with typed error output; no Windows lane may
  hang waiting for a lifecycle condition that can be asserted deterministically
- scenario-specific pass/fail semantics are fixed:
  - `DispatcherPanicDuringShutdown`
    - trigger: the dispatcher panics after the accept loop has admitted work
      and shutdown has started
    - required outcome: the daemon records the panic-path failure once, exits
      the affected runtime path without hanging, and the test observes bounded
      shutdown completion rather than a stuck join
  - `AcceptErrorInjection`
    - trigger: the listener injects a typed accept failure while the daemon is
      otherwise healthy
    - required outcome: the accept loop logs the injected failure once and
      exits the affected serve path with a typed error inside the bounded
      deadline; it must not spin, busy-retry, or hang on the failing accept
      path
  - `PostTerminateConnectionRejection`
    - trigger: a client attempts a new connection after the daemon runtime has
      already terminated
    - required outcome: the client receives a fast typed rejection/failure and
      the test does not wait on a socket/pipe path that can no longer succeed

## Paths To Delete

- Unix-only gating on Windows-capable local IPC integration-depth tests
- any claim that Windows daemon CI is complete while these three depth cases
  remain uncovered

## Deliverables

- Windows daemon CI covers:
  - dispatcher panic during shutdown
  - injected accept-error handling
  - post-terminate connection rejection
- docs describe the restored Windows integration-depth contract clearly and do
  not overstate coverage before these cases land
- Phase AD planning/index docs treat this Windows depth closure as separate
  from the post-send smoke matrix closure in `AD.29`
- `docs/plans/phase-AD/readiness.md` exists and is the sole authoritative
  closeout artifact for `AD.25` through `AD.30`
- `.triage/phase-AD/direct-fix-track.md` records the remaining closure-only
  obligations from plan review:
  - `AD9-BLANKPANE-001`
  - `ERRDOC-001`
  - historical `FTQ-001` reconciliation
  - the phase-AD triage sweep ledger
  - the release-facing `CHANGELOG.md` entry for `AD.13` through `AD.30`
- `CHANGELOG.md` carries the release-facing entry for the `AD.13` through
  `AD.30` corrective line before this sprint can close

## This Sprint Does Not Close

- the post-send smoke matrix from `AD.29`
- override lifecycle/reset semantics
- post-send boundary wiring/accounting
- upstream template-resolution extraction
- the `atm-graft` host-nudge race

## Acceptance Criteria

- Windows CI runs the repaired local IPC depth cases rather than treating them
  as Unix-only, specifically:
  - dispatcher panic during shutdown
  - injected accept-error handling
  - post-terminate connection rejection
- each restored Windows case has one explicit observable success condition:
  - dispatcher panic during shutdown: bounded shutdown plus one panic-path log
    record, no hang
  - injected accept-error handling: one logged accept failure plus one bounded
    typed failure path, no tight spin and no hang
  - post-terminate connection rejection: fast typed rejection after terminate,
    no late successful connection
- the repaired Windows tests fail fast and do not rely on manual runner
  intervention or long timeout-based hangs
- docs and Phase AD planning/index records keep this Windows daemon depth scope
  separate from the `AD.29` post-send smoke matrix
- `docs/plans/phase-AD/readiness.md` exists and records `AD.25` through
  `AD.30` closure only when both the `AD.29` smoke evidence and the `AD.30`
  Windows-depth evidence are present
- `.triage/phase-AD/direct-fix-track.md` exists and names the owner plus
  closure artifact for `AD9-BLANKPANE-001`, `ERRDOC-001`, historical
  `FTQ-001`, the phase-AD triage sweep, and the `AD.13` through `AD.30`
  `CHANGELOG.md` entry
- `CHANGELOG.md` contains the release-facing `AD.13` through `AD.30` entry on
  the accepted line

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted Windows/local-IPC integration coverage for dispatcher panic during
  shutdown, accept-error injection, and post-terminate connection rejection
- `git diff --check`
