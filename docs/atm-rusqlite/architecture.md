# ATM-Rusqlite Crate Architecture

## 1. Purpose

This document defines the `atm-storage-rusqlite` crate architectural boundary.

It complements the product and `atm-core` architecture documents and owns only
the first concrete SQLite implementation of the current store family.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

## 1.1 ADRs

## Concrete SQLite adapters remain private

```yaml
adr_id: ADR-ATM-RUSQLITE-001
crate: atm-storage-rusqlite
title: Concrete SQLite adapters remain private
status: accepted
date: 2026-05-03
deciders:
  - team-lead
  - arch-ctm
tags:
  - privacy
  - sqlite
related_boundaries:
  - BOUNDARY-MailStore-Sqlite
  - BOUNDARY-RosterStore-Sqlite
code_references:
  - docs/atm-rusqlite/boundaries.md
  - docs/atm-core/boundaries.md
```

Context:
- Direct caller access to SQLite implementation types is one of the clearest
  architecture violations from the abandoned early SQLite line.

Decision:
- Concrete SQLite adapter types, constructors, and re-exports remain private.
- Callers depend on `atm-core` contracts, not on concrete SQLite types.

Consequences:
- CLI and thin extension crates cannot bypass store contracts.
- Runtime composition may still assemble the concrete adapters through the
  legal composition owner.

Alternatives considered:
- Public concrete store types with policy enforced only by review.

Follow-up work:
- Keep forbidden dependency edges and reference checks aligned with this rule.

## 2. Architectural Rules

- `atm-storage-rusqlite` implements store contracts; it does not define them.
- `atm-storage-rusqlite` must not own workflow, routing, daemon, watcher, transport,
  or notifier business logic.
- bounded queue-query support for Phase S now owns the concrete SQL metadata
  projections and bounded row/count helpers used by `atm list` and
  selector-driven `atm read`, while logical message selection rules remain in
  `atm-core`
- all direct SQLite access stays inside this crate.
- concrete `rusqlite` types, row mappers, connection wiring, and migration
  helpers remain private implementation details.
- public callers depend on `atm-storage` traits plus runtime-owned
  `atm-core` adapter seams, not on concrete SQLite structs.
- Phase AA pairs those capability traits with subsystem-owned doctor traits so
  SQLite-specific diagnosis stays in this crate instead of moving upward into
  daemon or CLI code.
- schema changes are architecture changes and require explicit user approval
  plus matching doc updates before they land.
- routine database failure handling uses typed `Result`/error-enum paths rather
  than panic/unwrap.
- thin callers and runtime callers should depend on `atm-core` contracts
  rather than this crate directly
- the current production runtime composition root may depend on this crate in
  order to assemble concrete store adapters

## 3. Store Implementation Shape

The first implementation may share one internal SQLite root object, but the
public boundary shape must remain split:

- `MailStore`
- `RosterStore`
- `TaskStore` may remain an upstream trait, but no SQLite task schema is
  approved until the task model is explicitly designed explicitly around the
  Claude-code task schema.
- the paired doctor contract owns:
  - path resolution
  - openability
  - schema/bootstrap/migration readiness
  - bounded store findings
  - bounded task-store findings when the same SQLite root owns both domains

Within `MailStore`, the current approved durable shape is:
- `mail_messages` for immutable/authored message content
- `mail_message_states` for mutable mailbox state such as read, ack, expiry,
  and delete visibility

Within `RosterStore`, the current approved durable shape is:
- one canonical `team_roster` member table
- explicit member fields for `member_kind`, `harness`, `agent_type`, `model`,
  optional `recipient_pane_id`, and `metadata_json`
- no whole-roster JSON snapshot table
- no durable member `pid`

Built-in nudge override storage rule:
- the first concrete override-store implementation for `AD.21` lives in
  `atm-storage-rusqlite`
- the concrete table is `team_nudge_template_overrides`
- concrete columns are:
  - `team_name TEXT NOT NULL`
  - `template_kind TEXT NOT NULL`
  - `template_body TEXT NOT NULL`
  - `updated_at TEXT NOT NULL`
- the primary key is `(team_name, template_kind)`
- the concrete migration lands by extending
  `crates/atm-storage-rusqlite/src/shared_db.rs::DB_MIGRATIONS`
- higher layers must reach this data only through the accepted
  `NudgeTemplateOverrideStore` contract; no direct SQLite reads are allowed in
  `atm` or `atm-core`

Mail content/provenance rule:
- weak provenance round-trip fields are not part of the `MailStoreMessageRecord`
  contract
- if the SQLite implementation keeps ingest timing such as `recorded_at` for
  local health/reporting, that timing remains store-owned internal data rather
  than caller-supplied message content

Architectural rule:
- sharing one internal connection/transaction root is acceptable
- exposing one public god-interface is not

## 4. Migration And Transaction Boundary

`atm-storage-rusqlite` owns:

- opening/creating the SQLite database
- schema bootstrap and migration execution
- transaction begin/commit/rollback implementation
- enforcement of:
  - `journal_mode = WAL`
  - `foreign_keys = ON`
  - `busy_timeout = 5000ms`
  - explicit transactions for mutating operations

`atm-storage-rusqlite` does not own:

- deciding when the application should perform a command
- transport/runtime retry policy
- daemon lifecycle/shutdown behavior

## 5. Error Translation Boundary

`atm-storage-rusqlite` must translate raw SQLite failures into typed ATM store errors.

Rules:
- no raw SQLite error should leak across the public store boundary as the
  primary failure type
- ATM-owned `AtmErrorCode` remains the public code vocabulary
- the crate must not invent local ad hoc error-code strings
- connection open/configuration is not complete until `journal_mode = WAL`,
  `foreign_keys = ON`, and `busy_timeout = 5000ms` have all been enforced
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
- the dedicated blocking execution path must respect the approved SQLite handle
  budget of `1..=4`

## 7. Testability

`atm-storage-rusqlite` must be testable entirely in process.

Rules:
- no daemon process required
- no real socket transport required
- conformance tests should exercise the `atm-core` store traits
- tests may use temporary databases but should not rely on private
  implementation details when validating store-contract behavior
