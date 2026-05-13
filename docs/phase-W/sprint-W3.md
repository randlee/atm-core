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
- preserve interface parity so the same SQLite-backed failure class returns the
  same ATM error code/recovery semantics whether reached from CLI, graft host,
  or peer-transport-triggered daemon work

## Hard Dependencies

- depends on `W.1` for non-silent daemon-side event emission when SQLite
  subsystem signals are forwarded through daemon observability
- no hard dependency on `W.2`
- no code dependency on `W.4`; only ordinary merge-forward discipline applies

## Required Work

- add SQLite subsystem signals at the listed queue, reply, WAL, and budget
  paths
- define exactly how SQLite-backed failures project into doctor/runtime health
- verify protocol envelope parity for non-CLI consumers
- collapse duplicate interface-specific SQLite error/reporting paths if they
  exist
- compare every touched SQLite-backed failure class against the current `main`
  CLI command-failure contract before finalizing any refactor

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
- the sprint keeps SQLite observability bottom-of-stack and does not re-create
  daemon-side semantic reconstruction
- `W.1` is a hard gate for any new SQLite observability emit site; `W.3`
  cannot land SQLite event emission against a silent-discard daemon sink policy
- the sprint preserves one shared doctor/reporting pipeline rather than
  introducing SQLite-specific output plumbing
- the sprint verifies that shared ATM errors returned from SQLite-backed paths
  are preserved consistently through protocol envelopes and non-CLI consumers
- where SQLite-backed error mapping/reporting has duplicated interface-specific
  handling, the sprint should collapse those paths onto one shared
  implementation
- the sprint names the concrete collapse pairs
  `SqliteWriter::submit(...)` / `SharedDb::acquire_connection_guard(...)` and
  `SharedDb::checkpoint_wal(...)` / `DaemonRequestDispatcher::finalize_shutdown(...)`,
  and verifies parity for the three consumer interfaces:
  CLI, `atm-graft` host, and peer-triggered daemon work
- the sprint identifies the shared ATM error/protocol/doctor functions that
  become the single source of truth for each touched SQLite-backed failure
  class
- the sprint reconciles its local code inventory with the shared Phase `W`
  ATM code inventory in `docs/plan-phase-W.md`
- req-qa can verify from the sprint doc that SQLite-backed parity and protocol
  envelope preservation are explicitly owned here

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
- `crates/atm-core/src/protocol.rs`
  - protocol error-envelope preservation for SQLite-backed failures

Current path inventory:
- `crates/atm-core/src/error.rs`
  - shared daemon-unavailable / lifecycle-wedge constructors and stable code
    bindings reused by SQLite-backed command/runtime failures
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
- `crates/atm-core/src/protocol.rs`
  - `ProtocolErrorEnvelope::{from_error,into_atm_error}` parity for non-CLI
    consumers
- `crates/atm-daemon/src/peer_transport.rs`
  - remote peer request/response propagation where SQLite-backed daemon errors
    must survive protocol-envelope projection unchanged

Daemon-side integration points:
- `crates/atm-daemon/src/composition.rs`
  - SQLite boundary assembly wiring
  - remote replay store assembly failure path
- `crates/atm-daemon/src/runtime_health.rs`
  - bounded shutdown WAL checkpoint path
  - doctor/runtime-health projections that can expose SQLite degraded state

