---
id: W.3
title: SQLite Subsystem Observability
status: planned
branch: TBD
worktree: TBD
---

# Sprint W.3 — SQLite Observability

## Goals

- add real subsystem observability for SQLite writer and connection-budget
  failures
- restore the existing expectation that SQLite failure is a first-class
  critical issue visible through:
  - concise ATM CLI failure output when a command hits the failure
  - fuller `atm doctor` diagnostics for queue backlog, reply timeout, WAL
    health, and reader-budget exhaustion
- keep emission/reporting on shared paths; SQLite-specific work should be about
  event content and insertion points, not a separate reporting implementation

## Acceptance Criteria

- every SQLite path listed in the current path inventory is addressed directly
  in the implementation scope
- the sprint covers the exact SQLite insertion points for:
  - writer queue timeout
  - writer reply timeout
  - writer shutdown / final WAL checkpoint
  - shared reader-budget exhaustion
- the sprint identifies the daemon wiring needed to expose SQLite health
  through doctor
- the sprint makes the reporting split explicit:
  - CLI gets concise failure output
  - doctor gets the fuller diagnostic surface
- the sprint identifies where the current implementation already returns
  adequate ATM errors versus where the doctor/observability side regressed or
  never completed
- the sprint keeps SQLite observability bottom-of-stack and does not re-create
  daemon-side semantic reconstruction
- the sprint preserves one shared doctor/reporting pipeline rather than
  introducing SQLite-specific output plumbing

## Implementation Notes

Primary SQLite insertion points:
- `crates/atm-rusqlite/src/writer/mod.rs`
  - `SqliteWriter::start_with_settings(...)`
  - `SqliteWriter::submit(...)`
  - `checkpoint_writer_connection(...)`
  - shutdown join-helper branches in `Drop for SqliteWriter`
- `crates/atm-rusqlite/src/shared_db.rs`
  - `SharedDb::checkpoint_wal(...)`
  - `SharedDb::acquire_connection_guard(...)`
- `crates/atm-rusqlite/src/lib.rs`
  - `SqliteBoundaryAssembly::new(...)`
  - `SqliteBoundaryAssembly::checkpoint_wal(...)`

Current path inventory:
- `crates/atm-rusqlite/src/writer/mod.rs`
  - `SqliteWriter::start_with_settings(...)`
    - writer thread spawn failure
  - `SqliteWriter::submit(...)`
    - submission channel closed
    - submission queue timeout
    - submission channel disconnected
    - reply timeout
    - reply channel disconnected
  - `Drop for SqliteWriter`
    - shutdown signal skipped because queue is full
    - writer thread panic during shutdown
    - shutdown join timeout
    - join-helper disconnected
  - `checkpoint_writer_connection(...)`
    - final WAL checkpoint warning
- `crates/atm-rusqlite/src/shared_db.rs`
  - `SharedDb::checkpoint_wal(...)`
    - daemon-shutdown checkpoint failure
  - `SharedDb::acquire_connection_guard(...)`
    - connection-budget state lock poisoned
    - reader-budget exhausted
- `crates/atm-rusqlite/src/lib.rs`
  - `SqliteBoundaryAssembly::new(...)`
    - boundary open/assemble failure
  - `SqliteBoundaryAssembly::checkpoint_wal(...)`
    - top-level WAL checkpoint propagation
- `crates/atm-daemon/src/composition.rs`
  - replay-store assembly failure currently logged as warning-only
- `crates/atm-daemon/src/runtime_health.rs`
  - `finalize_shutdown()` bounded `sqlite_wal_checkpoint` step

Daemon-side integration points:
- `crates/atm-daemon/src/composition.rs`
  - SQLite boundary assembly wiring
  - remote replay store assembly failure path
- `crates/atm-daemon/src/runtime_health.rs`
  - bounded shutdown WAL checkpoint path
  - doctor/runtime-health projections that can expose SQLite degraded state

Event families required:
- writer queue backlog / timeout
- writer reply timeout
- writer lane shutdown degradation
- WAL checkpoint success/failure
- reader-budget exhaustion
- SQLite boundary assembly unavailable

Critical issue classes covered directly by this sprint:
- SQLite writer queue failure
- SQLite reply timeout
- SQLite WAL checkpoint failure
- SQLite reader-budget exhaustion
- ATM command failure caused by SQLite unavailability or SQLite backpressure

Doctor/CLI reporting contract:
- ATM CLI must keep command failure concise and actionable for SQLite-backed
  failures
- `atm doctor` must expose the deeper subsystem details so an operator can tell
  whether the problem is queue saturation, reply timeout, checkpoint failure,
  or connection-budget exhaustion

Cross-sprint dependency:
- if `atm-rusqlite` needs a new thin observability port, it must follow the
  Phase V bottom-of-stack rule and stay free of daemon-owned semantic types

## Out of Scope

- unrelated SQLite schema work
- daemon-client startup tracing
- peer replay recovery text
