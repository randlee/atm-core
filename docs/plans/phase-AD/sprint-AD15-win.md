---
id: AD.15-win
title: Native Windows Execution For AD.15
status: planned
branch: feature/pAD-s15-daemon-advisory-runtime-deletion
worktree: ../atm-core-worktrees/feature/pAD-s15-daemon-advisory-runtime-deletion
target: integrate/phase-AD
---

# Sprint AD.15-win — Native Windows Execution For AD.15

## Goal

- execute `AD.15` on a real Windows host
- capture and fix any Windows-only defect that blocks closure of `AD.15`

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD15.md` remains the source of truth for
  `AD.15` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.15` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD15.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD15.md`
- `docs/plans/phase-AD/sprint-AD15-win.md`
- any file already in `AD.15` scope that needs Windows-only fix-forward to
  close `AD.15`

## Deliverables

- native Windows execution report for `AD.15`
- any Windows-only fix-forward required to make `AD.15` pass its original
  validation scope on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD15.md` first, then this doc
- the Windows agent works only on the `AD.15` branch/worktree named above
- the Windows agent does not widen `AD.15` scope beyond what
  `docs/plans/phase-AD/sprint-AD15.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs `AD.15`'s original Required Validation on a real
  Windows host where those commands are meaningful
- if a Windows-only defect is found, the fix lands on the same `AD.15`
  worktree and stays within `AD.15` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and any CI run URL used for confirmation

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD15.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD15.md`.
2. Read `docs/plans/phase-AD/sprint-AD15-win.md`.
3. Treat `sprint-AD15.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.15` worktree before running anything.
5. Run the original `AD.15` validation commands on the Windows host.
6. If a Windows-only defect appears, fix it on the same `AD.15` worktree
   without widening sprint scope.
7. Re-run the failed validation plus the full original validation set.
8. Push the branch and report the final commit hash, validation results, and
   any CI evidence.

## Windows Execution Report

- execution date: `2026-07-06`
- host: native Windows
- branch: `feature/pAD-s15-daemon-advisory-runtime-deletion`

### Windows-only fix applied

- updated `crates/atm-daemon/src/tests.rs` to remove the stale extra
  `graft_dispatcher` argument from `serve_with_runtime_hooks(...)`
- this aligned the carried-forward AD.14 Windows fix with the current AD.15
  transport API and removed the Windows compile/test failure

### Commands run

- `python .just/run_lint.py all`
- `just test`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `rg -n "AdvisoryRuntime|dispatch_advisory_stream|RequestEnvelope::Advisory|ResponseEnvelope::Advisory|LocalIpcAdvisoryStreamSink" crates/atm-daemon`
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
