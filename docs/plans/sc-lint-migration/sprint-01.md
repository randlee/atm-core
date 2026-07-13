---
id: SCLINT.01
title: Published Release Target And Gap Register
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 01 — Published Release Target And Gap Register

## Goal

Freeze the exact released `sc-lint` version and installation path ATM will
target, then capture the known capability gaps before any deletion starts.

## Hard Dependencies

- `docs/plans/sc-lint-migration/plan.md`

## Exact Targets

- `docs/plans/sc-lint-migration/sprint-01.md`
- `docs/plans/sc-lint-migration/gap-register.md`
- `.github/workflows/ci.yml`
- `scripts/validate_release.py`
- `Justfile`
- `.just/run_lint.py`

## Deliverables

- one exact released `sc-lint` version is named for the migration branch
- one exact install method is chosen for:
  - `ubuntu-latest`
  - `macos-latest`
  - `windows-latest`
- `docs/plans/sc-lint-migration/plan.md` is updated only as needed to keep the
  top-level planning summary and sprint-package index aligned with the pinned
  release / gap-register contract
- `docs/plans/sc-lint-migration/gap-register.md` exists and records:
  - the pinned release version
  - the chosen install method
  - known parity matches already proven
  - known gaps already visible before implementation
  - the exact temporary workaround allowed for each known gap, if any
- the initial known-gap list explicitly includes at least:
  - `unix_path_prefixes` portability-config parity
  - JSON output / machine-contract parity required by ATM lint surfaces
  - rule-ID continuity expectations for `PORT-004`, `PORT-005`,
    `SCB-RUNTIME-001`, and `SCB-RUNTIME-002`
  - all-platform installation coverage, especially Windows

## Acceptance Criteria

- no implementation sprint starts before the release version and install method
  are frozen in the gap register
- no known pre-existing gap is left implicit or buried only in prose
- every listed workaround points to a specific missing released capability, not
  a convenience preference for keeping Python
- the plan and gap register state that native `sc-lint` usage is the target
  end state for every adopted lint surface

## Paths To Delete

- none

## Required Validation

- `! rg -n 'released \`sc-lint\` version: \`TBD\`|Linux: \`TBD\`|macOS: \`TBD\`|Windows: \`TBD\`' docs/plans/sc-lint-migration/gap-register.md`
- `rg -n '^## Pinned Release$|^## Initial Known Gaps To Review$|SCLINT-GAP-001|SCLINT-GAP-002|SCLINT-GAP-003|SCLINT-GAP-004' docs/plans/sc-lint-migration/gap-register.md`
- `rg -n 'native sc-lint usage is the target end state|sprint-01.md|sprint-99.md' docs/plans/sc-lint-migration/plan.md`
- `git diff --check`
