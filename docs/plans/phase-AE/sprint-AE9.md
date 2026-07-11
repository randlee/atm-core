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
- `scripts/validate_release.py`
- `reports/smoke/phase-AE-installed-docs-proof.md`

## Deliverables

- `docs/plans/phase-AE/readiness.md` records
  `reports/smoke/phase-AE-installed-docs-proof.md` as the AE.9 closure
  artifact for the accepted line
- one accepted-line artifact proves:
  - the installed archive/output contains `share/doc/atm/`
  - the copied corpus matches the repo-owned source tree
  - the copied corpus still passes link and fenced-example validation
- the accepted-line proof artifact path is fixed as
  `reports/smoke/phase-AE-installed-docs-proof.md`
- release notes authored in `AE.5` are re-verified here for installed-doc
  location/scope, but not re-authored

## Acceptance Criteria

- this sprint is the only phase-close source of truth for installed-doc proof
- the final artifact names the exact corpus files verified
- the final artifact records the release version reviewed
- the final artifact records whether `release/release-notes.md` still names the
  installed doc location
- `docs/plans/phase-AE/readiness.md` points at the same
  `reports/smoke/phase-AE-installed-docs-proof.md` artifact named by this
  sprint

## Required Validation

- `just test`
- `python3 scripts/validate_release.py validate --proof-output reports/smoke/phase-AE-installed-docs-proof.md`
- `rg -n "phase-AE-installed-docs-proof.md" docs/plans/phase-AE/readiness.md`
- `python3 -c "from pathlib import Path; assert Path('reports/smoke/phase-AE-installed-docs-proof.md').is_file()"`
- `python3 -c "from pathlib import Path; text = Path('reports/smoke/phase-AE-installed-docs-proof.md').read_text(); assert 'share/doc/atm' in text and 'README.md' in text and 'release version' in text and 'release/release-notes.md' in text"`
- `git diff --check`
