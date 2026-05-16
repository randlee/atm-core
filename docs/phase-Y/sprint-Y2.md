---
id: Y.2
title: Pre-Smoke Easy Fixes And Validation
status: planned
branch: feature/pY-s2-pre-smoke-easy-fixes
worktree: ../atm-core-worktrees/feature/pY-s2-pre-smoke-easy-fixes
target: integrate/phase-Y
---

# Sprint Y.2 — Pre-Smoke Easy Fixes And Validation

## Goal

- land the second approved small implementation slice before heavy smoke work
- close only narrow, low-risk fixes that are explicitly identified during
  planning and `Y.1`
- validate that the release line is ready for the larger write-boundary work
  that begins in `Y.3`

## Hard Dependencies

- `docs/plan-phase-Y.md`
- `docs/phase-Y/sprint-Y1.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- approved `Y.1` closeout notes

## Exact Targets

- implementation files owned by the approved `Y.2` easy-fix set
- user-facing docs touched by those easy fixes
- `docs/phase-Y/sprint-Y2.md`
- `docs/project-plan.md`

## Required Work

- land only small, approved pre-smoke fixes
- keep scope tight enough that `Y.3` still owns the first serious
  compatibility-write boundary refactor
- validate that `Y.1` and `Y.2` together do not change the working inbox wire
  contract accidentally
- record any larger architectural leftovers for `Y.3+` instead of absorbing
  them here

## Acceptance Criteria

- only explicitly approved easy fixes land
- no accidental compatibility-inbox format change is introduced
- larger boundary work remains queued for `Y.3` rather than partially started
  here

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `git diff --check`
