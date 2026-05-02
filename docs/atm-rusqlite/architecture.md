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

## 6. Testability

`atm-rusqlite` must be testable entirely in process.

Rules:
- no daemon process required
- no real socket transport required
- conformance tests should exercise the `atm-core` store traits
- tests may use temporary databases but should not rely on private
  implementation details when validating store-contract behavior
