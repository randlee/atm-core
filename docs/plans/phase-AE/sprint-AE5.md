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
- `scripts/validate_release.py`
- `.github/workflows/release.yml`
- `target/phase-ae/staged-install-root/`
- `release/release-notes.md`

## Deliverables

- release/archive assembly includes `share/doc/atm/` populated from
  `docs/user-documents/`
- the installed primary entrypoint is `share/doc/atm/README.md`
- packaging exposes one deterministic validation root at
  `target/phase-ae/staged-install-root/`; downstream sprints must reuse that
  path instead of inventing new placeholders
- the binary/doc relationship is fixed as:
  - installed binary: `<install-root>/bin/atm`
  - installed doc root: `<install-root>/share/doc/atm/`
  - installed doc entrypoint resolved from the binary as
    `../share/doc/atm/README.md`
- `target/phase-ae/staged-install-root/share/doc/atm/README.md` exists after
  the packaging stage command runs
- local install flow and release archive verification both check only for the
  installed user-doc tree and the `README.md` entrypoint; content/link/example
  validation is explicitly deferred to `AE.7`
- release notes document the installed doc location as part of the public
  install surface; later sprints may verify that wording but must not redefine
  ownership for it

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
- `AE.5` is the only sprint allowed to define the deterministic staged install
  root path used by later verification work
- `AE.5` is the only sprint allowed to author the release-note statement that
  explains where installed user docs land

## Required Validation

- `python3 scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml`
- `python3 scripts/release_artifacts.py stage-install-docs --manifest release/publish-artifacts.toml --output-root target/phase-ae/staged-install-root`
- `python3 scripts/validate_release.py manifest --staged-install-root target/phase-ae/staged-install-root`
- `python3 -c "from pathlib import Path; assert Path('target/phase-ae/staged-install-root/share/doc/atm/README.md').is_file()"`
- `git diff --check`
