---
id: AD.17-win
title: Native Windows Execution For AD.17
status: completed
branch: feature/pAD-s17-boundary-reset-verification-closeout
worktree: ../atm-core-worktrees/feature/pAD-s17-boundary-reset-verification-closeout
target: integrate/phase-AD
---

# Sprint AD.17-win — Native Windows Execution For AD.17

## Goal

- execute `AD.17` on a real Windows host
- close the Windows local-IPC and lifecycle gate using native Windows
  execution rather than non-Windows inference
- finish the native Windows-only closeout work that cannot be proven from the
  macOS/Linux side alone

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD17.md` remains the source of truth for
  `AD.17` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.17` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD17.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD17.md`
- `docs/plans/phase-AD/sprint-AD17-win.md`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/host_ownership.rs`
- `crates/atm-daemon/src/composition.rs`
- `.github/workflows/ci.yml`
- any other file already in `AD.17` scope that needs Windows-only fix-forward
  to close `AD.17`

## Native Windows Closure Scope

This historical overlay records the former Windows daemon-lane investigation.
The accepted line now uses the replacement-workspace CI lane. The non-Windows
agent already:

- merged `AD.16` forward into the `AD.17` worktree
- aligned CI to the replacement-workspace test lane
- confirmed the failing cluster is not generic compile trouble, but a
  Windows-path lifecycle/lock-isolation defect

The Windows agent owns the remaining work only where native execution is the
right tool:

- reproduce or confirm the current Windows hang/failure signature on the
  `AD.17` branch
- validate the lifecycle shutdown path on a real Windows host
- validate shared lifecycle-test state reset/teardown on a real Windows host
- validate tempdir-scoped host-ownership locking on a real Windows host
- confirm the replacement-workspace CI lane remains green

Do not spend Windows time redoing macOS/Linux-only lint or non-Windows
investigation that has already been completed elsewhere.

## Required Root Causes To Verify

The current Windows handoff is based on three already-identified findings that
must be treated as the minimum required scope on the `AD.17` branch:

- `FTQ-001`: `crates/atm-daemon/src/lifecycle_control.rs` must keep
  `shutdown_worker_with_timeout()` bounded end-to-end. A helper-thread timeout
  is not sufficient if a later `join()` can still block forever.
- `FTQ-002`: the `windows_tests` shared lifecycle state must be torn down and
  reset between serial Windows tests so one test cannot inherit tripped flags
  or a stale worker registration from another.
- `FTQ-003`: host-ownership locking used by composition tests must resolve via
  the tempdir-scoped `AtmHomeDir`, not the real machine `HOME/USERPROFILE`
  lock path.

If native Windows execution shows these fixes are already sufficient, close the
lane with validation evidence. If Windows reveals an additional defect, keep
the repair on the same `AD.17` worktree and do not widen scope beyond the
replacement-workspace CI lane.

## Deliverables

- native Windows execution report for `AD.17`
- any Windows-only fix-forward required to keep the replacement-workspace CI
  lane green
- exact evidence for the former hang cluster on Windows
- exact evidence that the replacement-workspace CI lane is green on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD17.md` first, then this doc
- the Windows agent works only on the `AD.17` branch/worktree named above
- the Windows agent does not widen `AD.17` scope beyond what
  `docs/plans/phase-AD/sprint-AD17.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent treats `FTQ-001`, `FTQ-002`, and `FTQ-003` as the minimum
  required closure scope for the Windows lane
- the Windows agent runs the targeted Windows tests listed below one by one on
  a real Windows host
- if a Windows-only defect is found, the fix lands on the same `AD.17`
  worktree and stays within `AD.17` scope
- the `CI` workflow keeps the replacement-workspace lane enabled; a manual
  Windows repro is not an acceptable substitute for a green CI result
- the final report records exact commands run, pass/fail results, commit hash,
  and the GitHub Actions run URL showing the replacement-workspace lane

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD17.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `cargo test -p atm-daemon lifecycle_control::windows_tests::windows_terminate_request_wakes_waiters -- --exact --nocapture`
- `cargo test -p atm-daemon lifecycle_control::windows_tests::windows_install_reuses_one_lifecycle_worker_until_shutdown -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::runtime_composition_failed_startup_returns_to_stopped -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::runtime_composition_fails_closed_when_replay_store_cannot_open -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::server_transport_cannot_bootstrap_outside_runtime_composition_start -- --exact --nocapture`
- `cargo test -p atm-daemon`
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD17.md`.
2. Read `docs/plans/phase-AD/sprint-AD17-win.md`.
3. Treat `sprint-AD17.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.17` worktree before running anything.
5. Start by running the targeted lifecycle tests above one by one on the
   Windows host, then run the three targeted composition tests one by one.
6. If any targeted test stalls or exceeds 60 seconds without progress,
   capture the exact test name, last emitted line, and wall-clock duration.
7. Verify the branch actually closes the three known findings:
   `FTQ-001`, `FTQ-002`, and `FTQ-003`. If one is still open, fix that exact
   defect before widening investigation.
8. If a Windows-only defect appears beyond those three findings, fix it on the
   same `AD.17` worktree without widening sprint scope.
9. Re-run the targeted tests, then `cargo test -p atm-daemon`, then the full
   original `AD.17` validation set that is practical on the Windows host.
10. Push the branch and report the final commit hash, validation results, and
   CI evidence showing the replacement-workspace lane green.

## Windows Execution Report

- Date: 2026-07-06
- Host: native Windows (`x86_64-pc-windows-msvc`)
- Branch: `feature/pAD-s17-boundary-reset-verification-closeout`
- Merge-forward source: `origin/feature/pAD-s16-thin-graft-receiver-reset`

### Root Cause

- The initial Windows repro was
  `composition::tests::runtime_composition_failed_startup_returns_to_stopped`
  hanging indefinitely.
- The hang was not a lifecycle-boundary contract regression. On Windows,
  `daemon_local_ipc_name_from_path()` hashes arbitrary logical socket paths
  into `\\.\pipe\...` names, so the test input rooted under
  `not-a-dir/atm.sock` did not fail during endpoint preparation.
- Because startup succeeded instead of failing closed, the test entered the
  serve loop and waited forever for shutdown instead of returning the expected
  startup error.

### Windows Fix

- Preserved all existing boundary contracts; no sealed-trait or cross-crate
  boundary changes were made.
- Added Windows-side logical parent validation in
  `crates/atm-daemon/src/local_ipc_transport/shutdown.rs` for non-pipe endpoint
  inputs before they are converted into legacy local endpoint addresses.
- Added lifecycle reset coverage to
  `composition::tests::runtime_composition_failed_startup_returns_to_stopped`
  so the shared Windows lifecycle worker is drained between serial tests.
- Added a Windows regression test proving
  `prepare_local_ipc_endpoint()` rejects a logical parent that is a file.

### Commands Run

- `cargo test -p atm-daemon lifecycle_control::windows_tests::windows_install_reuses_one_lifecycle_worker_until_shutdown -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::runtime_composition_failed_startup_returns_to_stopped -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::runtime_composition_fails_closed_when_replay_store_cannot_open -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::server_transport_cannot_bootstrap_outside_runtime_composition_start -- --exact --nocapture`
- `just test`
- `just lint`

### Results

- All targeted Windows lifecycle and composition tests passed after the fix.
- `just test` passed on Windows.
- `just lint` passed on Windows, including `fmt`, `clippy`, `boundaries`, and
  the Windows portability checks.
- `FTQ-001`: closed for the AD.17 Windows lane.
- `FTQ-002`: closed for the AD.17 Windows lane.
- `FTQ-003`: closed for the AD.17 Windows lane.
