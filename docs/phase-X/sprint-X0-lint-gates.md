---
id: X.0
title: Pre-Phase Lint Gates
status: complete
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
  - `justfile`
  - `.just/print_help.py`
- CI workflow files that own the lint path:
  - `.github/workflows/ci.yml`
- lint-tool unit tests:
  - `.just/tests/test_run_lint.py`
  - `.just/tests/test_print_help.py`
  - `.just/tests/test_check_function_length.py`

## Required Work

- add the silent-emit regression gate to the shared lint/CI path
- add the RULE-002 function-length gate to the shared lint/CI path
- ensure both gates are reachable through the same entrypoints Phase `X`
  branches will use locally and in CI
- keep the silent-emit gate strict immediately because the merged Phase `W`
  line is already clean
- implement RULE-002 rollout as:
  - advisory at `70` lines
  - hard gate at `80` lines
  - grandfather unchanged pre-existing `80+` violations by diff overlap against
    the develop baseline
- record the pre-phase prerequisite in `docs/project-plan.md`

## Acceptance Criteria

- the silent-emit gate is runnable through the normal lint entrypoint
- the RULE-002 gate is runnable through the normal lint entrypoint
- both gates run on `develop` before `integrate/phase-X` is created
- silent `let _ = ...emit(...)` discards fail immediately in non-test Rust
- unchanged pre-existing `80+` RULE-002 violations do not block the pre-phase
  gate, but any new diff-overlapping `80+` violation fails
- Phase `X` planning docs treat these gates as pre-phase prerequisites, not
  internal `integrate/phase-X` sprint work

## Required Validation

- run the local lint entrypoint that exercises both new gates
- run direct script invocation to prove both gates work in isolation
- `git diff --check`
