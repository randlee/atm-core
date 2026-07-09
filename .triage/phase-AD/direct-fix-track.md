# Phase AD Direct-Fix Track

This ledger exists because several phase-end review findings were closure- or
artifact-oriented rather than new implementation-sprint work. `AD.30` owns
keeping this ledger current and tying each item to the final
`docs/plans/phase-AD/readiness.md` verdict.

## Carry-Forward Items

| Item | Technical owner | Closure-artifact owner | Tracking / closure artifact |
| --- | --- | --- | --- |
| `AD9-BLANKPANE-001` | `AD.9` | `AD.30` | cite accepted-line evidence that closes `docs/plans/phase-AD/sprint-AD9.md` Acceptance Criteria `the validated-on-entry blank tmux_pane_id drift for team-lead and arch-ctm is repaired on the accepted baseline` |
| `ERRDOC-001` | `AD.9` | `AD.30` | cite accepted-line evidence that closes the `docs/plans/phase-AD/sprint-AD9.md` CLI Error Contract entries for `ATM_MEMBER_ALREADY_EXISTS`, `ATM_MEMBER_NOT_FOUND`, `ATM_IDENTITY_UNAVAILABLE`, `ATM_TEAM_UNAVAILABLE`, `ATM_IDENTITY_INVALID`, and `ATM_TEAM_INVALID` |
| historical `FTQ-001` env-race reconciliation | accepted-line code fix predates this follow-up | `AD.30` | either update `.triage/phase-Xb/findings/FTQ-001.ttl` to closed or record the explicit historical-provenance reason for leaving it open in `docs/plans/phase-AD/readiness.md` |
| phase-AD triage sweep ledger | `AD.30` | `AD.30` | expand this directory with the final sweep disposition before phase closure |
| `CHANGELOG.md` entry for the `AD.13` through `AD.30` corrective line | `AD.30` | `AD.30` | landed `CHANGELOG.md` text plus citation in `docs/plans/phase-AD/readiness.md` |

## Notes

- This file is intentionally phase-local. It does not replace the original
  historical finding ledgers from earlier phases; it records how the accepted
  `Phase AD` closeout line must reconcile those historical records.
- If a carry-forward item turns into code work after review, the owning sprint
  must be named here before implementation starts.
