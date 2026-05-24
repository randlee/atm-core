---
id: Z.16
title: Smoke Z.2 Revalidation
status: planned
branch: feature/pZ-s16-smoke-z1-rerun
worktree: ../atm-core-worktrees/feature/pZ-s16-smoke-z1-rerun
target: integrate/phase-Z
---

# Sprint Z.16 — Smoke Z.2 Revalidation

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.16
worktree: ../atm-core-worktrees/feature/pZ-s16-smoke-z1-rerun
branch: feature/pZ-s16-smoke-z1-rerun
status: planned
estimated_scope: medium
```

## Goal

Close `Z1-F002`, rerun the frozen `Z.1` smoke checklist on the approved
`integrate/phase-Z` line after `Z.11` through `Z.15`, and truthfully record the
final `Z.2` revalidation verdict.

## Scope Summary

This sprint owns:

- the SQLite schema migration compatibility fix for copied-state `mail.db`
  initialization (`Z1-F002`)
- rerunning the frozen `Z.1` smoke matrix (`Z1-001` through `Z1-009`)
- closing the `Z.2` readiness row if and only if the rerun passes

This sprint does not begin canary or release execution.

## Governing Requirements

- `REQ-CORE-DAEMON-001`
- `REQ-CORE-RUNTIME-001`
- `REQ-CORE-TEAM-001`
- `REQ-CORE-BOUNDARY-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RequestDispatcher`
- `SqliteWriter`

## Prerequisites

- `Z.15` complete
- `integrate/phase-Z` merged through `Z.15`

## Hard Dependencies

- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Exact Targets

- `crates/atm-rusqlite/src/shared_db.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`
- `docs/phase-Z/sprint-Z16.md`

## Delete / Narrow Inventory

- narrow the SQLite compatibility fix to the schema migration path; do not add
  one-off runtime recovery branches for copied-state DBs
- delete the `Z1-F002` open row from the active blocker set by closing it in the
  smoke findings ledger once revalidation passes

## Non-Goals

- no new canary/dogfood execution work
- no new release checklist execution work
- no widening into new boundary cleanup beyond what the smoke rerun proves is
  still necessary

## Sub-Tasks

1. Fix copied-state SQLite schema initialization.
   Development work:
   - make `ensure_schema(...)` compatible with older `mail_messages` tables that
     still expose `legacy_message_id` and not `message_id`
   - keep the fix in the migration path so copied-state durable baselines are
     upgraded rather than rejected at init
   Required tests:
   - add a regression test that creates the older schema shape and proves the
     current initialization path upgrades it successfully
   Required docs:
   - update `docs/phase-Z/smoke-findings-ledger.md`

2. Rerun the frozen `Z.1` smoke matrix.
   Development work:
   - rebuild release binaries
   - rerun `Z1-001` through `Z1-009` on the clean-room and copied-state lanes
   - confirm `Z1-003`, `Z1-005`, and `Z1-006` stay closed after the `Z.11` and
     `Z.12` fix line
   Required tests:
   - copied-state lane proves daemon start, IPC publication, and retained
     daemon-backed commands now succeed
   Required docs:
   - update `docs/phase-Z/smoke-checklist.md`
   - update `docs/phase-Z/readiness.md`

3. Stamp closure records.
   Development work:
   - close `Z1-F002` in `docs/phase-Z/smoke-findings-ledger.md`
   - stamp the `Z.2` readiness row with final verdict and accepted commit
   - add the `Z.16` sprint ledger row to `docs/project-plan.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md`
   - update `docs/phase-Z/sprint-Z16.md`

## Split Recommendation

If the smoke rerun exposes a new product bug outside `Z1-F002` and outside the
already-accepted `Z.11` through `Z.15` line, stop and open a new follow-up
sprint rather than silently widening `Z.16`.

## Acceptance Criteria

- copied-state `mail.db` initialization succeeds through the current schema
  migration path
- `cargo build --release -p agent-team-mail -p atm-daemon` passes
- `cargo test --workspace` passes
- `python3 .just/run_lint.py all` passes
- the frozen `Z.1` smoke rows show final `PASS` revalidation verdicts
- `docs/phase-Z/smoke-findings-ledger.md` records `Z1-F002` as `closed` with
  `revalidation_result = PASS`
- `docs/phase-Z/readiness.md` records `Z.2` verdict `PASS`
- `docs/project-plan.md` includes the `Z.16` sprint ledger row

## Non-Closure

- `Z.16` does not begin `Z.3`
- `Z.16` does not finalize release sign-off

## Production-Ready Expectation

No smoke blocker should remain open after `Z.16`; copied-state daemon bring-up
and the retained command surface must be production-ready before canary begins.

## Required Validation

- `cargo build --release -p agent-team-mail -p atm-daemon`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`
- `docs/phase-Z/sprint-Z16.md`

## Risks And Watchouts

- keep the copied-state lane disposable; never write against live host ATM state
- do not treat a synthetic compatibility test as a substitute for the actual
  frozen smoke rerun
- if a smoke row still fails, record it truthfully and stop before canary
