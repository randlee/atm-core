# AC.3 SQLite Backend Convergence

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.3
worktree: ../atm-core-worktrees/feature/pAC-s3-sqlite-backend-convergence
branch: feature/pAC-s3-sqlite-backend-convergence
status: planned
estimated_scope: large
```

## Goal

Make the concrete SQLite backend implement the same `atm-storage` contract and
remove its dependency on `atm-core`.

## Scope Summary

This sprint adapts the SQLite implementation to the shared traits, moves any
required shared types into `atm-storage`, and freezes post-commit notification
semantics for the SQL backend.

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/sprint-AC1.md`
- `crates/atm-rusqlite/`
- current SQLite-related boundary traits in `atm-core`

## Prerequisites

- `AC.1`

## Out Of Scope

- no SQL Server implementation yet
- no full RPC envelope simplification yet

## Deliverables

- the concrete SQLite backend implements the shared core traits from `atm-storage`
- the SQLite backend no longer depends on `atm-core`
- SQLite-specific power stays in capability traits rather than the base CRUD traits
- notifications are explicitly post-commit:
  - write succeeds
  - transaction commits
  - only then may `message_received` / `roster_changed` fire

- If stronger delivery guarantees are needed, the sprint documents the outbox/future delayed-notification design rather than burying it in ad hoc runtime logic.

## Acceptance Criteria

- the SQLite backend can satisfy the shared CRUD contract without importing `atm-core`
- no base trait method is widened purely to fit SQLite-specific power
- notification semantics are documented as post-commit only

## Required Validation

- `cargo test -p atm-rusqlite`
- `cargo clippy -p atm-rusqlite -- -D warnings`
- `cargo tree -p atm-rusqlite`
- `git diff --check`

## Required Document Updates

- `docs/phase-AC/sprint-AC3.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- backend architecture docs for SQLite storage ownership

## Risks And Watchouts

- if SQLite still needs `atm-core`, the shared type move is incomplete
- if notification behavior happens before commit, the notifier contract is wrong
