---
id: AD.13-win
title: Native Windows Execution For AD.13
status: planned
branch: feature/pAD-s13-ulid-message-identity-reset
worktree: ../atm-core-worktrees/feature/pAD-s13-ulid-message-identity-reset
target: integrate/phase-AD
---

# Sprint AD.13-win — Native Windows Execution For AD.13

## Goal

- execute `AD.13` on a real Windows host
- capture and fix any Windows-only defect that blocks closure of `AD.13`

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD13.md` remains the source of truth for
  `AD.13` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.13` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD13.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD13.md`
- `docs/plans/phase-AD/sprint-AD13-win.md`
- any file already in `AD.13` scope that needs Windows-only fix-forward to
  close `AD.13`

## Deliverables

- native Windows execution report for `AD.13`
- any Windows-only fix-forward required to make `AD.13` pass its original
  validation scope on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD13.md` first, then this doc
- the Windows agent works only on the `AD.13` branch/worktree named above
- the Windows agent does not widen `AD.13` scope beyond what
  `docs/plans/phase-AD/sprint-AD13.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs `AD.13`'s original Required Validation on a real
  Windows host where those commands are meaningful
- if a Windows-only defect is found, the fix lands on the same `AD.13`
  worktree and stays within `AD.13` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and any CI run URL used for confirmation

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD13.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD13.md`.
2. Read `docs/plans/phase-AD/sprint-AD13-win.md`.
3. Treat `sprint-AD13.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.13` worktree before running anything.
5. Run the original `AD.13` validation commands on the Windows host.
6. If a Windows-only defect appears, fix it on the same `AD.13` worktree
   without widening sprint scope.
7. Re-run the failed validation plus the full original validation set.
8. Push the branch and report the final commit hash, validation results, and
   any CI evidence.
