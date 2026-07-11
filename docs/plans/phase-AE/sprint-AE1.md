---
id: AE.1
title: User-Doc Contract And Source Tree Baseline
status: planned
branch: feature/pAE-s1-user-doc-contract-and-source-tree
worktree: ../atm-core-worktrees/feature/pAE-s1-user-doc-contract-and-source-tree
target: integrate/phase-AE
---

# Sprint AE.1 — User-Doc Contract And Source Tree Baseline

## Goal

Define the authoritative installed user-doc contract so later content and
packaging sprints do not guess at file layout, metadata, or validation shape.

## Hard Dependencies

- `docs/plans/phase-AE/plan-phase-AE.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/documentation-guidelines.md`

## Exact Targets

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/documentation-guidelines.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/commands/help.md`
- `docs/adr/ADR-025-installed-user-documentation-surface.md`
- `docs/adr/INDEX.md`
- `docs/project-plan.md`
- `docs/plans/phase-AE/plan-phase-AE.md`
- `docs/plans/phase-AE/issues.md`
- `docs/plans/phase-AE/readiness.md`
- `docs/plans/phase-AE/sprint-AE1.md`

## Deliverables

- one canonical repo-owned end-user documentation source tree is defined as
  `docs/user-documents/`
- the required corpus inventory is fixed as:
  - `README.md`
  - `install-layout.md`
  - `quickstart.md`
  - `identity-and-team.md`
  - `mailbox-workflows.md`
  - `doctor-and-log.md`
  - `hooks.md`
  - `nudge-templates.md`
  - `troubleshooting.md`
- the required metadata header is fixed as:
  ```yaml
  ---
  title: <document title>
  audience: end-user
  reviewed_for_release: 1.3.0
  ---
  ```
- the install destination is fixed as `<install-root>/share/doc/atm/`
- the contract distinguishes install-tree docs from runtime-state data under
  `~/.atm/`
- the contract requires relative links only
- the contract requires mechanically valid fenced `json`, `xml`, `toml`, and
  `bash` examples
- `docs/project-plan.md` registers Phase `AE` as the active installed
  user-documentation planning line and points to `plan/phase-AE` /
  `integrate/phase-AE`

## Acceptance Criteria

- no top-level requirement, architecture, or help doc still treats long-form
  operator help as repo-only or ad hoc prose
- no document leaves the install destination or metadata header open-ended
- the new ADR states why long-form help lives in installed markdown rather than
  new help-only commands
- the project plan includes a Phase `AE` planning note consistent with this
  sprint's branch and integration-branch contract

## Required Validation

- `rg -n "docs/user-documents|share/doc/atm|reviewed_for_release" docs/requirements.md docs/architecture.md docs/documentation-guidelines.md docs/atm/requirements.md docs/atm/architecture.md docs/atm/commands/help.md docs/adr/ADR-025-installed-user-documentation-surface.md`
- `rg -n "Phase-AE planning note|plan/phase-AE|integrate/phase-AE" docs/project-plan.md`
- `git diff --check`
