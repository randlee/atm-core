---
id: SCLINT.07
title: Lint Orchestration Surface Reduction
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 07 — Lint Orchestration Surface Reduction

## Goal

Reduce or delete `.just/run_lint.py` while preserving the current ATM
user-facing lint entrypoints and failure gating.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-03.md`
- `docs/plans/sc-lint-migration/sprint-04.md`
- `docs/plans/sc-lint-migration/sprint-05.md`
- `docs/plans/sc-lint-migration/sprint-06.md`

## Exact Targets

- `.just/run_lint.py`
- `.just/tests/test_run_lint.py`
- `Justfile`
- `scripts/validate_release.py`
- `docs/plans/sc-lint-migration/sprint-07.md`

## Deliverables

- `.just/run_lint.py` is either deleted or reduced to the smallest temporary
  compatibility shim that still preserves:
  - `just lint`
  - `just lint fast`
  - manual lint target names
- the preserved user-facing contract remains:
  ```bash
  just lint
  just lint fast
  just lint sc-boundary
  just lint sc-portability
  just lint unix-gating
  just lint runtime-waits
  ```
- `.just/tests/test_run_lint.py` protects the surviving contract only
- `scripts/validate_release.py` references the surviving lint orchestration
  surface, not deleted implementation details

## Acceptance Criteria

- no orchestrator code survives merely because it existed before migration
- if `.just/run_lint.py` survives, the plan and code both state why native
  `Justfile`/`sc-lint` wiring is still insufficient
- no stale reference to deleted wrappers remains inside the orchestrator or
  release-preflight path

## Paths To Delete

- `.just/run_lint.py` when direct `Justfile`/native `sc-lint` wiring can carry
  the full ATM lint surface

## Required Validation

- Execution-branch proof:
  - `rg -n 'just lint|just lint fast|just lint sc-boundary|just lint sc-portability|just lint unix-gating|just lint runtime-waits' Justfile .just/tests/test_run_lint.py scripts/validate_release.py`
  - `if [ -e .just/run_lint.py ]; then rg -n 'compatibility shim|consumer-surface gap|deletion trigger' docs/plans/sc-lint-migration/gap-register.md && rg -n 'just lint fast|sc-boundary|sc-portability|unix-gating|runtime-waits' .just/run_lint.py .just/tests/test_run_lint.py; else ! rg -n 'run_lint.py' Justfile .just/tests/test_run_lint.py scripts/validate_release.py .github/workflows/ci.yml; fi`
- Planning-time limitation:
  - this planning branch cannot prove the final orchestrator behavior because the surviving direct-vs-shimmed implementation is an execution outcome; the execution sprint must run `just lint` and `just lint fast` before closure
- `git diff --check`
