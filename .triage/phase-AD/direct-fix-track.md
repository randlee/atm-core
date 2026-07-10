# Phase AD Direct-Fix Track

This ledger exists because several phase-end review findings were closure- or
artifact-oriented rather than new implementation-sprint work. `AD.30` owns
keeping this ledger current and tying each item to the final
`docs/plans/phase-AD/readiness.md` verdict.

## Carry-Forward Items

| Item | Technical owner | Closure-artifact owner | Tracking / closure artifact |
| --- | --- | --- | --- |
| `AD9-BLANKPANE-001` | `AD.9` | `AD.30` | closed on accepted AD line by `team_admin::tests::update_member_repairs_blank_pane_ids_for_team_lead_and_arch_ctm_fixture` in `crates/atm-core/src/team_admin.rs`; this is the direct acceptance-criteria proof named by `docs/plans/phase-AD/sprint-AD9.md` |
| `ERRDOC-001` | `AD.9` | `AD.30` | closed on accepted AD line by the explicit `update_member` validation coverage in `crates/atm/src/commands/teams.rs` tests plus the documented CLI error contract in `docs/plans/phase-AD/sprint-AD9.md`; no unresolved ATM-member / ATM-identity / ATM-team error-code gap remains on this branch |
| historical `FTQ-001` env-race reconciliation | accepted-line code fix predates this follow-up | `AD.30` | historical provenance intentionally retained; `docs/plans/phase-AD/readiness.md` now records that the old discovery ledger remains as history while the accepted AD line carries the repaired runtime behavior |
| phase-AD triage sweep ledger | `AD.30` | `AD.30` | terminal-sprint findings are now recorded in `.triage/phase-AD/findings/AD30-QA-2.ttl`; prior AD findings remain in `.triage/phase-AD/findings/` as the accepted sweep ledger |
| `CHANGELOG.md` entry for the `AD.13` through `AD.30` corrective line | `AD.30` | `AD.30` | release-facing `AD.13` through `AD.30` changelog text is present on this branch in `CHANGELOG.md`; `docs/plans/phase-AD/readiness.md` remains the phase-close citation surface |

## Notes

- This file is intentionally phase-local. It does not replace the original
  historical finding ledgers from earlier phases; it records how the accepted
  `Phase AD` closeout line must reconcile those historical records.
- If a carry-forward item turns into code work after review, the owning sprint
  must be named here before implementation starts.
