# AD.1 Published Tool Install Contract And Version Pin

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.1
worktree: ../atm-core-worktrees/feature/pAD-s1-published-tool-install-contract
branch: feature/pAD-s1-published-tool-install-contract
status: planned
estimated_scope: medium
```

## Goal

Freeze the first published `sc-lint` version ATM will consume and land one
repo-owned installation path for the published tool binaries on Linux, macOS,
and Windows.

## Scope Summary

This sprint closes only the published-tool install contract.

Production-ready commitment:
- the sprint must leave one reviewable version pin and one reviewable install
  path that local development, CI, and later wrapper sprints can all reuse
- no wrapper retargeting or proc-macro cutover may be smuggled into this sprint

Tool contract shape:

```toml
[sc_lint_release]
version = "<published version>"
install_method = "<cargo-install|cargo-binstall|release-archive>"
binaries = ["sc-lint-boundary", "sc-lint-portability", "sc-lint-runtime"]
```

Any equivalent single-source-of-truth record is acceptable if it exposes the
same facts directly from repo state.

## Prerequisites

- a published `sc-lint` release exists for the binaries ATM will use first
- `Phase AD` is the accepted planning line

## Out Of Scope

- no wrapper retargeting yet
- no proc-macro dependency change yet
- no vendored crate deletion yet

## Code And Document Targets

- repo-owned install contract doc or config record for the published tool pin
- local developer install path docs/scripts
- CI-consumable install path docs/scripts
- no `.just/lint_*.py` wrapper edits in this sprint

## Deliverables

- one repo-owned install path exists for the published analyzer binaries ATM
  will use during the migration:
  - `sc-lint-boundary`
  - `sc-lint-portability`
  - `sc-lint-runtime`
- one explicit version-pin source of truth exists in ATM for those published
  tool binaries
- Linux, macOS, and Windows install instructions are recorded in repo-owned
  docs or scripts rather than assumed from the sibling `../sc-lint` checkout
- the install path is proven without changing the current ATM wrapper behavior
  yet
- the sprint records whether the published install path comes from:
  - release binaries
  - `cargo install`
  - `cargo binstall`

## Required Work

- choose one published `sc-lint` version pin that every later sprint will use
- define one repo-owned install method for Linux, macOS, and Windows
- record the exact binaries included in that contract
- record the local and CI install steps without relying on `../sc-lint`
- leave wrapper behavior unchanged so this sprint closes only the install line

## Acceptance Criteria

- ATM can install the published `sc-lint` binaries on Linux, macOS, and
  Windows without using `../sc-lint`
- the selected version pin is explicit and reviewable in repo state
- each supported platform records a `--version` check for the installed
  binaries
- `AD.1` does not retarget `.just/lint_*.py` yet; existing ATM lint behavior
  remains authoritative until `AD.2`
- a reviewer can identify the exact install method and pinned version from repo
  state without consulting `../sc-lint`

## Required Validation

- published `sc-lint-boundary --version`
- published `sc-lint-portability --version`
- published `sc-lint-runtime --version`
- `git diff --check`
