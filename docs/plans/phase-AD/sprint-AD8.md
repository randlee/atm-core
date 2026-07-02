# AD.8 CI And Release-Preflight Published Tool Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.8
worktree: ../atm-core-worktrees/feature/pAD-s8-ci-release-published-tool-cutover
branch: feature/pAD-s8-ci-release-published-tool-cutover
status: planned
estimated_scope: medium
```

## Goal

Retarget CI and release-preflight to the published `sc-lint` install path
only.

## Scope Summary

This sprint closes only the automation cutover line.

Production-ready commitment:
- all supported CI platforms must install and use published `sc-lint` binaries
- release gating must use the same published install contract rather than a
  special vendored fallback

## Prerequisites

- `AD.7`

## Out Of Scope

- no dependency-policy Phase `D.1` adoption yet
- no new lint-target rename or local UX redesign

## Deliverables

- `.github/workflows/ci.yml` uses the published install path only
- `scripts/validate_release.py` uses the published install path only
- any stale docs or tests that still describe a vendored analyzer build path
  are updated or removed

## Acceptance Criteria

- CI can run ATM linting on Linux, macOS, and Windows using only the published
  install path
- release-preflight no longer depends on a workspace-built vendored analyzer
  path
- no automation path in ATM shells through `../sc-lint`

## Required Validation

- `python3 .just/run_lint.py all`
- `python3 scripts/validate_release.py`
- `rg -n '\\.\\./sc-lint|cargo run -q -p sc-lint-boundary|path = \"\\.\\./sc-lint-' .github/workflows .just scripts Justfile Cargo.toml crates -S`
- `git diff --check`
