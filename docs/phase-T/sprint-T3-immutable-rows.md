# Sprint T.3 Immutable Message Rows

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ bdac03c`
**PR target**: `develop`
**Status**: Planning

## Goal

Finish the mailbox hot-write contract by enforcing immutable `mail_messages`
rows, removing the pre-write probe, and using writer-owned row-count semantics
instead of conflict-driven row mutation.

## Deliverables

- remove the pre-write `SELECT 1 ...` probe from `MailStore::upsert_message`
- stop using `ON CONFLICT DO UPDATE` to mutate message payload fields on
  duplicate message keys
- move message insertion semantics to insert-first / rows-changed detection
- keep mutable live state in the appropriate state tables rather than in
  `mail_messages`
- reject known ATM-owned logical/schema violations before SQL submission so one
  invalid queued row does not poison unrelated rows in the same batch
- add regression tests for:
  - duplicate message rows preserving the original payload
  - one invalid row not failing unrelated rows in the same drained batch
  - successor / logical invariant rejection before SQL execution

## Key File Targets

- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`
- `crates/atm-rusqlite/src/tests.rs`
- `docs/atm-rusqlite/architecture.md`
- `docs/adr/ADR-ATM-RUSQLITE-002.md`

## Acceptance Criteria

- `mail_messages` rows are immutable after the initial insert
- the hot write path no longer performs the pre-write probe
- duplicate writes preserve the first payload and do not silently rewrite
  message content
- known logical/schema violations are rejected before SQL wherever the crate
  already owns the invariant

## QA Pointers

- `req-qa` must verify both absence and presence:
  - absence of the old probe and mutable-row conflict update
  - presence of insert-first immutable-row semantics in the hot path
- rust/service review should look closely at partial-batch isolation and
  transaction semantics

## Dependencies

- depends on `T.2`
- should be reviewed with `T.2` as one SQLite-correctness line even if they
  land as separate commits
