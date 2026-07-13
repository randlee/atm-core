---
id: SCLINT.06
title: Runtime Waits Surface Resolution
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
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

- Execution-branch proof, chosen by outcome:
  - direct native path:
    - `test ! -e .just/lint_runtime_waits.py`
    - `! rg -n 'lint_runtime_waits.py' Justfile .just/run_lint.py .just/tests/test_lint_runtime_waits.py scripts/validate_release.py .github/workflows/ci.yml`
  - temporary shim path:
    - `test -e .just/lint_runtime_waits.py`
    - `rg -n 'runtime-waits|SCB-RUNTIME-001|SCB-RUNTIME-002|Deletion trigger|deletion trigger' docs/plans/sc-lint-migration/gap-register.md`
- Planning-time limitation:
  - this planning branch cannot run the final `just lint runtime-waits` path because Sprint 06 explicitly allows one of two execution outcomes; the execution sprint must run `just lint runtime-waits` against the chosen outcome before closure
- `git diff --check`
