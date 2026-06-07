# AA.11 Delete Pre-Production SQLite Compatibility Scaffolding

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.11
worktree: ../atm-core-worktrees/feature/pAA-s11-delete-sqlite-legacy-compat
branch: feature/pAA-s11-delete-sqlite-legacy-compat
status: complete
estimated_scope: small
```

## Goal

Remove SQLite compatibility scaffolding for abandoned pre-production schema
shapes that ATM 1.2 no longer intends to support.

## Scope Summary

This sprint is the SQLite cleanup complement to `AA.10`. The work is to delete
the historical `legacy_message_id` migration support and any related docs/tests
that still treat pre-production SQLite schema drift as supported 1.2 behavior,
unless a narrower user-approved compatibility exception is recorded first.

## Governing Sources

- `crates/atm-rusqlite/src/lib.rs`
- `docs/phase-U/removal-inventory.md`
- `docs/phase-Z/smoke-findings-review.md`
- `docs/adr/ADR-012-one-message-identity.md`

## Prerequisites

- `AA.10`

## Out Of Scope

- no current Claude inbox schema changes
- no daemon/runtime boundary changes beyond deleting obsolete SQLite
  compatibility paths
- no support restoration for historical mail DB shapes that never shipped as
  accepted production state

## Deliverables

- The `legacy_message_id` compatibility path is deleted from the active 1.2
  runtime line. No normal bootstrap or migration path accepts that abandoned
  pre-production shape.

- The active 1.2 SQLite bootstrap/migration contract is restated clearly:
  - what exact schema versions/shapes are supported
  - what is no longer supported
  - what repair path, if any, exists for abandoned pre-production DBs

- Tests and docs no longer claim that pre-production `legacy_message_id`
  identity shapes are part of normal runtime support.

- The retained `Phase U` removal inventory reference is justified directly in
  the sprint outputs as the canonical cross-phase ledger for deleting
  superseded storage/runtime scaffolding rather than treating it as a stray
  historical artifact mention.

## Split Recommendation

Keep this sprint small and mechanical. If deleting the compatibility code
reveals a broader durable-store redesign need, stop and plan that separately.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA11.md` exists with the planned branch/worktree
- `docs/phase-AA/readiness.md` is updated consistently with the accepted AA.11
  closure state
- no normal 1.2 runtime path depends on `legacy_message_id` support
- docs and tests no longer describe abandoned pre-production SQLite identity
  shapes as supported active behavior
- any remaining `legacy_message_id` references are historical documentation
  only and are not silently exercised during normal bootstrap

## Required Validation

- `cargo test -p atm-rusqlite`
- `cargo test --workspace`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/sprint-AA11.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/adr/ADR-012-one-message-identity.md`
- `docs/phase-U/removal-inventory.md`

## Risks And Watchouts

- if this sprint deletes compatibility code without restating the supported 1.2
  SQLite baseline, operators and QA will not know what database states are in
  scope
- if pre-production repair logic remains silently reachable from normal
  bootstrap, the repo will still be carrying an undeclared compatibility
  contract
