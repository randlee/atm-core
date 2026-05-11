# Sprint T.2 SQLite Single-Writer Lane

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ 1232244`
**PR target**: `develop`
**Status**: Implementation Complete

## Goal

Implement the real crate-private SQLite writer lane promised by
`ADR-ATM-RUSQLITE-002` and the S.15 planning docs so the hot mailbox write path
no longer depends on ad-hoc per-operation write transactions.

## Preconditions

- `ADR-ATM-RUSQLITE-002` must be accepted before implementation begins
- this sprint owns the ADR update needed to move that ADR from `proposed` to
  an accepted design decision with the final writer-lane contract recorded

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
- audit every surviving `SharedDb::with_transaction(...)` caller and classify it
  explicitly as:
  - read-only / not a migration target
  - cold-path write allowed to stay on `with_transaction(...)`
  - incorrectly left on the legacy path and therefore still in scope for T.2
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
- any doc or audit note that records the `SharedDb::with_transaction(...)`
  caller classification

## Acceptance Criteria

- the writer lane exists in code, not only in docs
- the hot-path write calls actually flow through the writer lane
- the `SharedDb::with_transaction(...)` caller audit is explicit and complete
- the worker queue, batch drain, and shutdown path are bounded and test-covered
- the crate does not widen the writer surface beyond `pub(crate)` ownership
- async daemon callers do not block Tokio worker threads directly when
  submitting writes

## QA Pointers

- `req-qa` must verify the named writer modules exist and that the hot-path
  methods route through them, and that the `with_transaction(...)` caller audit
  is complete
- `arch-qa` should verify the writer remains crate-private and does not loosen
  store boundaries
- hardening review should focus on queue bounds, batch semantics, and shutdown
  behavior under load

## Dependencies

- may start independently of `T.4` and `T.5`
- `T.3` depends on this sprint landing the writer-owned path first
