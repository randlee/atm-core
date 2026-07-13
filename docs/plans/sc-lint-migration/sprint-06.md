---
id: SCLINT.06
title: Runtime Waits Surface Resolution
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: develop
---

# Sprint 06 — Runtime Waits Surface Resolution

## Goal

Resolve `runtime-waits` against the released runtime product surface, using a
temporary ATM shim only if direct native `sc-lint` selection is not possible.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-02.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `.just/lint_runtime_waits.py`
- `.just/tests/test_lint_runtime_waits.py`
- `.just/run_lint.py`
- `Justfile`
- `docs/plans/sc-lint-migration/gap-register.md`
- `docs/plans/sc-lint-migration/sprint-06.md`

## Deliverables

- one of these two outcomes is chosen explicitly:
  - direct published-tool selection replaces `.just/lint_runtime_waits.py`
  - a minimal temporary shim remains because one named published-surface gap
    blocks deletion
- if a shim remains, the gap register records:
  - the missing `sc-lint-runtime` capability
  - the upstream owner
  - the deletion trigger
- `.just/tests/test_lint_runtime_waits.py` matches the chosen
  direct-or-shimmed contract

## Acceptance Criteria

- `runtime-waits` is not left in an ambiguous "keep for now" state
- if the wrapper survives, it is minimal and tied to one named upstream gap
- if the wrapper is deleted, no stale path reference remains

## Paths To Delete

- `.just/lint_runtime_waits.py` when direct native `sc-lint-runtime` usage is
  proven

## Required Validation

- `rg -n "runtime-waits|SCB-RUNTIME-001|SCB-RUNTIME-002" docs/plans/sc-lint-migration/gap-register.md .just/tests/test_lint_runtime_waits.py Justfile .just/run_lint.py`
- `git diff --check`
