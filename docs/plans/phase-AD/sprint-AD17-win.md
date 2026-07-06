---
id: AD.17-win
title: Native Windows Execution For AD.17
status: planned
branch: feature/pAD-s17-boundary-reset-verification-closeout
worktree: ../atm-core-worktrees/feature/pAD-s17-boundary-reset-verification-closeout
target: integrate/phase-AD
---

# Sprint AD.17-win — Native Windows Execution For AD.17

## Goal

- execute `AD.17` on a real Windows host
- close the Windows local-IPC and lifecycle gate using native Windows
  execution rather than non-Windows inference

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
- any file already in `AD.17` scope that needs Windows-only fix-forward to
  close `AD.17`

## Deliverables

- native Windows execution report for `AD.17`
- any Windows-only fix-forward required to make the restored
  `windows-latest` `atm-daemon` lane green
- exact evidence for the former hang cluster on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD17.md` first, then this doc
- the Windows agent works only on the `AD.17` branch/worktree named above
- the Windows agent does not widen `AD.17` scope beyond what
  `docs/plans/phase-AD/sprint-AD17.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs the targeted Windows tests listed below one by one on
  a real Windows host
- if a Windows-only defect is found, the fix lands on the same `AD.17`
  worktree and stays within `AD.17` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and the GitHub Actions run URL showing the restored `windows-latest` lane

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD17.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `cargo test -p atm-daemon lifecycle_control::windows_tests::windows_install_reuses_one_lifecycle_worker_until_shutdown -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::runtime_composition_failed_startup_returns_to_stopped -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::runtime_composition_fails_closed_when_replay_store_cannot_open -- --exact --nocapture`
- `cargo test -p atm-daemon composition::tests::server_transport_cannot_bootstrap_outside_runtime_composition_start -- --exact --nocapture`
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD17.md`.
2. Read `docs/plans/phase-AD/sprint-AD17-win.md`.
3. Treat `sprint-AD17.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.17` worktree before running anything.
5. Run the four targeted tests above one by one on the Windows host.
6. If any targeted test stalls or exceeds 60 seconds without progress,
   capture the exact test name, last emitted line, and wall-clock duration.
7. If a Windows-only defect appears, fix it on the same `AD.17` worktree
   without widening sprint scope.
8. Re-run the targeted tests, then the full original `AD.17` validation set.
9. Push the branch and report the final commit hash, validation results, and
   CI evidence.
