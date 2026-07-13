---
id: SCLINT.05
title: Unix Gating Surface Resolution
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 05 — Unix Gating Surface Resolution

## Goal

Resolve `unix-gating` against the released portability product surface, using a
temporary ATM shim only if direct native `sc-lint` selection is not possible.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-02.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `.just/lint_unix_gating.py`
- `.just/tests/test_lint_unix_gating.py`
- `.just/run_lint.py`
- `Justfile`
- `docs/plans/sc-lint-migration/gap-register.md`
- `docs/plans/sc-lint-migration/sprint-05.md`

## Deliverables

- one of these two outcomes is chosen explicitly:
  - direct published-tool selection replaces `.just/lint_unix_gating.py`
  - a minimal temporary shim remains because one named published-surface gap
    blocks deletion
- if a shim remains, the gap register records:
  - the missing `sc-lint-portability` capability
  - the upstream owner
  - the deletion trigger
- `.just/tests/test_lint_unix_gating.py` matches the chosen direct-or-shimmed
  contract

## Acceptance Criteria

- `unix-gating` is not left in an ambiguous "keep for now" state
- if the wrapper survives, it is minimal and tied to one named upstream gap
- if the wrapper is deleted, no stale path reference remains

## Paths To Delete

- `.just/lint_unix_gating.py` when direct native `sc-lint-portability` usage
  is proven

## Required Validation

- Execution-branch proof, chosen by outcome:
  - direct native path:
    - `test ! -e .just/lint_unix_gating.py`
    - `! rg -n 'lint_unix_gating.py' Justfile .just/run_lint.py .just/tests/test_lint_unix_gating.py scripts/validate_release.py .github/workflows/ci.yml`
  - temporary shim path:
    - `test -e .just/lint_unix_gating.py`
    - `rg -n 'unix-gating|PORT-004|PORT-005|Deletion trigger|deletion trigger' docs/plans/sc-lint-migration/gap-register.md`
- Planning-time limitation:
  - this planning branch cannot run the final `just lint unix-gating` path because Sprint 05 explicitly allows one of two execution outcomes; the execution sprint must run `just lint unix-gating` against the chosen outcome before closure
- `git diff --check`
