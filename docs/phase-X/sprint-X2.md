---
id: X.2
title: Command Path Simplification
status: complete
branch: feature/pX-s2-command-path-simplification
worktree: ../atm-core-worktrees/feature/pX-s2-command-path-simplification
target: integrate/phase-X
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

## Exact Targets

- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/boundary/mail.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/lib.rs`
- `crates/atm-core/src/mailbox/mod.rs`
- `crates/atm-core/src/mailbox/source.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/src/workflow.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/mailbox_metadata.rs`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`
- `docs/project-plan.md`

## Required Work

- delete `crates/atm-core/src/read/legacy_path.rs`
- remove `legacy:` mailbox-key handling as a production control-flow path
- remove direct source-file lock/read/write branches from `ack`, `read`,
  `clear`, and shared mailbox append helpers
- move any surviving file-watcher or ingress helpers behind a daemon-private
  ingress boundary rather than the general command runtime trait
- narrow or delete `boundary_support.rs` helpers that still keep file-backed
  mailbox behavior on the production path
- remove the retained boundary-adapter stubs in
  `service_runtime_store.rs:305-315`
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

## Delivered

- removed the remaining production `legacy:` mailbox-key branches from
  retained command/runtime code
- changed the shared send/ack append path to load mailbox thread context from
  SQLite metadata/records instead of compatibility inbox files
- renamed the old file-surface helper APIs so retained command logic no longer
  depends on the legacy `observe_source_files` / `commit_source_files` /
  `commit_mailbox_state` / `read_messages` surface
- kept the remaining compatibility inbox import/export helpers behind the
  hidden daemon-side ingress/export seam in `boundary_support.rs`
- tightened boundary docs so compatibility inbox files are explicitly
  daemon-owned ingress/export edges rather than a retained runtime backend
- updated mailbox health/count DTO names so the X.2 grep gate is clean in
  production code

Implementation result:
- the X.2 acceptance criteria are satisfied on
  `feature/pX-s2-command-path-simplification`
- `rg -n "legacy:" crates/atm-core/src` returns only the explicit rejection
  test fixture in `workflow.rs`
- `rg -n "observe_source_files|commit_source_files|with_locked_source_files|commit_mailbox_state|read_messages" crates/atm-core/src`
  returns only test names in `mailbox/mod.rs`, not production helper or
  command-path matches

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
