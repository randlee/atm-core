# Sprint S.14 — SQLite Write-Worker Plan

**Branch**: feature/pS-s15-rusqlite-hardening  
**Base**: integrate/phase-S @ 77badd5  
**PR target**: integrate/phase-S  
**Status**: Planning

## Goal

Produce the implementation plan for the next `atm-rusqlite` hardening pass.
The focus is write-path serialization and batching: remove avoidable SQLite
write contention, cut the hot-path `SELECT` probe from mailbox appends, and
define a dedicated writer design that preserves the current `atm-core` store
contracts.

This sprint is planning-only. It must critically review the Opus
write-worker proposal and adopt only the parts that fit the current crate
contracts, requirements, and durability model.

## Required Work

### 1. Write the design document

Add `docs/phase-S/sprint-S14-rusqlite-plan.md` covering:
- current `atm-rusqlite` write-path shape and hot spots
- explicit review of the Opus recommendations: what is adopted as-is, what is
  modified, and what is deferred or rejected
- the `SqliteWriter` actor design
- `SharedDb` integration and connection-budget changes
- migration order with no flag-day cutover
- hot-path `INSERT OR IGNORE` conditions and invariants
- test and validation requirements
- follow-up risks that are out of scope for S.14 implementation

### 2. Record the sprint authority

This sprint brief must remain aligned with the design document and the current
crate architecture:
- no public writer exposure across crate boundaries
- no widening of `MailStore`, `TaskStore`, or `RosterStore`
- no Tokio runtime dependency added to `atm-rusqlite`
- no benchmark claims used as acceptance criteria without measurements
- no heartbeat writer shortcut added unless a real crate write surface exists

### 3. Record the implementation scope for the follow-on fix sprint

The follow-on implementation worktree should target:
- `crates/atm-rusqlite/src/shared_db.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/writer/mod.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`
- `crates/atm-rusqlite/src/writer/stmt_cache.rs`
- targeted `atm-rusqlite` tests

The follow-on implementation should not:
- expose a public `SqliteWriter` type
- force a flag-day migration of every write call site at once
- rewrite reader query paths into the writer actor
- add a synthetic `submit_record_heartbeat` shortcut for a write surface that
  does not exist in the current crate

## Acceptance Criteria

- `docs/phase-S/sprint-S14.md` exists and names the implementation scope and
  acceptance criteria
- `docs/phase-S/sprint-S14-rusqlite-plan.md` exists and covers all required
  design areas
- `docs/adr/ADR-ATM-RUSQLITE-002.md` exists as an ADR stub or full ADR
- no implementation code changes are made on this planning branch
- `just lint` PASS

Implementation-sprint acceptance:

- `MailStore::upsert_message` and `MailStore::upsert_visibility_state` route
  through `SqliteWriter`
- `SharedDb::with_transaction(...)` keeps its current callable shape for
  remaining cold-path callers while the migrated hot-path writes use typed
  writer submissions
- the mailbox append hot path removes the pre-write `SELECT` probe and uses
  row-count-based insertion detection instead
- the write worker drains pending queued writes on shutdown before returning
- regression tests cover:
  - single-writer correctness
  - batch drain behavior
  - closed-channel drain-on-shutdown
- `cargo fmt --all --check` PASS
- `cargo test -p atm-rusqlite` PASS
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc` PASS
- `just lint` PASS

## References

- `docs/phase-S/sprint-S14-rusqlite-plan.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/architecture.md`
- `docs/adr/ADR-ATM-RUSQLITE-002.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/shared_db.rs`
- `TASK-S15-ARCH`
