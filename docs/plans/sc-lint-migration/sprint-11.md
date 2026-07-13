---
id: SCLINT.11
title: CI And Release Preflight Retarget
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: develop
---

# Sprint 11 — CI And Release Preflight Retarget

## Goal

Retarget CI and release-preflight so ATM validates only against the published
`sc-lint` install path.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-07.md`
- `docs/plans/sc-lint-migration/sprint-10.md`

## Exact Targets

- `.github/workflows/ci.yml`
- `scripts/validate_release.py`
- `Justfile`
- `docs/plans/sc-lint-migration/sprint-11.md`

## Deliverables

- CI installs released `sc-lint` on:
  - `ubuntu-latest`
  - `macos-latest`
  - `windows-latest`
- release-preflight validates against the same published install path
- no CI or release path assumes a sibling checkout or vendored workspace build

## Acceptance Criteria

- `just lint` passes in CI using only the published-tool installation path
- release-preflight no longer assumes analyzer crates live in the ATM workspace
- Windows installation is treated as first-class, not an afterthought

## Paths To Delete

- any CI or preflight logic that still assumes vendored analyzer crates or an
  ad hoc sibling checkout

## Required Validation

- `rg -n "sc-lint|cargo install|binstall|windows-latest|validate_release" .github/workflows/ci.yml scripts/validate_release.py Justfile`
- `git diff --check`
