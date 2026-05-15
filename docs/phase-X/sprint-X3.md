---
id: X.3
title: Daemon Runtime Truth Unification
status: planned
branch: feature/pXb-s3-runtime-truth-unification
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pXb-s3-runtime-truth-unification
target: integrate/phase-Xb
---

# Sprint X.3 — Daemon Runtime Truth Unification

## Modification

- This sprint is a restart replay on `feature/pXb-s3-runtime-truth-unification`.
- Prior Phase `X` already has one clean implementation candidate for the core
  `X.3` work:
  - `9264c3e8c480a4262a23ee0fd1ffaa144cd5f38f`
    - `feat: unify daemon runtime truth on roster store`
- The old `feature/pX-s3-runtime-truth-unification` branch ref is contaminated
  by later sprint merges and must not be treated as the replay source; salvage
  only the audited `9264c3e...` delta.
- QA must validate the entire `X.3` sprint on `pXb-s3`, not only the replayed
  delta from that commit.

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
- `crates/atm-rusqlite/src/lib.rs`
- any roster-store implementation files touched by the new enumeration boundary
- `crates/atm-daemon/src/composition.rs`
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

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
