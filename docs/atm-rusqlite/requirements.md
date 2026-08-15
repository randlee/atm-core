# ATM-Rusqlite Crate Requirements

## 1. Purpose

This document defines the `atm-storage-rusqlite` crate requirements.

The `atm-storage-rusqlite` crate owns the first concrete SQLite implementation of the
durable store boundaries defined by `atm-core`.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

## 2. Ownership

`atm-storage-rusqlite` owns:

- concrete `rusqlite`-backed implementations of:
  - `MailStore`
  - `RosterStore`
- SQLite connection/bootstrap wiring
- schema migrations/bootstrap execution
- transaction execution inside the concrete store implementation
- SQLite-specific translation into typed ATM store errors

`atm-storage-rusqlite` does not own:

- workflow/state-machine business logic
- CLI parsing/rendering
- daemon transport/runtime logic
- inbox JSONL parsing or writing
- agent notification delivery
- daemon live-status truth
- approved durable task semantics; that line remains unresolved until the task
  model is designed explicitly

## 3. Requirement Namespace

The `atm-storage-rusqlite` crate uses the `REQ-RUSQLITE-*` namespace.

Initial allocation:

- `REQ-RUSQLITE-STORE-*`
- `REQ-RUSQLITE-MIGRATION-*`
- `REQ-RUSQLITE-ERROR-*`
- `REQ-RUSQLITE-TEST-*`
- `REQ-RUSQLITE-TEMPLATE-WORKFLOW-*`

Initial crate requirement IDs:

- `REQ-RUSQLITE-STORE-001` `atm-storage-rusqlite` must implement the
  approved `MailStore` and `RosterStore` contracts without widening those
  interfaces. The `TaskStore` trait may remain defined upstream, but SQLite
  task persistence is not an approved schema line at this time. Satisfies:
  `REQ-CORE-RUNTIME-001`, `REQ-CORE-STORE-001`, `REQ-CORE-STORE-002`.
- `REQ-RUSQLITE-MIGRATION-001` `atm-storage-rusqlite` must own deterministic schema
  bootstrap and migration execution. Satisfies:
  `REQ-CORE-STORE-001`, `REQ-CORE-STORE-002`.
- `REQ-RUSQLITE-ERROR-001` `atm-storage-rusqlite` must translate SQLite failures into
  typed ATM store errors with stable `AtmErrorCode` mapping. Satisfies:
  `REQ-CORE-BOUNDARY-002`.
- `REQ-RUSQLITE-TEST-001` `atm-storage-rusqlite` must be testable in process without
  requiring daemon or real socket runtime. Satisfies:
  `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-RUSQLITE-TEMPLATE-WORKFLOW-001` `atm-storage-rusqlite` owns the
  additive, deterministic migration and single-transaction persistence of a
  decomposed message's template-declared workflow snapshot and tag
  provenance. It must preserve pre-extension rows, never reconstruct facts
  from current catalog state, and expose only the storage capability defined
  by `atm-core`; SQLite JSON/FTS/index choices remain private. Satisfies:
  `REQ-CORE-TEMPLATE-WORKFLOW-001`,
  `REQ-CORE-WORKFLOW-ANALYTICS-001`,
  `REQ-P-TEMPLATE-WORKFLOW-001`, `REQ-P-TEMPLATE-TAGS-001`, and
  `REQ-P-WORKFLOW-ANALYTICS-001` per ADR-046.
- `REQ-RUSQLITE-SEARCH-INDEX-001` `atm-storage-rusqlite` owns the private,
  additive desired-state ledger and single-writer idle drain for recoverable
  FTS projections. Canonical source mutation and work enqueue must be one
  transaction; drain completion and ledger removal must be one transaction;
  restart must resume unfinished work; foreground writer work always wins.
  The crate exposes only the backend-neutral read-only freshness status
  authorized by the amended sealed search capability. It adds no second
  SQLite owner, daemon, or public maintenance trait. Satisfies
  `REQ-P-SEARCH-INDEX-001` and `REQ-CORE-SEARCH-INDEX-001` per ADR-047.

