---
id: AD.4
title: Reconcile Runtime Removal
status: planned
branch: feature/pAD-s4-reconcile-runtime-removal
worktree: ../atm-core-worktrees/feature/pAD-s4-reconcile-runtime-removal
target: integrate/phase-AD
---

# Sprint AD.4 — Reconcile Runtime Removal

## Goal

- remove `ReconcileRuntime` and the daemon watch/import subsystem

## Hard Dependencies

- `AD.2` complete
- `AD.3` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/architecture.md`

## Exact Targets

- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-core/src/boundary/mod.rs`
- `boundaries/atm-core/reconcile-coordinator.toml`
- `boundaries/atm-core/watch-event-source.toml`
- `boundaries/atm-daemon/daemon-reconcile-coordinator.toml`
- `boundaries/atm-daemon/file-watch-event-source.toml`
- `docs/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-core/requirements.md`

## Paths To Delete

- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/reconcile_runtime/notification_fingerprints.rs`
- `crates/atm-daemon/src/reconcile_runtime_tests.rs`
- `crates/atm-daemon/src/watch_runtime.rs`
- daemon-only watch/reconcile wiring that exists solely to feed reconcile

## Modified Surfaces

- modify daemon composition so no accepted startup path constructs reconcile or
  watch runtimes
- modify boundary exports so reconcile/watch-only traits are absent from the
  accepted runtime surface
- modify reconcile/watch boundary TOMLs so retired components are no longer
  declared `active`, `stub_landed`, or composition-owned by deleted runtime
  roots
- rewrite docs that still describe reconcile/watch as a live Claude runtime

## Obsolescence Instructions

- any retained `WatchEventSource`, `ReconcileCoordinator`, reconcile request,
  or watch-runtime helper that cannot be deleted immediately must be marked
  `Phase AD obsolete: historical reconcile/watch only`
- retained obsolete reconcile/watch symbols must have zero accepted runtime
  construction paths and zero new call sites

## Deliverables

- the daemon no longer ships or starts `ReconcileRuntime`
- the daemon no longer ships or starts `WatchRuntime`
- watched-source import no longer participates in the accepted runtime
- no send/read path depends on reconcile completion or reconcile notifications

## Required Work

- remove the reconcile coordinator boundary and runtime wiring from the daemon
- remove file-watch/import behavior that is only there for the retired
  reconcile model
- remove any watched-source/import support that exists only to feed the retired
  Claude JSON mailbox path

## This Sprint Does Not Close

- daemon notification runtime removal
- post-send emitter implementation
- Claude inbox nudge deletion

## Acceptance Criteria

- no accepted daemon composition path starts or references `ReconcileRuntime`
- no accepted daemon composition path starts or references `WatchRuntime`
- no accepted `atm-core` boundary surface still requires reconcile-only traits
- no reconcile/watch boundary TOML still declares a deleted composition root or
  a retired reconcile/watch component as `active` or `stub_landed`
- `docs/architecture.md`, `docs/atm-daemon/architecture.md`,
  `docs/atm-daemon/requirements.md`, and `docs/atm-core/requirements.md` no
  longer describe watch/reconcile as an accepted production subsystem
- removing reconcile does not regress `send`, `read`, or `ack`

## Required Validation

- targeted daemon composition and command regression tests
- targeted boundary-lint / boundary-grep gates for retired reconcile/watch
  TOMLs
- `test ! -e crates/atm-daemon/src/reconcile_runtime.rs`
- `test ! -e crates/atm-daemon/src/reconcile_runtime/notification_fingerprints.rs`
- `test ! -e crates/atm-daemon/src/reconcile_runtime_tests.rs`
- `test ! -e crates/atm-daemon/src/watch_runtime.rs`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `git diff --check`
