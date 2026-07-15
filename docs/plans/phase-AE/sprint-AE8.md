---
id: AE.8
title: Publisher Freshness Gate
status: complete
branch: feature/pAE-s8-publisher-freshness-gate
worktree: ../atm-core-worktrees/feature/pAE-s8-publisher-freshness-gate
target: integrate/phase-AE
---

# Sprint AE.8 — Publisher Freshness Gate

## Goal

Make publisher/release preflight fail when the end-user corpus was not reviewed
for the release version.

## Hard Dependencies

- `AE.7` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `scripts/validate_release.py`
- `scripts/release_artifacts.py`
- `.github/workflows/release-preflight.yml`
- `release/publish-surface-scope.md`

## Deliverables

- `scripts/release_artifacts.py` exposes the release-version input needed by
  the freshness gate so user-doc review checks run against the same version
  contract as the publish surface
- `.github/workflows/release-preflight.yml` executes the freshness gate in CI
  so stale user docs fail before publish/release work proceeds
- every file in `docs/user-documents/` must carry:
  ```yaml
  reviewed_for_release: 1.3.0
  ```
  where the value matches the release version under validation
- publisher/release preflight fails closed when:
  - a user-doc file is missing the field
  - the field does not match the release version
  - a referenced linked document is missing
- the failure output clearly names the offending file and expected version
- `release/publish-surface-scope.md` records the user-doc freshness gate as a
  required publish-surface check, including the `reviewed_for_release` contract
  and the validation entrypoint used to enforce it

## Acceptance Criteria

- stale user docs are treated as a release blocker, not a warning
- the freshness gate reuses the verifier from `AE.7` instead of inventing a
  second partial validation path
- the publish-surface scope doc names the same freshness gate inputs and
  validation entrypoint implemented by this sprint
- `scripts/release_artifacts.py` and `.github/workflows/release-preflight.yml`
  name the same freshness-gate entrypoint and release-version contract

## Required Validation

- `python3 scripts/validate_release.py all --version 1.3.0 --staged-install-root target/phase-ae/staged-install-root`
- `rg -n "reviewed_for_release|release version|docs/user-documents" scripts/release_artifacts.py .github/workflows/release-preflight.yml`
- `rg -n "reviewed_for_release|docs/user-documents|validate_release.py all" release/publish-surface-scope.md`
- `git diff --check`