## 4. Required References

The `atm-storage-rusqlite` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-R.md`](../plan-phase-R.md)
- [`../plan-phase-S.md`](../plan-phase-S.md)
- [`../plan-phase-U.md`](../plan-phase-U.md)
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
- only `atm-storage-rusqlite` may own direct `rusqlite` calls in the first
  implementation line
- concrete SQLite details remain private to this crate
- callers depend on `atm-core` store traits, not on `rusqlite` types
- the default production durable database path is `~/.atm/db/mail.db`
- the host-scoped SQLite database is one shared durable store keyed by team
  and agent, not one database per team
- the daemon is the only ATM-owned writer to the production database
- read-only consumers may query SQLite directly as a supported integration
  surface, but ATM-owned writes must still go through the documented runtime
  and store boundaries
- the current runtime composition owner may depend on this crate in order to
  assemble production adapters, but thin callers and extension crates must not
- Phase-AA target direction:
  - the long-term concrete composition owner is `atm-runtime`, not
    `atm-daemon`
  - `AA.2` lands that production composition transfer; any remaining direct
    daemon references stay transition-scoped only until `AA.4` removes the
    direct daemon assembly/test imports and `AA.5` relocks the final machine
    boundary
  - daemon crates must not retain direct SQLite assembly dependencies after
    the Phase AA simplification line closes
  - runtime-owned adapters surface SQLite-specific readiness through the
    subsystem-owned doctor traits rather than promoting those report shapes
    into `atm-storage-rusqlite`
  - speculative SQLite task-store code is not part of the approved AC scope
- schema bootstrap must be deterministic and idempotent
- schema bootstrap must run once per database root before normal store
  operations, not on every connection acquisition
- WAL / foreign-key / explicit-transaction policy must be enforced here
- `MailStore` and `RosterStore` may share one internal SQLite
  root object, but they must not collapse into one public god-interface
- the durable schema must expose:
  - one concrete message table with queryable identity/timestamp columns plus
    full-envelope JSON
  - one explicit mutable message-state table for read/ack/expiry/delete state;
    split `ack_state` / `mail_visibility_states` storage is not permitted
  - one canonical roster/member store with explicit behavioral fields such as
    `member_kind` and `harness`
  - one team-scoped built-in nudge template override table named
    `team_nudge_template_overrides` with:
    - `team_name TEXT NOT NULL`
    - `template_kind TEXT NOT NULL`
    - `mode TEXT NOT NULL`
    - `template_body TEXT NOT NULL`
    - `updated_at TEXT NOT NULL`
    - primary key `(team_name, template_kind)`
    - template-kind values constrained to the six built-in template kinds
      accepted in `AD.21`
    - `mode` values constrained to `override` or `disabled`
    - reset-to-default modeled as row deletion, not as a third persisted mode
- weak provenance round-trip fields such as `imported_from` must not be part
  of the enduring `MailStoreMessageRecord` contract
- if ingest timing is retained for health/reporting, it must be store-owned
  internal data rather than caller-supplied message metadata
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
- most SQLite tests should use dedicated in-memory fixtures with explicit
  setup/cleanup
- only a small deliberate suite may use on-disk temporary databases for
  reopen, migration, and filesystem-behavior verification
- tests must never use or mutate the production durable root under
  `~/.atm/db/mail.db`
- every SQLite schema change must be treated as a contract change and requires
  explicit user approval plus synchronized requirements/architecture/boundary
  doc updates before landing
- the `team_nudge_template_overrides` schema step is owned by
  `atm-storage-rusqlite` and must land by extending
  `crates/atm-storage-rusqlite/src/shared_db.rs::DB_MIGRATIONS`; because this
  crate centralizes bootstrap SQL in that constant today, no separate SQL file
  is the accepted migration shape unless the migration architecture changes
