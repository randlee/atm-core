# ATM-Rusqlite Boundary Inventory

This document captures the concrete SQLite adapters for the approved store
line.

Current design assumption:
- concrete sqlite adapters stay private to this crate
- no external production client crate should depend on `atm-rusqlite` directly
- runtime composition must go through `atm-core` boundary traits/facades
- client crates such as `atm`, `atm-graft`, and future harness-specific clients
  must not depend on this crate directly
- the only approved direct dependents outside `atm-daemon` are:
  - `atm-daemon-bootstrap` for installing the default retained-runtime factory
  - `atm-runtime-test-support` for cross-crate SQLite runtime test helpers

Canonical machine-readable boundary sources:
- [`boundaries/atm-rusqlite/mail-store-sqlite.toml`](../../boundaries/atm-rusqlite/mail-store-sqlite.toml)
- [`boundaries/atm-rusqlite/task-store-sqlite.toml`](../../boundaries/atm-rusqlite/task-store-sqlite.toml)
- [`boundaries/atm-rusqlite/roster-store-sqlite.toml`](../../boundaries/atm-rusqlite/roster-store-sqlite.toml)
- [`boundaries/atm-rusqlite/sqlite-boundary-assembly.toml`](../../boundaries/atm-rusqlite/sqlite-boundary-assembly.toml)
- [`boundaries/atm-rusqlite/shared-db.toml`](../../boundaries/atm-rusqlite/shared-db.toml)

Important crate-private assembly/state-root structs that must stay visible in
review:
- `SqliteBoundaryAssembly`
  - owns composition of the approved SQLite store adapters over one shared
    SQLite root
- `SharedDb`
  - owns connection/bootstrap/transaction policy for the shared host-scoped
    database

These are not public boundary traits, but they are important private
implementation surfaces for review.

## SqliteBoundaryAssembly

Canonical machine-readable boundary source:
- [../../boundaries/atm-rusqlite/sqlite-boundary-assembly.toml](../../boundaries/atm-rusqlite/sqlite-boundary-assembly.toml)

Purpose:
- Own the crate-private assembly seam that composes the SQLite-backed boundary
  adapters over one shared host-scoped database root.

Notes:
- This record exists so the assembly seam remains review-visible even though it
  is not a public cross-crate trait.
- The production assembly path must resolve the host-scoped durable root via
  one crate-owned default entry point rather than by leaking path ownership to
  callers.
- `atm-daemon-bootstrap` may use the default runtime assembly entrypoint to
  install the production retained-runtime factory without taking ownership of
  SQLite policy.
- `atm-runtime-test-support` may use the assembly seam only for test-only
  cross-crate runtime harnessing and lock-contention helpers.

## SharedDbStateRoot

Canonical machine-readable boundary source:
- [../../boundaries/atm-rusqlite/shared-db.toml](../../boundaries/atm-rusqlite/shared-db.toml)

Purpose:
- Own the crate-private SQLite bootstrap, connection-open, and transaction
  policy for the shared host-scoped database root.

Notes:
- This record exists so the host-scoped durable-state root and transaction
  policy are enforced as a private boundary surface rather than informal crate
  internals.
- The owned production path resolves to `~/.atm/db/mail.db`.
- Schema changes are lock-step architectural changes and require explicit user
  approval plus matching updates to requirements, architecture, and boundary
  docs before implementation is accepted.
- Test-only support may hold writer locks through this crate so cross-crate
  integration tests can exercise SQLite contention without reimplementing the
  underlying transaction policy.

## SqliteMailStoreAdapter

Purpose:
- Own the SQLite-backed implementation of the `MailStore` contract.

Notes:
- Caller crates should know only the `MailStore` trait, never this concrete
  type.

## SqliteTaskStoreAdapter

Purpose:
- Historical SQLite implementation surface for the `TaskStore` contract.

Notes:
- Task persistence is not an approved SQLite schema line today.
- The trait may remain upstream as a contract placeholder, but this crate must
  not grow or preserve an unapproved durable task schema.

## SqliteRosterStoreAdapter

Purpose:
- Own the SQLite-backed implementation of the `RosterStore` contract.

Notes:
- The approved target is one canonical member store, not a whole-roster JSON
  snapshot plus a second per-member truth table.
- Explicit behavioral member fields belong in the canonical roster store:
  - `member_kind`
  - `harness`
  - `agent_type`
  - `model`
  - `metadata_json`
- Durable roster truth must not carry daemon-owned `pid` continuity.
