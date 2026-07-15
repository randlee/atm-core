---
id: SCLINT.04
title: Direct Portability Tool Migration
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 04 — Direct Portability Tool Migration

## Goal

Delete the thin ATM portability wrapper and move the `sc-portability` surface
to direct released `sc-lint-portability` usage.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-02.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `.just/lint_sc_portability.py`
- `.just/tests/test_lint_sc_portability.py`
- `.just/run_lint.py`
- `Justfile`
- `docs/plans/sc-lint-migration/sprint-04.md`

## Deliverables

- `.just/lint_sc_portability.py` is deleted
- the `sc-portability` lint surface invokes released `sc-lint-portability`
  directly
- the direct command contract is fixed as:
  ```bash
  sc-lint-portability analyze --root <repo-root> --format json
  ```
- `.just/tests/test_lint_sc_portability.py` is updated or deleted to match the
  new direct contract
- any command help or repo-local lint references that still mention the deleted
  wrapper path are updated

## Acceptance Criteria

- `sc-portability` no longer shells through a repo-local thin wrapper
- no deleted wrapper path remains referenced by `Justfile`, `.just/run_lint.py`,
  tests, or docs
- the user-facing `just lint sc-portability` surface still exists

## Paths To Delete

- `.just/lint_sc_portability.py`

## Required Validation

- Execution-branch proof:
  - `test ! -e .just/lint_sc_portability.py`
  - `! rg -n 'lint_sc_portability.py|cargo run -q -p sc-lint-boundary -- .*--rule portability' Justfile .just/run_lint.py .just/tests/test_lint_sc_portability.py scripts/validate_release.py .github/workflows/ci.yml`
- Planning-time limitation:
  - this planning branch cannot run `just lint sc-portability` against the future published-tool wiring because that command surface does not exist until the execution branch lands it; the execution sprint must run `just lint sc-portability` before closure
- `git diff --check`
