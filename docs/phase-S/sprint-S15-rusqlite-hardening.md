# Sprint S.15 Rusqlite Hardening

**Branch**: `feature/pS-s15-rusqlite-hardening`  
**Base**: `integrate/phase-S @ 77badd5`  
**PR target**: `integrate/phase-S`  
**Status**: Implementation

## Goal

Implement the S.15 rusqlite write-worker plan from
`docs/phase-S/sprint-S15-rusqlite-plan.md` and land the first bounded
single-writer migration for the hot mailbox write path.

## Scope

This sprint hardens:
- mailbox hot-path write serialization
- bounded batching with per-op isolation inside one writer-owned transaction lane
- preflight validation for known logical/schema violations before SQL execution
- incremental migration of existing `SharedDb` write callers without a flag-day cutover
- writer shutdown/drain behavior and regression coverage

## Required Code Targets

- `crates/atm-rusqlite/src/shared_db.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/writer/mod.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`
- `crates/atm-rusqlite/src/writer/stmt_cache.rs`
- targeted `atm-rusqlite` tests

## Closeout Requirements

- keep `SqliteWriter` crate-private and avoid widening `MailStore`, `TaskStore`, or `RosterStore`
- migrate `MailStore::upsert_message` and `MailStore::upsert_message_state` through the writer
- preserve `SharedDb::with_transaction(...)` for remaining cold-path callers during incremental migration
- remove the mailbox append pre-write `SELECT` probe and derive insertion from row-count semantics
- ensure one invalid queued write row does not fail unrelated rows in the same batch
- reject known ATM-owned logical/schema invariant violations before SQL rather than on the hot path
- preserve `REQ-P-RUNTIME-002` by keeping the daemon singleton as the only writer-process invariant
- drain pending queued writes on writer shutdown before returning

## Validation

- `cargo test -p atm-rusqlite`
- `cargo test -p atm-daemon`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `just lint`
- `cargo fmt --all --check`
- `git diff --check`

## References

- `docs/phase-S/sprint-S15.md`
- `docs/phase-S/sprint-S15-rusqlite-plan.md`
- `docs/plan-phase-S.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/architecture.md`
- `docs/adr/ADR-ATM-RUSQLITE-002.md`
