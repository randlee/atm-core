# AC.3 SQLite Backend Convergence (`atm-rusqlite` -> `atm-storage-rusqlite`)

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

Make the current concrete SQLite backend (`crates/atm-rusqlite`) converge into
the target backend crate shape `crates/atm-storage-rusqlite`, implement the
same `atm-storage` contract, and remove its dependency on `atm-core`.

## Scope Summary

This sprint adapts the current SQLite implementation to the shared traits,
moves any required shared types into `atm-storage`, freezes post-commit
notification semantics for the SQL backend, and makes the final backend naming
explicit.

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/sprint-AC1.md`
- current crate source: `crates/atm-rusqlite/`
- target backend crate name: `crates/atm-storage-rusqlite/`
- current SQLite-related boundary traits in `atm-core`

## Prerequisites

- `AC.1`

## Out Of Scope

- no SQL Server implementation yet
- no full RPC envelope simplification yet

## Deliverables

- the concrete SQLite backend implements the shared core traits from `atm-storage`
- the backend naming is explicit: the current `atm-rusqlite` implementation is
  converged toward the target backend crate identity `atm-storage-rusqlite`
- the SQLite backend no longer depends on `atm-core`
- SQLite-specific power stays in capability traits rather than the base CRUD traits
- notifications are explicitly post-commit:
  - write succeeds
  - transaction commits
  - only then may `message_received` / `roster_changed` fire

- If stronger delivery guarantees are needed, the sprint documents the outbox/future delayed-notification design rather than burying it in ad hoc runtime logic.

## Ledger-Driven Type Work

`AC.3` owns the SQLite-only support surface and the replay / finalizer seams
that the ledger marked as backend-only, capability-candidate, or delete-bundle.

SQLite-internal types that should stay below the trait line:

- `SqliteWriterLockGuard`
- `SqliteObservabilityOutcome`
- `SqliteObservabilityEvent`
- `SqliteObservability`
- `NullSqliteObservability`

Backend-shaped helpers that must not survive as shared storage abstractions:

- `SqliteBoundaryAssembly`
- `RuntimeBundle`

Capability review surfaces that must either become small optional capability
traits or remain backend-internal:

- `ReplaySource`
- `MailStoreIngestReplayState`
- `MailStoreHealthSnapshot`
- `RosterStoreHealthSnapshot`
- `MailStoreDoctorReport`
- `TaskStoreDoctorReport`
- `RosterStoreDoctorReport`
- `RemoteReplayStateRecord`
- `RemoteReplayStore`
- `RuntimeStorageFinalizer`

## Execution Checklist

Implementation order for `AC.3`:

1. Point the SQLite backend at `atm-storage` first; do not start by copying `atm-core` helpers.
2. Re-home any truly shared helper into `atm-storage`; keep SQLite-only helpers in the backend crate.
3. Convert the main storage implementation to the canonical shared types selected in `AC.1`.
4. Make the rename/convergence intent explicit in docs and boundaries:
   - current source crate: `atm-rusqlite`
   - target backend identity: `atm-storage-rusqlite`
5. Review each capability-candidate seam explicitly:
   - keep as optional capability trait
   - internalize below the backend line
   - or delete
6. Delete or replace `SqliteBoundaryAssembly`.
7. Freeze the post-commit notification rule in code and docs:
   - durable write
   - commit
   - only then notify

Proof this sprint must leave behind:

- `atm-storage-rusqlite` is the target backend identity, and the current `atm-rusqlite` implementation is only the source implementation being converged
- the SQLite backend is a backend implementation, not a second copy of `atm-core` storage semantics
- SQLite-only observability and lifecycle helpers are no longer exposed as if they were shared contract concepts
- capability traits are explicit and few, not an escape hatch for old surface-area sprawl

## Acceptance Criteria

- the SQLite backend can satisfy the shared CRUD contract without importing `atm-core`
- no base trait method is widened purely to fit SQLite-specific power
- notification semantics are documented as post-commit only
- `SqliteBoundaryAssembly` does not survive as a required public assembly bundle above the trait line
- no SQLite-only observability or replay helper is promoted into the base CRUD contract by convenience

## Required Validation

- `cargo test -p atm-rusqlite` or the renamed backend crate once the rename lands
- `cargo clippy -p atm-rusqlite -- -D warnings` or the renamed backend crate once the rename lands
- `cargo tree -p atm-rusqlite` or the renamed backend crate once the rename lands
- `git diff --check`
- verify the updated boundary TOMLs and `cargo tree` output both show `atm-storage`, not `atm-core`, as the shared storage dependency
- `rg -n "SqliteBoundaryAssembly|SqliteObservability|RemoteReplayStore|RuntimeStorageFinalizer" crates/atm-rusqlite crates/atm-runtime crates/atm-core -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC3.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- backend architecture docs for SQLite storage ownership
- update `boundaries/atm-rusqlite/mail-store-sqlite.toml` or the renamed backend ownership path if the crate rename lands in this sprint
- update `boundaries/atm-rusqlite/task-store-sqlite.toml` or the renamed backend ownership path if the crate rename lands in this sprint
- update `boundaries/atm-rusqlite/roster-store-sqlite.toml` or the renamed backend ownership path if the crate rename lands in this sprint
- replace `atm-core` with `atm-storage` in `allowed_dependencies` for the shared storage ownership records
- pair the dependency-tree check with a boundary-lint consistency check before sprint closure

## Risks And Watchouts

- if SQLite still needs `atm-core`, the shared type move is incomplete
- if notification behavior happens before commit, the notifier contract is wrong
- if replay/doctor/finalizer seams are promoted wholesale instead of trimmed, `atm-storage` will regrow the old DTO problem under new names
