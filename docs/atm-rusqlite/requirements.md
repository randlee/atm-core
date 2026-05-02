# ATM-Rusqlite Crate Requirements

## 1. Purpose

This document defines the `atm-rusqlite` crate requirements.

The `atm-rusqlite` crate owns the first concrete SQLite implementation of the
Phase Q durable store boundaries defined by `atm-core`.

## 2. Ownership

`atm-rusqlite` owns:

- concrete `rusqlite`-backed implementations of:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
- SQLite connection/bootstrap wiring
- schema migrations/bootstrap execution
- transaction execution inside the concrete store implementation
- SQLite-specific translation into typed ATM store errors

`atm-rusqlite` does not own:

- workflow/state-machine business logic
- CLI parsing/rendering
- daemon transport/runtime logic
- inbox JSONL parsing or writing
- agent notification delivery
- daemon live-status truth

## 3. Requirement Namespace

The `atm-rusqlite` crate uses the `REQ-RUSQLITE-*` namespace.

Initial allocation:

- `REQ-RUSQLITE-STORE-*`
- `REQ-RUSQLITE-MIGRATION-*`
- `REQ-RUSQLITE-ERROR-*`
- `REQ-RUSQLITE-TEST-*`

Initial crate requirement IDs:

- `REQ-RUSQLITE-STORE-001` `atm-rusqlite` must implement the Phase Q
  `MailStore`, `TaskStore`, and `RosterStore` contracts without widening those
  interfaces. Satisfies:
  `REQ-CORE-RUNTIME-001`, `REQ-CORE-STORE-001`, `REQ-CORE-STORE-002`.
- `REQ-RUSQLITE-MIGRATION-001` `atm-rusqlite` must own deterministic schema
  bootstrap and migration execution. Satisfies:
  `REQ-CORE-STORE-001`, `REQ-CORE-STORE-002`.
- `REQ-RUSQLITE-ERROR-001` `atm-rusqlite` must translate SQLite failures into
  typed ATM store errors with stable `AtmErrorCode` mapping. Satisfies:
  `REQ-CORE-BOUNDARY-002`.
- `REQ-RUSQLITE-TEST-001` `atm-rusqlite` must be testable in process without
  requiring daemon or real socket runtime. Satisfies:
  `REQ-CORE-TEST-RUNTIME-001`.

## 4. Required References

The `atm-rusqlite` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-Q.md`](../plan-phase-Q.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)

## 5. Phase Q SQLite Implementation Rules

Requirement IDs:
- `REQ-RUSQLITE-STORE-001`
- `REQ-RUSQLITE-MIGRATION-001`
- `REQ-RUSQLITE-ERROR-001`
- `REQ-RUSQLITE-TEST-001`

Required rules:
- only `atm-rusqlite` may own direct `rusqlite` calls in the first Phase Q
  implementation line
- concrete SQLite details remain private to this crate
- callers depend on `atm-core` store traits, not on `rusqlite` types
- schema bootstrap must be deterministic and idempotent
- WAL / foreign-key / explicit-transaction policy must be enforced here
- `MailStore`, `TaskStore`, and `RosterStore` may share one internal SQLite
  root object, but they must not collapse into one public god-interface
- routine SQLite failures must return typed errors, not panic/unwrap
- conformance tests should validate behavior through the `atm-core` store
  traits rather than by depending on internal SQLite details
