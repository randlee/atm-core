---
id: X.1
title: Mailbox Runtime Cutover
status: planned
branch: feature/pXb-s1-mailbox-runtime-cutover
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pXb-s1-mailbox-runtime-cutover
target: integrate/phase-Xb
---

# Sprint X.1 — Mailbox Runtime Cutover

## Modification

- This sprint is a restart replay on `feature/pXb-s1-mailbox-runtime-cutover`.
- Prior Phase `X` work already completed most of the intended `X.1` scope:
  - `70460f9b4fca061dd069b7b4245215c635ceb693`
    - `feat: complete phase X mailbox runtime cutover`
  - `6143bdab6cd2279558c62da836b5cb9aab054262`
    - `fix: close phase X1 follow-up findings`
- Execute this sprint by cherry-picking or selectively reapplying that prior
  work onto `pXb-s1` after audit; do not re-derive the design from scratch.
- The old `feature/pX-s1-mailbox-runtime-cutover` branch is a salvage source,
  not the new execution branch.
- QA must validate the entire `X.1` sprint on `pXb-s1`, not only the replayed
  delta from those commits.

## Goal

- remove dual mailbox runtime selection from the retained runtime surface
- make SQLite/store the only durable mailbox backend exposed to command logic

## Hard Dependencies

- `X.0` merged on `develop`
- `integrate/phase-Xb` created from `develop` after `X.0` was already live
- no fallback path may be preserved for later cleanup once this sprint starts

## Exact Targets

- `crates/atm-core/src/service_runtime_store.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`

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

## Acceptance Criteria

- `rg -n "LegacyMailboxRuntime|DefaultMailboxRuntime::Legacy|legacy_runtime\\(|allows_legacy_mailbox_files" crates/atm-core/src`
  returns no production-code matches
- `rg -n "LEGACY_MESSAGE_KEY_PREFIX" crates/atm-core/src` returns no
  production-code matches
- `default_runtime()` no longer returns a legacy mailbox implementation
- retained mailbox interfaces no longer expose backend-choice branching
- daemon/store unavailability returns shared ATM errors instead of selecting a
  second mailbox implementation

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
