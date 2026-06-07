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

Production-ready commitment:
- every deliverable listed in this sprint is expected to land at a
  production-ready level for the SQLite-backend convergence scope this sprint
  claims; partial rename-only or boundary-only closure is not accepted

Primary closure rule:
- `AC.3` is the primary closure sprint for SQLite-backend internalization and
  for every `capability-review` storage seam that survives or is deleted
- `AC.1` may cap the shared contract, but `AC.3` decides whether replay,
  doctor, health, and lifecycle seams become optional capabilities,
  backend-internal details, or deletions
- `AC.3` also owns the backend naming cutover itself: this sprint does not
  defer the final crate identity to a later rename-only follow-up

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
  renamed and converged into the target backend crate identity
  `atm-storage-rusqlite`
- the SQLite backend no longer depends on `atm-core`
- SQLite-specific power stays in capability traits rather than the base CRUD traits
- notifications are explicitly post-commit:
  - write succeeds
  - transaction commits
  - only then may `message_received` / `roster_changed` fire

- If stronger delivery guarantees are needed, the sprint documents the outbox/future delayed-notification design rather than burying it in ad hoc runtime logic.
- capability-candidate seams are resolved explicitly:
  - promoted to a named capability trait
  - internalized below `atm-storage-rusqlite`
  - or deleted
  - leaving them as undecided candidates is not an accepted outcome

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

If any of those surfaces survive publicly, the allowed named capability set is
small and explicit:

```rust
pub trait StorageHealth { /* health / doctor read surface only */ }
pub trait ReplayStore { /* replay ingest / replay state only */ }
pub trait RuntimeStorageFinalizer { /* finalizer hook only if still justified */ }
```

No additional capability trait may be invented in `AC.3` without updating the
Phase AC ADR and the shared contract docs in the same change.

## Execution Checklist

Implementation order for `AC.3`:

1. Point the SQLite backend at `atm-storage` first; do not start by copying `atm-core` helpers.
2. Re-home any truly shared helper into `atm-storage`; keep SQLite-only helpers in the backend crate.
3. Convert the main storage implementation to the canonical shared types selected in `AC.1`.
4. Make the rename/convergence intent explicit in docs and boundaries:
   - current source crate: `atm-rusqlite`
   - target backend identity: `atm-storage-rusqlite`
   - the rename lands in `AC.3`; if the backend is still named
     `atm-rusqlite` at sprint close, the sprint is incomplete
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

- `atm-storage-rusqlite` is the backend identity left behind by the sprint, and
  `atm-rusqlite` survives only as the pre-convergence source state
- the SQLite backend is a backend implementation, not a second copy of `atm-core` storage semantics
- SQLite-only observability and lifecycle helpers are no longer exposed as if they were shared contract concepts
- capability traits are explicit and few, not an escape hatch for old surface-area sprawl
- `SqliteBoundaryAssembly` closure happens here; `AC.4` may only remove
  remaining consumers of its replacement
- every capability-candidate row from the ledger has an explicit keep/delete
  outcome by sprint close; no "decide later" carry-forward is allowed

## Acceptance Criteria

- the SQLite backend can satisfy the shared CRUD contract without importing `atm-core`
- no base trait method is widened purely to fit SQLite-specific power
- notification semantics are documented as post-commit only
- the backend crate rename to `atm-storage-rusqlite` lands in this sprint
- `lint_boundaries.py` accepts the updated `atm-storage-rusqlite` boundary
  TOMLs before sprint closure
- `boundaries/atm-rusqlite/` TOML records do not exist after the rename lands;
  stale pre-rename boundary records are removed and verified absent by
  `lint_boundaries.py`
- `SqliteBoundaryAssembly` does not survive as a required public assembly bundle above the trait line
- no SQLite-only observability or replay helper is promoted into the base CRUD contract by convenience
- every `capability-candidate` ledger row owned by `AC.3` is either:
  - a named optional capability trait
  - an internal backend detail
  - or deleted

## Required Validation

- `cargo test -p atm-storage-rusqlite`
- `cargo clippy -p atm-storage-rusqlite -- -D warnings`
- `cargo tree -p atm-storage-rusqlite`
- `python3 scripts/lint_boundaries.py`
- `git diff --check`
- verify the updated boundary TOMLs and `cargo tree` output both show `atm-storage`, not `atm-core`, as the shared storage dependency
- `rg -n "SqliteBoundaryAssembly|SqliteObservability|RemoteReplayStore|RuntimeStorageFinalizer" crates/atm-storage-rusqlite crates/atm-runtime crates/atm-core -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC3.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- backend architecture docs for SQLite storage ownership
- rename the backend crate path to `crates/atm-storage-rusqlite/`
- update `boundaries/atm-storage-rusqlite/mail-store-sqlite.toml`
- update `boundaries/atm-storage-rusqlite/task-store-sqlite.toml`
- update `boundaries/atm-storage-rusqlite/roster-store-sqlite.toml`
- delete `boundaries/atm-rusqlite/` TOML records when the crate rename to
  `atm-storage-rusqlite` lands
- replace `atm-core` with `atm-storage` in `allowed_dependencies` for the shared storage ownership records
- pair the dependency-tree check with a boundary-lint consistency check before sprint closure so `lint_boundaries.py` accepts the updated `atm-storage-rusqlite` ownership TOMLs

## Risks And Watchouts

- if SQLite still needs `atm-core`, the shared type move is incomplete
- if notification behavior happens before commit, the notifier contract is wrong
- if replay/doctor/finalizer seams are promoted wholesale instead of trimmed, `atm-storage` will regrow the old DTO problem under new names
- if the rename is deferred, later sprints will inherit avoidable boundary TOML,
  docs, and cargo-tree drift