Shared paths that must be reused or consolidated:
- `crates/atm-core/src/error.rs`
- `crates/atm-core/src/protocol.rs`
  - `ProtocolErrorEnvelope::{from_error,into_atm_error}`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm/src/commands/doctor.rs`
- `crates/atm/src/output.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`

Current main CLI baseline to preserve:
- SQLite-backed ATM command failures already terminate through the shared ATM
  error surface rather than a SQLite-specific CLI formatter
- the touched SQLite-backed failure classes must continue to use the same ATM
  code and recovery intent when surfaced to CLI, graft host, or peer-triggered
  consumers

Concrete duplicate-path collapse sites to resolve:
- `crates/atm-rusqlite/src/writer/mod.rs::submit(...)`
  and `crates/atm-rusqlite/src/shared_db.rs::acquire_connection_guard(...)`
  both mint queue/budget `DaemonUnavailable` failures with SQLite-specific
  wording; where the recovery contract is the same, they should converge on one
  shared ATM error/recovery helper instead of duplicated strings
- `crates/atm-rusqlite/src/shared_db.rs::checkpoint_wal(...)`
  and `crates/atm-daemon/src/runtime_health.rs::finalize_shutdown(...)`
  both describe shutdown-time WAL degradation; doctor/runtime-health projection
  should be shared rather than independently narrated
- `crates/atm-core/src/protocol.rs::ProtocolErrorEnvelope::{from_error,into_atm_error}`
  and CLI/peer response handling must remain the single mapping path for
  SQLite-backed ATM errors; `W.3` must not leave a second interface-specific
  SQLite error envelope

Stable ATM code inventory for this sprint:
- sqlite writer thread spawn failure:
  - `AtmErrorCode::DaemonUnavailable`
- sqlite writer submission queue timeout:
  - `AtmErrorCode::DaemonUnavailable`
- sqlite writer reply timeout or reply channel disconnect:
  - `AtmErrorCode::DaemonUnavailable`
- sqlite writer shutdown / final WAL checkpoint degradation:
  - operator-facing returned error remains `AtmErrorCode::DaemonUnavailable`
    where the current shared path returns one; doctor/runtime health carries the
    deeper checkpoint degradation detail
- sqlite reader-budget exhaustion:
  - `AtmErrorCode::DaemonUnavailable`
- sqlite boundary assembly / replay-store assembly unavailable:
  - `AtmErrorCode::DaemonUnavailable`
- no new `AtmErrorKind` or `AtmErrorCode` variants are planned for `W.3`

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
- `atm-graft` host and peer-triggered consumers must receive the same ATM error
  code/recovery intent for the same SQLite-backed failure class
- `atm doctor` must expose the deeper subsystem details so an operator can tell
  whether the problem is queue saturation, reply timeout, checkpoint failure,
  or connection-budget exhaustion
- peer-triggered daemon work must preserve the same SQLite-backed ATM code
  through `ProtocolErrorEnvelope::{from_error,into_atm_error}` as the same
  failure class would on same-host CLI

Cross-sprint dependency:
- if `atm-rusqlite` needs a new thin observability port, it must follow the
  Phase V bottom-of-stack rule and stay free of daemon-owned semantic types
- any new SQLite observability emit site must also satisfy the `W.1`
  non-silent emit-fallback rule in addition to the Phase V bottom-of-stack
  constraint
- if `W.4` is running in parallel, any `crates/atm-core/src/protocol.rs`
  change here must stay limited to SQLite-backed envelope parity and be
  merge-forwarded before either branch pushes a final head

## Out of Scope

- unrelated SQLite schema work
- daemon-client startup tracing
- peer replay recovery text

## Required Validation

Plan-auditable now:
- explicit ownership of SQLite-backed doctor projection and protocol parity
- explicit duplicate-path collapse responsibility
- explicit separation from same-host daemon-client tracing and peer replay text

Implementation validation later:
- the reporting split remains intact:
  - CLI gets concise failure output
  - doctor gets the fuller diagnostic surface
- the sprint identifies where the current implementation already returns
  adequate ATM errors versus where the doctor/observability side regressed or
  never completed
- runtime proof that doctor exposes queue backlog, reply timeout, WAL, and
  reader-budget diagnostics
- parity proof that equivalent SQLite-backed failures preserve the same ATM
  code/recovery intent across CLI and non-CLI consumers
- proof that duplicate SQLite-specific reporting or mapping paths were
  collapsed onto shared ATM error / protocol / doctor implementations
