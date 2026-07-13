---
id: SCLINT.12
title: Feature Enablement And Delta Capture
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 12 — Feature Enablement And Delta Capture

## Goal

Enable all adopted released `sc-lint` features in ATM and capture the resulting
finding delta without suppressing warnings or disabling rules.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-11.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `Justfile`
- `.github/workflows/ci.yml`
- `scripts/validate_release.py`
- `.just/`
- `crates/`
- `docs/plans/sc-lint-migration/gap-register.md`
- `docs/plans/sc-lint-migration/sprint-12.md`

## Deliverables

- all adopted released `sc-lint` features are enabled in the ATM validation
  path
- the default posture is:
  ```text
  enable released sc-lint features -> record ATM delta -> no suppressions -> route blockers explicitly
  ```
- every newly exposed ATM finding is classified as either:
  - non-architectural cleanup to close in `sprint-13.md`
  - architecture blocker to record in the gap register for `sprint-99.md`
- no finding is dismissed with a permanent warning disable or laissez-faire
  "existing issue" rationale

## Acceptance Criteria

- no adopted released `sc-lint` feature is left disabled merely to make the
  migration pass
- no new suppression path is introduced as a substitute for classification
- every exposed finding is classified before `sprint-12.md` can close
- every unresolved architecture blocker is explicitly recorded for the final
  review sprint before `sprint-12.md` can close

## Paths To Delete

- any temporary warning suppressions or feature-disable paths introduced only
  to bypass released `sc-lint` findings

## Required Validation

- `rg -n "allow\\(|deny\\(|warn\\(|sc-lint|feature" Justfile .github/workflows/ci.yml scripts/validate_release.py .just crates docs/plans/sc-lint-migration/gap-register.md`
- `git diff --check`
