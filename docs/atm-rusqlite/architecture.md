# ATM-Rusqlite Crate Architecture

## 1. Purpose

This document defines the `atm-rusqlite` crate architectural boundary.

It complements the product and `atm-core` architecture documents and owns only
the first concrete SQLite implementation of the Phase Q store family.

## 2. Architectural Rules

- `atm-rusqlite` implements store contracts; it does not define them.
- `atm-rusqlite` must not own workflow, routing, daemon, watcher, transport,
  or notifier business logic.
- all direct SQLite access stays inside this crate.
- concrete `rusqlite` types, row mappers, connection wiring, and migration
  helpers remain private implementation details.
- public callers depend on `atm-core` traits such as `MailStore`, `TaskStore`,
  and `RosterStore`, not on concrete SQLite structs.
- routine database failure handling uses typed `Result`/error-enum paths rather
  than panic/unwrap.

## 3. Store Implementation Shape

The first implementation may share one internal SQLite root object, but the
public boundary shape must remain split:

- `MailStore`
- `TaskStore`
- `RosterStore`

Architectural rule:
- sharing one internal connection/transaction root is acceptable
- exposing one public god-interface is not

## 4. Migration And Transaction Boundary

`atm-rusqlite` owns:

- opening/creating the SQLite database
- schema bootstrap and migration execution
- transaction begin/commit/rollback implementation
- enforcement of:
  - `journal_mode = WAL`
  - `foreign_keys = ON`
  - `busy_timeout = 1500ms`
  - explicit transactions for mutating operations

`atm-rusqlite` does not own:

- deciding when the application should perform a command
- transport/runtime retry policy
- daemon lifecycle/shutdown behavior

## 5. Error Translation Boundary

`atm-rusqlite` must translate raw SQLite failures into typed ATM store errors.

Rules:
- no raw SQLite error should leak across the public store boundary as the
  primary failure type
- ATM-owned `AtmErrorCode` remains the public code vocabulary
- the crate must not invent local ad hoc error-code strings
- connection open/configuration is not complete until `journal_mode = WAL`,
  `foreign_keys = ON`, and `busy_timeout = 1500ms` have all been enforced
- `SQLITE_BUSY` must map to a typed retry-able ATM store error rather than
  leaking as a raw driver failure
- `SQLITE_BUSY_SNAPSHOT` must map to a typed retry-able or replay-required ATM
  store error according to the calling contract
- WAL checkpoint failure during graceful shutdown is best-effort only: the
  failure must be logged with structured context and the daemon must still
  proceed with shutdown
- disk-full / `IOERR_WRITE` class failures must map to typed non-retryable
  persistence errors unless a narrower retry contract is explicitly documented

## 6. Blocking I/O And Async Runtime Interaction

`rusqlite` is synchronous blocking I/O.

Rules:
- if `atm-daemon` runs on a Tokio async runtime, direct `rusqlite` calls must
  execute on `tokio::task::spawn_blocking` or an equivalent dedicated blocking
  thread pool
- direct invocation of `rusqlite` calls from an async task is not permitted in
  production because it can block the runtime under mailbox or ingest load
- the dedicated blocking execution path must respect the Phase Q SQLite handle
  budget of `1..=4`

## 7. Testability

`atm-rusqlite` must be testable entirely in process.

Rules:
- no daemon process required
- no real socket transport required
- conformance tests should exercise the `atm-core` store traits
- tests may use temporary databases but should not rely on private
  implementation details when validating store-contract behavior
