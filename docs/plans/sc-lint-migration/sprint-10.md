---
id: SCLINT.10
title: Vendored Workspace Removal
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 10 — Vendored Workspace Removal

## Goal

Delete the vendored sc-lint crates from the ATM workspace once direct or
justified replacement surfaces are already proven.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-03.md`
- `docs/plans/sc-lint-migration/sprint-04.md`
- `docs/plans/sc-lint-migration/sprint-05.md`
- `docs/plans/sc-lint-migration/sprint-06.md`
- `docs/plans/sc-lint-migration/sprint-09.md`

## Exact Targets

- `Cargo.toml`
- `Cargo.lock`
- `crates/sc-lint-directives/`
- `crates/sc-lint-attributes/`
- `crates/sc-lint-boundary/`
- `docs/plans/sc-lint-migration/sprint-10.md`

## Deliverables

- the vendored workspace members are deleted:
  - `crates/sc-lint-directives/`
  - `crates/sc-lint-attributes/`
  - `crates/sc-lint-boundary/`
- the root workspace and lockfile no longer reference those vendored members
- any test, helper, or doc assumptions about those directories are removed

## Acceptance Criteria

- no ATM workspace path dependency points at a vendored `sc-lint-*` crate
- no deleted vendored crate directory is still referenced by `Cargo.toml`,
  scripts, tests, CI, or docs

## Paths To Delete

- `crates/sc-lint-directives/`
- `crates/sc-lint-attributes/`
- `crates/sc-lint-boundary/`

## Required Validation

- `rg -n "sc-lint-directives|sc-lint-attributes|sc-lint-boundary" Cargo.toml Cargo.lock .just .github scripts docs || true`
- `cargo build --workspace`
- `git diff --check`
