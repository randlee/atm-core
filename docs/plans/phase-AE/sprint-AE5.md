---
id: AE.5
title: Installed Copy Packaging
status: planned
branch: feature/pAE-s5-installed-copy-packaging
worktree: ../atm-core-worktrees/feature/pAE-s5-installed-copy-packaging
target: integrate/phase-AE
---

# Sprint AE.5 — Installed Copy Packaging

## Goal

Make the end-user document corpus ship in local-install and release outputs.

## Hard Dependencies

- `AE.4` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `release/publish-artifacts.toml`
- `scripts/release_artifacts.py`
- `scripts/verify_release_archive.py`
- `scripts/validate_release.py`
- `.github/workflows/release.yml`
- `release/release-notes.md`

## Deliverables

- release/archive assembly includes `share/doc/atm/` populated from
  `docs/user-documents/`
- local install flow and release archive verification both check for the user
  doc tree
- release notes document the installed doc location as part of the public
  install surface

## Acceptance Criteria

- no release artifact can pass archive verification while omitting the user-doc
  tree
- the install-copy step preserves relative paths exactly as authored in
  `docs/user-documents/`
- runtime state under `~/.atm/` is not confused with the installed doc tree

## Required Validation

- `python3 scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml`
- `python3 scripts/validate_release.py manifest`
- `git diff --check`
