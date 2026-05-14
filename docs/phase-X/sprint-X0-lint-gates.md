---
id: X.0
title: Pre-Phase Lint Gates
status: planned
branch: feature/pX-lint-gates
worktree: ../atm-core-worktrees/feature/pX-lint-gates
target: develop
---

# Sprint X.0 — Pre-Phase Lint Gates

## Goal

- land the shared lint gates that must already be active before any
  `integrate/phase-X` sprint branch is created
- make silent-emit and RULE-002 regressions fail from the first push on every
  Phase `X` implementation branch

## Hard Dependencies

- `integrate/phase-W` remains the code baseline for the new guard behavior
- this sprint lands on `develop`, not `integrate/phase-X`
- `X.1` through `X.5` must not start until this sprint is merged

## Exact Targets

- `scripts/check-silent-emit.sh`
- `scripts/check-function-length.py`
- local lint entrypoints:
  - `.just/run_lint.py`
  - `just` recipes that route to lint
- CI workflow files that own the lint path
- any related lint docs that name the active checks

## Required Work

- add the silent-emit regression gate to the shared lint/CI path
- add the RULE-002 function-length gate to the shared lint/CI path
- ensure both gates are reachable through the same entrypoints Phase `X`
  branches will use locally and in CI
- document any grandfather or rollout behavior for pre-existing violations so
  new Phase `X` diffs fail correctly without blocking on unrelated legacy debt

## Acceptance Criteria

- the silent-emit gate is runnable through the normal lint entrypoint
- the RULE-002 gate is runnable through the normal lint entrypoint
- both gates run on `develop` before `integrate/phase-X` is created
- Phase `X` planning docs treat these gates as pre-phase prerequisites, not
  internal `integrate/phase-X` sprint work

## Required Validation

- run the local lint entrypoint that exercises both new gates
- run any direct script invocation needed to prove both gates work in isolation
- `git diff --check`
