---
phase: AO2
sprint: AO2.6
title: Restore bounded admission-writer transaction coalescing
branch: future-dev-worktree
integration_branch: integrate/phase-ao2
status: complete
depends_on:
  - AO2.5.4-mandatory-benchmark-snapshot-restore
blocks:
  - AO2.7-m5-tcp-benchmark-parity
  - AO2.8-windows-tcp-benchmark-parity
---

# AO2.6 — Restore bounded admission-writer transaction coalescing

## Decision

Restore the fixed one-millisecond transaction-coalescing window in the sole
`atm-storage-rusqlite` writer thread. It was present before the Tokio/Axum
async-admission migration and was removed mechanically when the writer channel
changed from `std::sync::mpsc` to `tokio::sync::mpsc` in commit `e9afc2e19`.
That migration retained a drain of items already queued but removed the prior
bounded wait for arrivals immediately after the first write. The result turns
closely concurrent HTTP admissions into more SQLite commits/fsyncs than the
pre-migration path.

The repair is a storage-writer implementation correction, not a change to the
public HTTP protocol or a benchmark-specific optimization. It must retain the
Tokio/Axum daemon architecture and preserve one durable response per write
only after the enclosing SQLite transaction commits.

## Grounded historical evidence

- Commit `c02bb5801` changed `BATCH_TIME_BUDGET` in
  `crates/atm-storage-rusqlite/src/writer/mod.rs` to `Duration::from_millis(1)`
  specifically to coalesce concurrent SQLite writes.
- Commit `e9afc2e19` migrated the writer to `tokio::sync::mpsc`; its diff
  removed `BATCH_TIME_BUDGET`, `recv_timeout`, and the test that named the
  coalescing window. Its replacement exits collection as soon as the new
  channel is momentarily empty.
- The current writer still owns one transaction for each collected batch and
  preserves per-operation savepoints/replies. AO2.6 must restore collection
  timing only; it must not alter operation ordering, savepoint isolation, or
  commit/reply semantics.

## Non-negotiable invariants

1. The only changed production seam is the dedicated SQLite writer's batch
   collection behavior in `atm-storage-rusqlite`.
2. The maximum deliberate wait after receiving the first queued write is one
   millisecond. A lone request may therefore pay at most that bounded
   coalescing delay before its transaction is opened.
3. Every operation remains in submission order within one writer transaction;
   each keeps its savepoint and receives its result only after the outer commit
   succeeds. A commit failure fails all previously successful operations in
   that transaction exactly as today.
4. Shutdown/disconnect behavior remains bounded and drains/replies according
   to the existing writer contract; it must not strand async oneshots or sync
   callers.
5. No polling loop, unbounded sleep, busy wait, new runtime thread, global
   configuration knob, feature flag, or benchmark-only mode is permitted.

## Design constraints

The current Tokio channel does not provide the old `recv_timeout` primitive.
Implementation begins with a narrow characterization test around the writer's
existing collection seam. It may introduce the smallest timeout-capable wait
there, but must not redesign the channel ownership, async admission API,
Axum router, request deadline model, connection framing, TLS adapters, or
benchmark harness merely to recover this one millisecond.

The implementation must use an explicit deadline (`Instant + 1ms` or an
equivalent single bounded timer) rather than repeating sleeps. It must drain
all messages already available before waiting and stop waiting once the fixed
deadline expires. The selected mechanism must be deterministic under unit
test control; wall-clock race tests are not acceptance evidence.

### Rust structural review constraints

- `RBP-001`: preserve the existing typed `AtmError` causes and recovery
  behavior for queue, transaction-open, commit, and shutdown failures; do not
  replace them with opaque batching strings.
- `RBP-002`: the writer's queue/shutdown state is dynamic and already owned by
  one private loop, so a public typestate API would add surface without making
  a new invalid state impossible. No new public state machine is allowed.
- `RBP-004`: the one-millisecond interval remains a private named duration
  with one semantic owner. If implementation needs to pass it across helpers,
  introduce a private validated wrapper rather than raw duplicated numbers.
- `RBP-006`/`RBP-009`: no new shared mutable primitive or eager clone may be
  introduced on the writer's hot path without an explicit benchmarked reason.

## Work items

1. **Characterize before editing.** Add tests that distinguish: a currently
   queued burst, a write arriving within the one-millisecond window, a write
   arriving after the window, shutdown during the window, and receiver
   disconnection. Capture transaction/reply ordering in the existing writer
   test seam.
2. **Restore bounded collection.** Reintroduce the fixed one-millisecond
   deadline only in `collect_batch` (or its smallest necessary helper), while
   retaining the existing bounded Tokio submission and reply paths.
3. **Prove persistence semantics.** Extend tests for one outer transaction,
   savepoint isolation, commit failure fan-out, and no reply before commit.
4. **Guard scope.** Add or update an architecture guard/static assertion that
   the batching interval belongs only to the SQLite writer and is not exposed
   as HTTP, TLS, CLI, environment, or benchmark configuration.
5. **Regression evidence.** Run crate tests and the safe benchmark workflow
   only after AO2.5.4 has merged; benchmark data is evidence for AO2.7, not an
   AO2.6 development fixture.

## Acceptance criteria

| Property | Required proof |
| --- | --- |
| Window | A controlled second admission inside 1 ms joins the first transaction. |
| Bound | A controlled arrival after 1 ms begins the next transaction. |
| Lone write | It receives a durable result after no more than the bounded coalescing delay. |
| Ordering | Submitted writes execute in order; no duplicate or dropped write is possible. |
| Durability | Responses occur only after the outer transaction commits. |
| Failure | Commit/open/shutdown/disconnect behavior preserves existing typed errors and reply completion. |
| Scope | No production changes outside the storage writer and its required tests/guards. |
| Error contract | Existing typed queue/SQLite errors retain cause and recovery context. |

Required gates: targeted writer tests, `cargo test -p atm-storage-rusqlite`,
architecture tests, `just lint`, and `just test`. A clean diff review must
show no change to `atm-http-runtime` routing, client/framing, peer transport,
TLS, daemon lifecycle, or benchmark timed-profile code.

## Performance and rollback

AO2.6 does not claim a throughput number. It restores the previously intended
coalescing behavior and supplies deterministic correctness proof. AO2.7 alone
decides whether the physical M5 TCP result meets the requested threshold.

Rollback is a normal revert of the writer-only commit. No database migration,
schema change, persisted configuration, or data repair is involved.
