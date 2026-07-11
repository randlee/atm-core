---
id: AD.16-win
title: Native Windows Execution For AD.16
status: planned
branch: feature/pAD-s16-thin-graft-receiver-reset
worktree: ../atm-core-worktrees/feature/pAD-s16-thin-graft-receiver-reset
target: integrate/phase-AD
---

# Sprint AD.16-win — Native Windows Execution For AD.16

## Goal

- execute `AD.16` on a real Windows host
- capture and fix any Windows-only defect that blocks closure of `AD.16`

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD16.md` remains the source of truth for
  `AD.16` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.16` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD16.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD16.md`
- `docs/plans/phase-AD/sprint-AD16-win.md`
- any file already in `AD.16` scope that needs Windows-only fix-forward to
  close `AD.16`

## Deliverables

- native Windows execution report for `AD.16`
- any Windows-only fix-forward required to make `AD.16` pass its original
  validation scope on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD16.md` first, then this doc
- the Windows agent works only on the `AD.16` branch/worktree named above
- the Windows agent does not widen `AD.16` scope beyond what
  `docs/plans/phase-AD/sprint-AD16.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs `AD.16`'s original Required Validation on a real
  Windows host where those commands are meaningful
- if a Windows-only defect is found, the fix lands on the same `AD.16`
  worktree and stays within `AD.16` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and any CI run URL used for confirmation

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD16.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD16.md`.
2. Read `docs/plans/phase-AD/sprint-AD16-win.md`.
3. Treat `sprint-AD16.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.16` worktree before running anything.
5. Run the original `AD.16` validation commands on the Windows host.
6. If a Windows-only defect appears, fix it on the same `AD.16` worktree
   without widening sprint scope.
7. Re-run the failed validation plus the full original validation set.
8. Push the branch and report the final commit hash, validation results, and
   any CI evidence.

## Windows Execution Report

- execution date: `2026-07-06`
- host: native Windows
- branch: `feature/pAD-s16-thin-graft-receiver-reset`

### Windows-only fix applied

- none required for runtime or test behavior
- applied formatter-required module ordering in `crates/atm-daemon/src/lib.rs`
  so Windows lint reached a clean pass state

### Commands run

- `python .just/run_lint.py all`
- `just test`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `rg -n "AdvisorySessionPort|ActiveAdvisoryStream|Advisory(Register|Unregister|Fetch|Drain|Stream)" crates/atm-graft`
- `git diff --check`

### Results

- `python .just/run_lint.py all`: PASS
- `just test`: PASS
- `cargo test --workspace`: PASS
- `cargo clippy --workspace -- -D warnings`: PASS
- sprint grep: PASS
- `git diff --check`: PASS

### Remaining findings for mac follow-up

- none from native Windows execution
