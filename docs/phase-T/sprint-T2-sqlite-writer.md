# Sprint T.2 SQLite Single-Writer Lane

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ bdac03c`
**PR target**: `develop`
**Status**: Planning

## Goal

Implement the real crate-private SQLite writer lane promised by
`ADR-ATM-RUSQLITE-002` and the S.15 planning docs so the hot mailbox write path
no longer depends on ad-hoc per-operation write transactions.

## Deliverables

- add a crate-private `SqliteWriter` implementation with:
  - one dedicated writer thread
  - one long-lived write `Connection`
  - one bounded submission queue
  - one bounded batch-drain loop
  - one explicit shutdown/drain path
- add the writer module tree promised by the plan:
  - `crates/atm-rusqlite/src/writer/mod.rs`
  - `crates/atm-rusqlite/src/writer/ops.rs`
  - `crates/atm-rusqlite/src/writer/stmt_cache.rs`
- migrate the hot-path write entry points through the writer-owned lane:
  - `MailStore::upsert_message`
  - `MailStore::upsert_visibility_state`
- preserve `SharedDb::with_transaction(...)` for remaining cold-path callers
  during the incremental migration
- define the bounded backpressure contract for blocking submitters and async
  daemon call sites

## Key File Targets

- `crates/atm-rusqlite/src/shared_db.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/writer/mod.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`
- `crates/atm-rusqlite/src/writer/stmt_cache.rs`
- `docs/atm-rusqlite/architecture.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/adr/ADR-ATM-RUSQLITE-002.md`

## Acceptance Criteria

- the writer lane exists in code, not only in docs
- the hot-path write calls actually flow through the writer lane
- the worker queue, batch drain, and shutdown path are bounded and test-covered
- the crate does not widen the writer surface beyond `pub(crate)` ownership
- async daemon callers do not block Tokio worker threads directly when
  submitting writes

## QA Pointers

- `req-qa` must verify the named writer modules exist and that the hot-path
  methods route through them
- `arch-qa` should verify the writer remains crate-private and does not loosen
  store boundaries
- hardening review should focus on queue bounds, batch semantics, and shutdown
  behavior under load

## Dependencies

- may start independently of `T.4` and `T.5`
- `T.3` depends on this sprint landing the writer-owned path first
