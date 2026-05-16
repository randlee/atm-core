---
id: X.1
title: Mailbox Runtime Cutover
status: complete
branch: feature/pXb-s1-mailbox-runtime-cutover
worktree: ../atm-core-worktrees/feature/pXb-s1-mailbox-runtime-cutover
target: integrate/phase-X
---

# Sprint X.1 — Mailbox Runtime Cutover

## Goal

- remove dual mailbox runtime selection from the retained runtime surface
- make SQLite/store the only durable mailbox backend exposed to command logic

## Hard Dependencies

- `X.0` merged on `develop`
- `integrate/phase-X` created from a validated `integrate/phase-W`
- no fallback path may be preserved for later cleanup once this sprint starts

## Exact Targets

- `crates/atm-core/src/service_runtime_store.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-core/src/mailbox/mod.rs`
- `crates/atm-core/src/mailbox/source.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/src/mailbox/surface.rs`
- `crates/atm-core/src/read/state.rs`
- `crates/atm-core/src/workflow.rs`
- `crates/atm-core/src/lib.rs`
- `crates/atm-core/Cargo.toml`
- `crates/atm-core/tests/mailbox_locking.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`
- `crates/atm-daemon/src/tests.rs`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`
- `docs/project-plan.md`

## Required Work

- delete `LegacyMailboxRuntime`
- delete `DefaultMailboxRuntime::Legacy`
- delete `legacy_runtime()`
- delete the internal legacy-only helpers in
  `service_runtime_store.rs:112-185`
- stop `default_runtime()` from selecting a legacy backend
- remove `allows_legacy_mailbox_files()` from the runtime-facing contract
- replace file-backed mailbox trait methods on `RetainedMailboxRuntime` with
  the store-shaped operations that remain necessary after cutover
- remove `LEGACY_MESSAGE_KEY_PREFIX` and any production dependency on a mailbox
  backend discriminant
- update the atm-core boundary/architecture docs to state there is one durable
  mailbox backend

## Delivered

- removed dual runtime selection and all named legacy mailbox selectors from
  `atm-core`
- converted `default_runtime()` to fail closed when no SQLite/store-backed
  runtime factory is installed
- removed production `read/ack/clear/send/doctor` backend branching and updated
  the retained mailbox trait surface to store-shaped operations only
- deleted `crates/atm-core/src/read/legacy_path.rs` and
  `crates/atm-core/src/unsupported_adapters.rs`
- added test/runtime bootstrap support so same-host CLI, core mailbox-locking,
  and daemon doctor/runtime tests execute against the SQLite-backed retained
  runtime path
- fixed the SQLite envelope serialization mismatch uncovered by the cutover in
  `crates/atm-rusqlite/src/writer/ops.rs`
- updated project-plan / boundary / architecture docs to reflect the one-backend
  retained runtime rule

## Acceptance Criteria

- `rg -n "LegacyMailboxRuntime|DefaultMailboxRuntime::Legacy|legacy_runtime\\(|allows_legacy_mailbox_files" crates/atm-core/src`
  returns no production-code matches
- `rg -n "LEGACY_MESSAGE_KEY_PREFIX" crates/atm-core/src` returns no
  production-code matches
- `default_runtime()` no longer returns a legacy mailbox implementation
- retained mailbox interfaces no longer expose backend-choice branching
- daemon/store unavailability returns shared ATM errors instead of selecting a
  second mailbox implementation

Implementation result:
- all listed acceptance criteria are satisfied on this branch

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
