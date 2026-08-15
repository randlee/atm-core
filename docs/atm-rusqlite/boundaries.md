# ATM-Rusqlite Boundary Inventory

This document captures the concrete SQLite adapters for the approved store
line.

Current design assumption:
- concrete sqlite adapters stay private to this crate
- no external crate should depend on `atm-rusqlite` directly
- runtime composition must go through `atm-core` boundary traits/facades
- client crates such as `atm`, `atm-graft`, and future harness-specific clients
  must not depend on this crate directly
- after `AA.5`, `atm-daemon` is forbidden again as a direct dependent of the
  SQLite assembly, store, and shared-db state-root records; daemon callers
  reach SQLite only through `atm-runtime`

Canonical machine-readable boundary sources:
- [`boundaries/atm-storage-rusqlite/message-search-store-sqlite.toml`](../../boundaries/atm-storage-rusqlite/message-search-store-sqlite.toml)
- [`boundaries/atm-storage-rusqlite/analyst-query-store-sqlite.toml`](../../boundaries/atm-storage-rusqlite/analyst-query-store-sqlite.toml)
- [`boundaries/atm-rusqlite/mail-store-sqlite.toml`](../../boundaries/atm-rusqlite/mail-store-sqlite.toml)
- [`boundaries/atm-rusqlite/mail-store-doctor-sqlite.toml`](../../boundaries/atm-rusqlite/mail-store-doctor-sqlite.toml)
- [`boundaries/atm-rusqlite/task-store-sqlite.toml`](../../boundaries/atm-rusqlite/task-store-sqlite.toml)
- [`boundaries/atm-rusqlite/task-store-doctor-sqlite.toml`](../../boundaries/atm-rusqlite/task-store-doctor-sqlite.toml)
- [`boundaries/atm-rusqlite/roster-store-sqlite.toml`](../../boundaries/atm-rusqlite/roster-store-sqlite.toml)
- [`boundaries/atm-rusqlite/roster-store-doctor-sqlite.toml`](../../boundaries/atm-rusqlite/roster-store-doctor-sqlite.toml)
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

## SqliteMessageSearchStore

The private adapter compiles only `atm-storage` typed search DTOs to FTS5 and
JSON1, maintains external-content projections in the same transaction as
canonical rows, and owns the bounded SQLite reader lane. No HTTP/runtime crate
opens a direct SQLite search connection.

AA.5 relock note:
- `cargo test --package atm-architecture` is the second enforcement layer that
detects policy widening and any reintroduced `atm-daemon -> atm-rusqlite`
code edge before review closure

## SqliteAnalystQueryStore

The private analyst adapter is the only direct SQLite dependency of the local
`atm-query-python` Maturin facade. It owns the read-only connection authorizer
and query budgets. Daemon, HTTP runtime, CLI, and graft crates remain forbidden
dependents; they use the typed runtime search port instead.

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

## SqliteMailStoreAdapter

Purpose:
- Own the SQLite-backed implementation of the `MailStore` contract.

Notes:
- Caller crates should know only the `MailStore` trait, never this concrete
  type.

## SqliteMailStoreDoctorAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-rusqlite/mail-store-doctor-sqlite.toml](../../boundaries/atm-rusqlite/mail-store-doctor-sqlite.toml)

Purpose:
- Own the SQLite-backed implementation of the `MailStoreDoctor` diagnostics
  contract.

## SqliteTaskStoreAdapter

Purpose:
- Historical SQLite implementation surface for the `TaskStore` contract.

Notes:
- Task persistence is not an approved SQLite schema line today.
- The trait may remain upstream as a contract placeholder, but this crate must
  not grow or preserve an unapproved durable task schema.

## SqliteTaskStoreDoctorAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-rusqlite/task-store-doctor-sqlite.toml](../../boundaries/atm-rusqlite/task-store-doctor-sqlite.toml)

Purpose:
- Own the SQLite-backed implementation of the `TaskStoreDoctor` diagnostics
  contract.

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

## SqliteRosterStoreDoctorAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-rusqlite/roster-store-doctor-sqlite.toml](../../boundaries/atm-rusqlite/roster-store-doctor-sqlite.toml)

Purpose:
- Own the SQLite-backed implementation of the `RosterStoreDoctor` diagnostics
  contract.
