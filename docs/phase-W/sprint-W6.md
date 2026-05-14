---
id: W.6
title: SQLite Error Contract Cleanup
status: completed
branch: feature/pW-s6-sqlite-error-contract
worktree: ../atm-core-worktrees/feature/pW-s6-sqlite-error-contract
---

# Sprint W.6 — SQLite Error Contract Cleanup

## Goal

- close the remaining Phase `W` design-review mismatches around SQLite
  degradation projection and typed daemon event metadata

## Design Decisions

- `DESIGN-002-W`
  - SQLite degradation state must be recorded before retained-sink emission so
    `atm doctor` and runtime-health remain authoritative even when the sink is
    degraded
- `DESIGN-003-W`
  - daemon runtime degradation caused by SQLite must project through a
    dedicated ATM warning code, not the observability-health warning code
- `DESIGN-004-W`
  - `DaemonEvent` stores typed validated action/outcome labels instead of raw
    string fields

## Files Changed

- `crates/atm-core/src/error_codes.rs`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-daemon/src/daemon_runtime_observability.rs`
- `crates/atm-daemon/src/sqlite_observability.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm-daemon/src/test_observability.rs`
- `crates/atm-daemon/bin_support/daemon_observability.rs`
- `docs/phase-W/sprint-W6.md`

## Acceptance Criteria

- SQLite degradation is recorded in runtime status before retained-sink emit
  succeeds or fails
- degraded SQLite doctor/runtime-health findings use a dedicated SQLite ATM
  warning code
- `DaemonEvent` action/outcome fields are typed validated labels
- `cargo build --workspace` passes
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` passes
