---
id: AE.9
title: Phase-End Installed-Docs Proof
status: planned
branch: feature/pAE-s9-phase-end-installed-docs-proof
worktree: ../atm-core-worktrees/feature/pAE-s9-phase-end-installed-docs-proof
target: integrate/phase-AE
---

# Sprint AE.9 — Phase-End Installed-Docs Proof

## Goal

Produce the final release-facing evidence that the installed user-doc corpus
ships and validates on the accepted line.

## Hard Dependencies

- `AE.8` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `docs/plans/phase-AE/readiness.md`
- `release/release-notes.md`
- `reports/smoke/`

## Deliverables

- one accepted-line artifact proves:
  - the installed archive/output contains `share/doc/atm/`
  - the copied corpus matches the repo-owned source tree
  - the copied corpus still passes link and fenced-example validation
- release notes mention the installed user-doc location and scope

## Acceptance Criteria

- this sprint is the only phase-close source of truth for installed-doc proof
- the final artifact names the exact corpus files verified
- the final artifact records the release version reviewed

## Required Validation

- `just test`
- `python3 scripts/validate_release.py validate`
- `git diff --check`
