---
id: X.3
title: Daemon Runtime Truth Unification
status: complete
branch: feature/pX-s3-runtime-truth-unification
worktree: ../atm-core-worktrees/feature/pX-s3-runtime-truth-unification
target: integrate/phase-X
---

# Sprint X.3 — Daemon Runtime Truth Unification

## Goal

- make daemon runtime team/member truth come from one explicit ownership model
- close the hybrid filesystem-plus-SQLite runtime-state assembly

## Hard Dependencies

- `X.0` merged on `develop`
- `X.1` and `X.2` complete because daemon/runtime truth should be unified after
  mailbox SSOT cutover and command-path simplification are in place

## Exact Targets

- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-rusqlite/src/lib.rs`
- any roster-store implementation files touched by the new enumeration boundary
- `crates/atm-daemon/src/composition.rs`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-daemon/architecture.md`

## Required Work

- add the boundary operation needed to enumerate daemon teams from the
  authoritative store path
- remove filesystem enumeration of `ATM_HOME/.claude/teams` from
  `build_runtime_status_cache_state(...)`
- make `build_runtime_status_cache_state(...)` explicitly SQLite-owned for both
  team discovery and member discovery
- keep `evict_status_cache_entry_if_needed()` inside the named refactor surface
  so helper extraction does not silently change bounded-cap eviction behavior
- refactor `build_runtime_status_cache_state(...)` below the RULE-002 `80`-line
  limit
- treat the `runtime_health.rs:47-110` shutdown-finalizer registry as out of
  scope unless the runtime-truth rewiring forces a lifecycle integration change
- update daemon boundary/architecture docs so runtime health no longer implies
  direct filesystem discovery

## Acceptance Criteria

- no `read_dir(.../.claude/teams...)` based team discovery remains in
  `build_runtime_status_cache_state(...)`
- daemon runtime status assembly uses one explicit roster/store source for both
  team and member truth
- `build_runtime_status_cache_state(...)` is under `80` lines
- doctor/runtime-health still surface SQLite unavailability and degraded state
  with the existing shared ATM error contract

## Delivered

- added `RosterStore::list_teams(...)` as the explicit boundary operation for
  daemon runtime team discovery
- updated the SQLite roster-store adapter to enumerate canonical persisted team
  names in sorted order and added direct coverage for that boundary behavior
- removed `ATM_HOME/.claude/teams` enumeration from
  `build_runtime_status_cache_state(...)`
- refactored runtime-status hydration so the daemon now builds team/member truth
  only from the installed roster-store boundary plus preserved in-memory live
  state
- kept the shutdown-finalizer registry unchanged while rewiring only the
  runtime-truth assembly path
- updated both `atm-core` and `atm-daemon` boundary/architecture docs so
  roster truth now explicitly includes daemon runtime team discovery
- updated the `atm-core` doctor-only roster-store test double to match the
  expanded sealed boundary contract

Implementation result:
- the X.3 acceptance criteria are satisfied on
  `feature/pX-s3-runtime-truth-unification`
- `build_runtime_status_cache_state(...)` no longer uses `read_dir(...)` or
  `home_dir`
- daemon runtime-status hydration now has one explicit durable team/member
  source: `RosterStore`

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
