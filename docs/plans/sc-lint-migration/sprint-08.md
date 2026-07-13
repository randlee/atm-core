---
id: SCLINT.08
title: Boundary Governance Ownership Reduction
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: develop
---

# Sprint 08 — Boundary Governance Ownership Reduction

## Goal

Remove duplicated ATM-local dependency-policy and boundary-governance checks
where released `sc-lint` already owns the same enforcement.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-02.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `.just/lint_boundaries.py`
- `boundaries/**/*.toml`
- `.just/tests/test_lint_boundaries.py`
- `docs/plans/sc-lint-migration/gap-register.md`
- `docs/plans/sc-lint-migration/sprint-08.md`

## Deliverables

- each major `.just/lint_boundaries.py` responsibility is classified as:
  - move to released `sc-lint`
  - remain ATM-owned
  - temporary workaround pending one named upstream gap
- any dependency-policy enforcement already covered by released `sc-lint` is
  deleted from ATM-local code
- residual ATM-owned boundary checks are limited to ATM-specific governance
  that released `sc-lint` does not own

## Acceptance Criteria

- no duplicated dependency-policy logic remains without a written reason
- no residual ATM-owned check is described only as "kept for safety"
- `.just/tests/test_lint_boundaries.py` protects only the checks that truly
  remain ATM-owned

## Paths To Delete

- any redundant code paths inside `.just/lint_boundaries.py` proven equivalent
  to released `sc-lint` enforcement

## Required Validation

- `rg -n "allowed_|forbidden_edges|dependency|boundary" .just/lint_boundaries.py .just/tests/test_lint_boundaries.py docs/plans/sc-lint-migration/gap-register.md`
- `git diff --check`
