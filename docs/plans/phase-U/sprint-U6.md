# Sprint U.6 — Provenance / Timing Field Reduction

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.6"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u6-provenance-field-reduction
branch: feature/pU-u6-provenance-field-reduction
status: complete
estimated_scope: S
```

## Goal

Justify or remove mailbox-row provenance/timing fields that are not clearly
part of the enduring mailbox contract.

## Scope Summary

This sprint reviews and simplifies weakly justified mailbox-row fields,
starting with `imported_from` and `recorded_at`.

Target direction:
- delete weak round-trip provenance fields such as `imported_from`
- if `recorded_at` remains, treat it as store-owned ingest timing for
  health/reporting rather than caller-supplied message data

## Governing Requirements

- `REQ-CORE-MAILBOX-001`
- `REQ-CORE-BOUNDARY-001`
- `REQ-P-RELIABILITY-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-MailStore`
- `BOUNDARY-InboxIngress`

## Prerequisites

- unified state and query cutover from `U.4` and `U.5` are complete

## Hard Dependencies

- `U.4`
- `U.5`

## Non-Goals

- task or roster redesign
- replay-state redesign
- message identity redesign

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Field-by-field justification review
   Development work:
   - review `imported_from` and `recorded_at`
   - identify exact product/runtime reason for each
   Required tests:
   - add or update tests only for fields that survive
   Required doc or boundary updates:
   - update store docs to reflect the final reduced field set

2. Delete weak fields
   Development work:
   - remove fields that exist only for weak round-trip convenience
   - simplify store/load/query code accordingly
   Required tests:
   - adjust store and health tests to the surviving fields
   Required doc or boundary updates:
   - update SQL diagrams and schema references

## Split Recommendation

Do not split. This sprint should be small and decisive.

## Acceptance Criteria

- each surviving provenance/timing field has one clear product reason to exist
- weak round-trip-only fields are removed
- docs and diagrams no longer describe deleted mailbox-row fields

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/project-plan.md`
- `docs/plans/phase-U/plan-phase-U.md`
- `docs/architecture.md`
- `docs/atm-rusqlite/architecture.md`
- `docs/atm-rusqlite/requirements.md` (update clauses for removed provenance fields)
- `docs/atm-rusqlite/query-diagrams.md`

## Risks And Watchouts

- do not keep fields because tests currently reference them
- do not remove a timing field without checking health/reporting paths first
