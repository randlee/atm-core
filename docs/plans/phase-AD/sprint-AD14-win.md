---
id: AD.14-win
title: Native Windows Execution For AD.14
status: planned
branch: feature/pAD-s14-shared-graft-boundary-surface-reset
worktree: ../atm-core-worktrees/feature/pAD-s14-shared-graft-boundary-surface-reset
target: integrate/phase-AD
---

# Sprint AD.14-win — Native Windows Execution For AD.14

## Goal

- execute `AD.14` on a real Windows host
- capture and fix any Windows-only defect that blocks closure of `AD.14`

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD14.md` remains the source of truth for
  `AD.14` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.14` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD14.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD14.md`
- `docs/plans/phase-AD/sprint-AD14-win.md`
- any file already in `AD.14` scope that needs Windows-only fix-forward to
  close `AD.14`

## Deliverables

- native Windows execution report for `AD.14`
- any Windows-only fix-forward required to make `AD.14` pass its original
  validation scope on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD14.md` first, then this doc
- the Windows agent works only on the `AD.14` branch/worktree named above
- the Windows agent does not widen `AD.14` scope beyond what
  `docs/plans/phase-AD/sprint-AD14.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs `AD.14`'s original Required Validation on a real
  Windows host where those commands are meaningful
- if a Windows-only defect is found, the fix lands on the same `AD.14`
  worktree and stays within `AD.14` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and any CI run URL used for confirmation

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD14.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD14.md`.
2. Read `docs/plans/phase-AD/sprint-AD14-win.md`.
3. Treat `sprint-AD14.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.14` worktree before running anything.
5. Run the original `AD.14` validation commands on the Windows host.
6. If a Windows-only defect appears, fix it on the same `AD.14` worktree
   without widening sprint scope.
7. Re-run the failed validation plus the full original validation set.
8. Push the branch and report the final commit hash, validation results, and
   any CI evidence.

## Windows Execution Report

- execution date: `2026-07-06`
- host: native Windows
- branch: `feature/pAD-s14-shared-graft-boundary-surface-reset`

### Windows-only fix applied

- updated `crates/atm-daemon/src/tests.rs` to pass the current
  `graft_dispatcher` argument (`None`) to
  `serve_with_runtime_hooks(...)`
- this fixed the Windows compile/test failure introduced after the AD.13
  merge-forward

### Commands run

- `python .just/run_lint.py all`
- `just test`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `rg -n "dispatch_advisory_stream|AdvisoryStreamSink|AdvisorySessionPort|Advisory(Register|Unregister|Fetch|Drain|Stream)|open_advisory_stream" crates/atm-core crates/atm-daemon-client crates/atm boundaries/atm-daemon-client/rpc-envelope.toml`

### Results

- `python .just/run_lint.py all`: PASS
- `just test`: PASS
- `cargo test --workspace`: PASS
- `cargo clippy --workspace -- -D warnings`: PASS
- `git diff --check`: PASS
- sprint grep: FAIL

### Remaining findings for mac follow-up

- `AD.14` branch still contains advisory/graft RPC matches in
  `crates/atm-daemon-client/src/graft_rpc.rs`
- this is not caused by the Windows fix
- boundary lint passes when the branch contract is kept unchanged
- no boundary-contract changes were kept in this Windows fix-forward
