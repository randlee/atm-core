# Sprint T.3 Immutable Message Rows

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ 1232244`
**PR target**: `develop`
**Status**: Active

## Goal

Finish the mailbox hot-write contract by enforcing immutable `mail_messages`
rows, removing the pre-write probe, and using writer-owned row-count semantics
instead of conflict-driven row mutation.

Ownership note:
- `T.2` owns the main `ADR-ATM-RUSQLITE-002` writer-lane design update
- `T.3` owns only the immutable-row addendum and the hot-path semantic
  completion on top of the accepted `T.2` writer implementation

Pre-QA dependency:
- `crates/atm-rusqlite/src/writer/ops.rs` is a `T.2` deliverable
- `T.3` QA must not proceed until that file exists on the accepted `T.2` base

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
- `crates/atm-rusqlite/src/writer/mod.rs`
- `docs/atm-rusqlite/architecture.md`
- `docs/adr/ADR-ATM-RUSQLITE-002.md`

## Acceptance Criteria

- `mail_messages` rows are immutable after the initial insert
- the hot write path no longer performs the pre-write probe
- duplicate writes preserve the first payload and do not silently rewrite
  message content
- `WriteOpResult::UpsertMessage { inserted: bool }` reports `true` on the
  first insert and `false` on duplicate-key no-op replays
- rows-changed detection (`rows_changed()`) is used to determine
  `inserted=true/false` rather than a pre-write probe or conflict-driven
  mutation
- known logical/schema violations are rejected before SQL wherever the crate
  already owns the invariant
- invalid rows are rejected before SQL submission and valid rows in the same
  drained batch are unaffected

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
