# ATM-Rusqlite Boundary Inventory

This document captures the concrete SQLite adapters for Phase R.

Current design assumption:
- concrete sqlite adapters stay private to this crate
- no external crate should depend on `atm-rusqlite` directly
- any future runtime composition must go through boundary traits/facades rather
  than a direct daemon-to-sqlite crate edge

Canonical machine-readable boundary sources:
- [`boundaries/atm-rusqlite/mail-store-sqlite.toml`](../../boundaries/atm-rusqlite/mail-store-sqlite.toml)
- [`boundaries/atm-rusqlite/task-store-sqlite.toml`](../../boundaries/atm-rusqlite/task-store-sqlite.toml)
- [`boundaries/atm-rusqlite/roster-store-sqlite.toml`](../../boundaries/atm-rusqlite/roster-store-sqlite.toml)
- [`boundaries/atm-rusqlite/sqlite-boundary-assembly.toml`](../../boundaries/atm-rusqlite/sqlite-boundary-assembly.toml)
- [`boundaries/atm-rusqlite/shared-db.toml`](../../boundaries/atm-rusqlite/shared-db.toml)

Important crate-private assembly/state-root structs that must stay visible in
review:
- `SqliteBoundaryAssembly`
  - owns composition of the three store adapters over one shared SQLite root
- `SharedDb`
  - owns connection/bootstrap/transaction policy for the shared host-scoped
    database

These are not public boundary traits, but they are important private
implementation surfaces for `R.14` and later closeout review.

## SqliteBoundaryAssembly

Canonical machine-readable boundary source:
- [../../boundaries/atm-rusqlite/sqlite-boundary-assembly.toml](../../boundaries/atm-rusqlite/sqlite-boundary-assembly.toml)

Purpose:
- Own the crate-private assembly seam that composes the three SQLite-backed
  boundary adapters over one shared host-scoped database root.

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
- The owned production path resolves to `~/.atm/db/mail.db` and must not be
  derived from `ATM_HOME` or test-only compatibility state roots.
- The owned state-root policy includes:
  - one-time deterministic schema bootstrap per database root
  - per-connection runtime pragma enforcement
  - concurrent open-handle budget capped at 4 SQLite connections
  - no full migration/bootstrap rerun on every connection acquisition

## SqliteMailStoreAdapter

Purpose:
- Owns the SQLite-backed implementation of the MailStore contract.

Notes:
- Caller crates should know only the MailStore trait, never this concrete type.

## SqliteTaskStoreAdapter

Purpose:
- Owns the SQLite-backed implementation of the TaskStore contract.

Notes:
- This remains separate from mail and roster persistence to preserve ownership clarity.

## SqliteRosterStoreAdapter

Purpose:
- Owns the SQLite-backed implementation of the RosterStore contract.

Notes:
- Thin extensions such as atm-graft must not depend on this crate directly.
- The concrete implementation currently maintains both:
  - a crate-private `rosters` snapshot table as the canonical `TeamConfig`
    round-trip record
  - a per-member `team_roster` durable projection for runtime-facing lookup
