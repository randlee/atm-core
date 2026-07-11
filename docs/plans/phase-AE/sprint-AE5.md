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
- `scripts/verify_user_docs.py`
- `scripts/verify_release_archive.py`
- `scripts/validate_release.py`
- `.github/workflows/release.yml`
- `release/release-notes.md`

## Deliverables

- release/archive assembly includes `share/doc/atm/` populated from
  `docs/user-documents/`
- the installed primary entrypoint is `share/doc/atm/README.md`
- the binary/doc relationship is fixed as:
  - installed binary: `<install-root>/bin/atm`
  - installed doc root: `<install-root>/share/doc/atm/`
  - installed doc entrypoint resolved from the binary as
    `../share/doc/atm/README.md`
- local install flow and release archive verification both check for the user
  doc tree and the `README.md` entrypoint
- release notes document the installed doc location as part of the public
  install surface

## Acceptance Criteria

- no release artifact can pass archive verification while omitting the user-doc
  tree
- no release artifact can pass archive verification while omitting
  `share/doc/atm/README.md`
- the install-copy step preserves relative paths exactly as authored in
  `docs/user-documents/`
- runtime state under `~/.atm/` is not confused with the installed doc tree
- `ATM_HOME` is not used as the install-doc locator in packaging or archive
  verification logic

## Required Validation

- `python3 scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml`
- `python3 scripts/verify_user_docs.py --source-root docs/user-documents --installed-root '<staged-install-root>/share/doc/atm'`
- `python3 scripts/validate_release.py manifest`
- `git diff --check`
