# AA.4 Delete Remaining Daemon SQLite Leaks

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.4
worktree: ../atm-core-worktrees/feature/pAA-s4-delete-daemon-sqlite-leaks
branch: feature/pAA-s4-delete-daemon-sqlite-leaks
status: planned
estimated_scope: medium
```

## Goal

Delete the remaining daemon-side SQLite observability, replay-store, and test
support leakage after composition and doctor behavior have moved out.

## Scope Summary

This sprint is deletion-heavy. It removes code that exists only because the
boundary was broken and should not be re-homed inside the daemon.

The concrete delete/move decisions are frozen now:
- delete `crates/atm-daemon/src/sqlite_observability.rs`
- delete `crates/atm-daemon/src/runtime_health_test_support.rs`
- delete `mod sqlite_observability;` from `crates/atm-daemon/src/lib.rs`
- delete `SqliteRemoteReplayStore` from `crates/atm-daemon/src/lib.rs`
- delete `use atm_rusqlite::SqliteBoundaryAssembly;` and the
  `RemoteReplayStateRecord` re-export from `crates/atm-daemon/src/lib.rs`
- delete direct `atm_rusqlite::assemble_boundary*` test use from:
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/tests_advisory.rs`
- delete `mark_sqlite_unavailable(...)` and
  `mark_sqlite_unavailable_with_detail(...)` from
  `crates/atm-daemon/src/runtime_status_cache.rs`
- keep `crates/atm-daemon/src/peer_transport.rs`, but rewrite it to consume
  storage-neutral replay DTOs/traits that no longer originate in `atm-rusqlite`

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-DAEMON-OBS-003`
- `REQ-RUSQLITE-STORE-001`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`

## Governing Boundaries

- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`

## Prerequisites

- `AA.2`
- `AA.3`

## Hard Dependencies

- `atm-runtime` owns composition
- store doctor trait owns store diagnostics

## Non-Goals

- final boundary relock
- permanent enforcement guardrails

## Sub-Tasks

- Delete daemon-owned SQLite observability glue.
  Development work: remove `sqlite_observability.rs` and route SQLite
  observability injection entirely through `atm-runtime` / `atm-rusqlite`.
  Required tests: retained observability regression coverage.
  Required doc or boundary updates: daemon/rusqlite architecture docs.

- Delete daemon-owned SQLite test support.
  Development work: remove `runtime_health_test_support.rs` and any daemon test
  setup that assembles SQLite directly. Replace daemon-local assembly with
  runtime-owned or subsystem-owned fixtures.
  Required tests: replace with subsystem or composition-level tests.
  Required doc or boundary updates: testing docs if test ownership changes.

- Remove replay/store type leakage from daemon public/internal seams.
  Development work: eliminate `SqliteRemoteReplayStore`,
  `RemoteReplayStateRecord` re-exports, and any daemon-local SQLite replay
  wrappers. The exact destination decision is:
  - storage-neutral replay DTO/trait live outside `atm-daemon` and
    `atm-rusqlite`, under `atm-core`
  - concrete SQLite replay implementation lives in `atm-runtime`
  Required tests: peer transport / replay regression coverage.
  Required doc or boundary updates: daemon boundaries if trait ownership moves.

## Split Recommendation

Keep this sprint deletion-first. If a leaked helper is not required by the
simple router model, remove it instead of preserving compatibility for it.

## Acceptance Criteria

- daemon production code contains no SQLite observability glue
- daemon test support no longer assembles SQLite directly
- daemon replay/store seams no longer expose SQLite-owned record types
- `runtime_status_cache.rs` contains no SQLite-named degradation helpers
- the remaining daemon code is materially smaller and simpler

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/project-plan.md`

## Risks And Watchouts

- replay helpers are easy to preserve by habit; if they still mention SQLite in
  the daemon, this sprint did not actually close
