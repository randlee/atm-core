---
id: SCLINT.02
title: Published Tool Parity Baseline And Gap Classification
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 02 — Published Tool Parity Baseline And Gap Classification

## Goal

Prove, on one no-delete branch, exactly where released `sc-lint` already
matches ATM needs and exactly where it does not.

## Hard Dependencies

- `docs/plans/sc-lint-migration/plan.md`
- `docs/plans/sc-lint-migration/sprint-01.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `docs/plans/sc-lint-migration/sprint-02.md`
- `docs/plans/sc-lint-migration/gap-register.md`
- `artifacts/sc-lint-migration/parity/`

## Deliverables

- side-by-side parity results are recorded for:
  - `sc-boundary`
  - `sc-portability`
  - `unix-gating`
  - `runtime-waits`
- each parity mismatch is classified into exactly one bucket:
  - ATM wiring bug
  - released `sc-lint` product gap
  - legitimate ATM consumer-specific behavior
- the parity artifact records the exact commands used for both the vendored and
  released surfaces
- the gap register is updated with every product gap discovered during parity

## Acceptance Criteria

- no later sprint assumes deletion for a surface whose parity classification is
  still unknown
- no parity mismatch is allowed to remain as uncategorized "difference"
- every product gap discovered during parity is added to the gap register
  before any dependent deletion sprint claims closure

## Paths To Delete

- none

## Required Validation

- `test -d artifacts/sc-lint-migration/parity`
- `rg -n '^## Gap Table Schema$|^## Initial Known Gaps To Review$|atm-wiring-bug|sc-lint-product-gap|atm-consumer-specific|sc-boundary|sc-portability|unix-gating|runtime-waits' docs/plans/sc-lint-migration/gap-register.md`
- `git diff --check`
