---
id: SCLINT.03
title: Direct Boundary Tool Migration
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 03 — Direct Boundary Tool Migration

## Goal

Delete the thin ATM boundary wrapper and move the `sc-boundary` surface to
direct released `sc-lint-boundary` usage.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-02.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `.just/lint_sc_boundary.py`
- `.just/tests/test_lint_sc_boundary.py`
- `.just/run_lint.py`
- `Justfile`
- `docs/plans/sc-lint-migration/sprint-03.md`

## Deliverables

- `.just/lint_sc_boundary.py` is deleted
- the `sc-boundary` lint surface invokes released `sc-lint-boundary` directly
- the direct command contract is fixed as:
  ```bash
  sc-lint-boundary analyze --root <repo-root> --format json
  ```
- `.just/tests/test_lint_sc_boundary.py` is updated or deleted to match the
  new direct contract
- any command help or repo-local lint references that still mention the deleted
  wrapper path are updated

## Acceptance Criteria

- `sc-boundary` no longer shells through `cargo run -p sc-lint-boundary`
- no deleted wrapper path remains referenced by `Justfile`, `.just/run_lint.py`,
  tests, or docs
- the user-facing `just lint sc-boundary` surface still exists

## Paths To Delete

- `.just/lint_sc_boundary.py`

## Required Validation

- `rg -n "lint_sc_boundary.py|cargo run -q -p sc-lint-boundary" Justfile .just .github scripts docs || true`
- `git diff --check`
