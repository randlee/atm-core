---
id: SCLINT.13
title: Non-Architectural Finding Cleanup
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: develop
---

# Sprint 13 — Non-Architectural Finding Cleanup

## Goal

Clean every ATM finding exposed by the enabled released `sc-lint` feature set
unless the finding truly requires architecture changes.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-12.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `.just/`
- `crates/`
- `scripts/`
- `docs/plans/sc-lint-migration/gap-register.md`
- `docs/plans/sc-lint-migration/sprint-13.md`

## Deliverables

- every non-architectural finding exposed in `sprint-12.md` is fixed in ATM
- every finding left open is present in the gap register as an architecture
  blocker or upstream product gap
- no warning is left open merely because ATM already "works now"

## Acceptance Criteria

- no non-architectural finding remains open at sprint close
- no cleanup is deferred to `sprint-99.md` unless it truly requires
  architecture changes
- no new suppression or feature-disable path is used to fake closure

## Paths To Delete

- any temporary local suppression introduced during feature bring-up instead of
  real code cleanup

## Required Validation

- `rg -n "architecture blocker|upstream product gap|non-architectural" docs/plans/sc-lint-migration/gap-register.md docs/plans/sc-lint-migration/sprint-12.md docs/plans/sc-lint-migration/sprint-13.md`
- `git diff --check`
