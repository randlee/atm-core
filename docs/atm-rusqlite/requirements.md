# ATM-Rusqlite Crate Requirements

## 1. Purpose

This document defines the `atm-rusqlite` crate requirements.

The `atm-rusqlite` crate owns the first concrete SQLite implementation of the
durable store boundaries defined by `atm-core`.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

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
- `REQ-RUSQLITE-IMMUT-*`

Initial crate requirement IDs:

- `REQ-RUSQLITE-STORE-001` `atm-rusqlite` must implement the
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
- `REQ-RUSQLITE-IMMUT-001` the hot mailbox write path must use insert-first
  immutable-row semantics for `mail_messages`, treating duplicate message keys
  as durable no-op replays rather than payload-rewrite events. Satisfies:
  `REQ-RUNTIME-002`.
- `REQ-RUSQLITE-IMMUT-002` once a `mail_messages` row exists, later ATM-owned
  writes must preserve the original envelope/payload fields and keep mutable
  live state in the projection tables instead of rewriting the immutable row.
  Satisfies: `REQ-RUNTIME-002`.
- `REQ-RUSQLITE-IMMUT-003` the hot mailbox write path must not issue an
  existence probe before submitting message inserts to the writer lane, such
  as `SELECT 1` or `COUNT(*)` used only to decide whether a duplicate key
  should be written. Queue semantics and row-count detection own duplicate
  handling. This ban does not apply to crate-owned invariant validation queries
  that must run before `INSERT` (for example single-successor or legacy
  identity checks), as documented in [`architecture.md`](./architecture.md).
  Satisfies: `REQ-RUNTIME-002`.

## 4. Required References

The `atm-rusqlite` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-Q.md`](../plan-phase-Q.md)
- [`../plan-phase-R.md`](../plan-phase-R.md)
- [`../plan-phase-S.md`](../plan-phase-S.md)
- [`../phase-T/sprint-T2-sqlite-writer.md`](../phase-T/sprint-T2-sqlite-writer.md)
- [`../phase-T/sprint-T3-immutable-rows.md`](../phase-T/sprint-T3-immutable-rows.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`./boundaries.md`](./boundaries.md)

## 5. Phase R SQLite Implementation Rules

Requirement IDs:
- `REQ-RUSQLITE-STORE-001`
- `REQ-RUSQLITE-MIGRATION-001`
- `REQ-RUSQLITE-ERROR-001`
- `REQ-RUSQLITE-TEST-001`

Required rules:
- only `atm-rusqlite` may own direct `rusqlite` calls in the first
  implementation line
- concrete SQLite details remain private to this crate
- callers depend on `atm-core` store traits, not on `rusqlite` types
- the default production durable database path is `~/.atm/db/mail.db`
- the host-scoped SQLite database is one shared durable store keyed by team
  and agent, not one database per team
- the daemon is the only ATM-owned writer to the production database
- Phase T hot mailbox writes must use one crate-private SQLite writer lane
  instead of ad-hoc per-operation write transactions
- read-only consumers may query SQLite directly as a supported integration
  surface, but ATM-owned writes must still go through the documented runtime
  and store boundaries
- the current runtime composition owner may depend on this crate in order to
  assemble production adapters, but thin callers and extension crates must not
- schema bootstrap must be deterministic and idempotent
- schema bootstrap must run once per database root before normal store
  operations, not on every connection acquisition
- WAL / foreign-key / explicit-transaction policy must be enforced here
- the crate-private writer lane must keep the connection budget explicit:
  `1` permanent writer handle plus at most `3` concurrent reader handles
- the writer lane queue must be bounded and blocking; full-queue behavior is
  backpressure, not silent drop
- async daemon callers must not block Tokio worker threads directly when
  submitting to the writer lane
- `REQ-RUSQLITE-IMMUT-001` the immutable-row path must use insert-first
  semantics owned by the writer lane rather than conflict-driven row mutation
- `REQ-RUSQLITE-IMMUT-002` duplicate message writes must preserve the first
  stored payload and must not rewrite immutable `mail_messages` envelope fields
- `REQ-RUSQLITE-IMMUT-003` immutable-row enforcement must remove the pre-write
  probe from the hot mailbox write path
  This ban applies to hot-path probe queries only; crate-owned invariant
  validation queries that reject known schema or logical violations before SQL
  submission are permitted.
- `MailStore`, `TaskStore`, and `RosterStore` may share one internal SQLite
  root object, but they must not collapse into one public god-interface
- the durable schema must expose:
  - one concrete message table with queryable identity/timestamp columns plus
    full-envelope JSON
  - one per-member `team_roster` durable projection for runtime-facing roster
    lookup
  - one crate-private roster snapshot path sufficient to round-trip
    `TeamConfig` through the current boundary DTOs
- routine SQLite failures must return typed errors, not panic/unwrap
- constraint failures must map to validation-class ATM errors rather than
  generic store write failures
- busy/locked failures must map to lock-timeout/busy-class ATM errors
- open/create/read-only failures must map to mailbox-write/store-write-class
  ATM errors rather than validation
- conformance tests should validate behavior through the `atm-core` store
  traits rather than by depending on internal SQLite details
- Phase S queue support includes bounded mailbox metadata-query helpers and
  supporting indexes/row projections for `atm list` / selector-driven
  `atm read`, but selector semantics remain owned by `atm-core`
- bounded mailbox metadata queries in this crate must:
  - return metadata rows only, not full message bodies
  - support a hard SQL `LIMIT`
  - expose queue counts separately from row fetch
  - preserve durable `taskId` lookup for metadata rows
- the T.2 caller audit for surviving `SharedDb::with_transaction(...)` writes
  must stay explicit in `docs/atm-rusqlite/architecture.md`
- most SQLite tests should use dedicated in-memory fixtures with explicit
  setup/cleanup
- only a small deliberate suite may use on-disk temporary databases for
  reopen, migration, and filesystem-behavior verification
- tests must never use or mutate the production durable root under
  `~/.atm/db/mail.db`
