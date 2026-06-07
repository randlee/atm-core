# Phase Yb Hardening Audit

## Status

HARDENING COMPLETE

- Iterations: `3`
- Total findings resolved: `20`
- Final finding count: `0`
- Dev-agent decision points eliminated: `14`

## Scope

This audit covers the full Phase `Yb` planning set:

- `docs/project-plan.md`
- `docs/plans/phase-Yb/plan-phase-Yb.md`
- `docs/plans/phase-Yb/sprint-Y7.md`
- `docs/plans/phase-Yb/sprint-Y8.md`
- `docs/plans/phase-Yb/sprint-Y9.md`
- `docs/plans/phase-Yb/sprint-Y10.md`
- `docs/plans/phase-Yb/removal-ledger.md`
- `docs/plans/phase-Yb/message-path-call-stacks.md`
- `docs/plans/phase-Yb/lintable-boundary-plan.md`
- `docs/plans/phase-Yb/qa-handoff.md`
- `docs/plans/phase-Yb/testing-and-validation.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/boundaries.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/adr/INDEX.md`
- `boundaries/atm-core/non-claude-outbound.toml`
- `boundaries/atm-daemon/daemon-non-claude-outbound.toml`

## Iteration 1 Findings

| # | Document / Section | Type | Problem | Resolution |
| --- | --- | --- | --- | --- |
| 1 | `docs/plans/phase-Yb/sprint-Y7.md` through `sprint-Y10.md` / dependency ordering | `ORDERING` | The sprint docs did not state the hard dependency chain `Y.7 -> Y.8 -> Y.9 -> Y.10`. | Added `Hard Dependencies` sections to every sprint doc and reflected the same order in `plan-phase-Yb.md`. |
| 2 | `docs/plans/phase-Yb/plan-phase-Yb.md`, `ADR-013` | `UNDEF` | `DeliveryPlan` and `ReplyDeliveryPlan` were referenced but not explicitly defined or assigned to a module. | Defined the exact module and type ownership in `plan-phase-Yb.md`, `ADR-013`, `requirements.md`, and crate docs. |
| 3 | `docs/plans/phase-Yb/sprint-Y7.md` through `sprint-Y10.md` | `GAP` | The sprint docs had no `Required Validation` sections. | Added explicit command-based validation to every sprint doc and a shared matrix in `testing-and-validation.md`. |
| 4 | `docs/plans/phase-Yb/sprint-Y7.md` through `sprint-Y10.md` | `VAGUE` | Acceptance criteria were too prose-oriented for `req-qa` to verify mechanically. | Rewrote sprint acceptance criteria so grep/trace/test evidence is named directly. |
| 5 | `docs/plans/phase-Yb/lintable-boundary-plan.md` | `BOUNDARY` | The lint plan did not classify enforcement as compile-time, lint-time, or runtime, or name the enforcement points. | Added enforcement classes and named the exact TOML, `sc-lint`, and runtime fail-closed hooks. |
| 6 | `boundaries/` / non-Claude outbound seam | `GAP` | The plan introduced a dedicated non-Claude outbound boundary without machine-readable boundary definitions. | Added `boundaries/atm-core/non-claude-outbound.toml` and `boundaries/atm-daemon/daemon-non-claude-outbound.toml`, then referenced them from the boundary docs. |
| 7 | `docs/adr/INDEX.md`, crate architecture/requirements docs | `CROSS-DOC` | ADR index and crate docs did not reflect the exact Yb corrective modules and boundaries. | Updated ADR index, `docs/atm-core/*`, and `docs/atm-daemon/*` to match the Yb plan. |
| 8 | Phase Yb QA/process coverage | `GAP` | The planning set had no dedicated Yb QA handoff or validation matrix even though the sprint docs depend on mechanical review gates. | Added `qa-handoff.md` and `testing-and-validation.md`, and linked them from the phase plan and project plan. |

## Iteration 2 Supplemental Findings

Supplemental QA findings on the pre-hardening push required one more pass:

1. add row ownership to `removal-ledger.md` and cite row IDs from sprint docs
2. resplit `Y.8` and `Y.9` per the explicit ruling:
   - `Y.8` removes Claude-specific and generic outer policy leakage
   - `Y.9` introduces the non-Claude outbound boundary and deletes old
     non-Claude routing surfaces atomically
3. add `sprint-Y10.md` to ADR-013 required follow-on
4. assign executor-type introduction sprints
5. assign `AckReplyStateMachine` introduction sprint
6. clarify `Y.8` prose-rule timeline versus `Y.10` machine-enforced timeline
7. remove `docs/project-plan.md` from `Y.7` document-update scope
8. add `ADR-013` to the Section G artifact list in `plan-phase-Yb.md`
9. add `docs/plans/phase-Yb/message-path-call-stacks.md` to `Y.8` governing
   requirements

## Iteration 3 Result

Third audit pass found:

- no missing sprint docs
- no undefined Yb-specific plan types or boundaries
- no missing dependency ordering
- no missing Yb QA or validation artifact
- no remaining cross-document contradiction in the Yb planning set

## Documents Modified

- `docs/project-plan.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/boundaries.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/adr/INDEX.md`
- `docs/plans/phase-Yb/plan-phase-Yb.md`
- `docs/plans/phase-Yb/sprint-Y7.md`
- `docs/plans/phase-Yb/sprint-Y8.md`
- `docs/plans/phase-Yb/sprint-Y9.md`
- `docs/plans/phase-Yb/sprint-Y10.md`
- `docs/plans/phase-Yb/removal-ledger.md`
- `docs/plans/phase-Yb/message-path-call-stacks.md`
- `docs/plans/phase-Yb/lintable-boundary-plan.md`

## Documents Created

- `docs/plans/phase-Yb/hardening-audit.md`
- `docs/plans/phase-Yb/qa-handoff.md`
- `docs/plans/phase-Yb/testing-and-validation.md`
- `boundaries/atm-core/non-claude-outbound.toml`
- `boundaries/atm-daemon/daemon-non-claude-outbound.toml`
