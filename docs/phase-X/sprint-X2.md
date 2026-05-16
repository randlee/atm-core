---
id: X.2
title: Command Path Simplification
status: replayed
branch: feature/pXb-s2-command-path-simplification
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pXb-s2-command-path-simplification
target: integrate/phase-Xb
---

# Sprint X.2 — Command Path Simplification And Legacy Path Deletion

## Goal

- delete the remaining file-backed mailbox command branches
- reduce runtime-layer branching so command behavior routes through one
  store-backed path only

## Hard Dependencies

- `X.0` merged on `develop`
- `X.1` complete because this sprint assumes the dual runtime selection has
  already been removed

## Replay Status

- replayed on `feature/pXb-s2-command-path-simplification` at `e508ecb`
- replayed from prior Phase `X` salvage commit:
  - `0580c0e`
- `quality-mgr` alignment review already confirmed the replayed branch matches
  the intended sprint deliverables at a non-critical level

## Already Complete On Restart Branch

- the legacy command-path deletion work is replayed onto the restart branch
- the replayed branch already carries the primary file deletions and
  simplification of command-path control flow away from file-backed mailbox
  branches

## Remaining Restart Work

- split `crates/atm-core/src/clear/mod.rs:clear_mail_with_runtime_impl` below
  the RULE-002 `80`-line limit
- replace the fixed sleeps in
  `crates/atm-core/tests/mailbox_locking.rs` lines `1011` and `1041` with a
  bounded synchronization approach that passes the fixed-sleep lint
- run full sprint QA and validation on `feature/pXb-s2-command-path-simplification`
- keep these corrections on `X.2`; do not defer them to later branches because
  they are direct residuals in the command-path sprint surface

## Exact Targets

- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/read/legacy_path.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `docs/atm-core/boundaries.md`

## Required Work

- delete `crates/atm-core/src/read/legacy_path.rs`
- remove `legacy:` mailbox-key handling as a production control-flow path
- remove direct source-file lock/read/write branches from `ack`, `read`,
  `clear`, and shared mailbox append helpers
- move any surviving file-watcher or ingress helpers behind a daemon-private
  ingress boundary rather than the general command runtime trait
- narrow or delete `boundary_support.rs` helpers that still keep file-backed
  mailbox behavior on the production path
- remove any retained boundary-adapter stubs in `service_runtime_store.rs`
  that keep file-backed mailbox behavior reachable
- document any remaining source-file helper ownership as daemon-private
  ingress/migration-only scope

## Acceptance Criteria

- `crates/atm-core/src/read/legacy_path.rs` is removed
- `rg -n "legacy:" crates/atm-core/src` returns no production compatibility
  branch matches outside explicit test fixtures
- `rg -n "observe_source_files|commit_source_files|with_locked_source_files|commit_mailbox_state|read_messages" crates/atm-core/src`
  finds no production use outside explicitly retained daemon-private ingress or
  migration modules and tests
- command logic no longer branches on mailbox backend selection
- mailbox command behavior remains routed through one store-backed path

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
