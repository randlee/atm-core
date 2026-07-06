---
id: AD.19-win
title: Native Windows Execution For AD.19
status: planned
branch: feature/pAD-s19-read-mutation-output-consistency-repair
worktree: ../atm-core-worktrees/feature/pAD-s19-read-mutation-output-consistency-repair
target: integrate/phase-AD
---

# Sprint AD.19-win — Native Windows Execution For AD.19

## Goal

- execute `AD.19` on a real Windows host
- capture and fix any Windows-only defect that blocks closure of `AD.19`

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD19.md` remains the source of truth for
  `AD.19` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.19` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD19.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD19.md`
- `docs/plans/phase-AD/sprint-AD19-win.md`
- any file already in `AD.19` scope that needs Windows-only fix-forward to
  close `AD.19`

## Deliverables

- native Windows execution report for `AD.19`
- any Windows-only fix-forward required to make `AD.19` pass its original
  validation scope on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD19.md` first, then this doc
- the Windows agent works only on the `AD.19` branch/worktree named above
- the Windows agent does not widen `AD.19` scope beyond what
  `docs/plans/phase-AD/sprint-AD19.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs `AD.19`'s original Required Validation on a real
  Windows host where those commands are meaningful
- if a Windows-only defect is found, the fix lands on the same `AD.19`
  worktree and stays within `AD.19` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and any CI run URL used for confirmation

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD19.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD19.md`.
2. Read `docs/plans/phase-AD/sprint-AD19-win.md`.
3. Treat `sprint-AD19.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.19` worktree before running anything.
5. Run the original `AD.19` validation commands on the Windows host.
6. If a Windows-only defect appears, fix it on the same `AD.19` worktree
   without widening sprint scope.
7. Re-run the failed validation plus the full original validation set.
8. Push the branch and report the final commit hash, validation results, and
   any CI evidence.
