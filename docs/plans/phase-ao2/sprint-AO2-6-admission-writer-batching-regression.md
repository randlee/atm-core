---
phase: AO2
sprint: AO2.6
title: Restore async admission writer batching parity
branch: fix/admission-writer-batching-regression
integration_branch: integrate/phase-ao2
status: draft_for_review
depends_on:
  - PR-977-scoped-live-database-guard
  - AO2.5-benchmark-primary-database-safety
blocks: []
---

# AO2.6 — Restore async admission writer batching parity

## Decision summary

Restore the pre-`e9afc2e19093700f4424873ed8effb1bb38ead35` one-millisecond
collection window at the single SQLite writer-thread ingress. Preserve the
Tokio/Axum HTTP daemon, the public HTTP admission resource, the outer TLS
transport wrapper, and the single SQLite writer ownership model. Do not add a
TLS branch, controller, retry queue, second persistence path, or a post-HTTP
batching layer.

The proposed mechanism is a bounded `std::sync::mpsc::SyncSender`/
`Receiver` at the already-existing writer ingress, with the existing async
caller reply path retained through `tokio::sync::oneshot`. `submit_async` uses
nonblocking `try_send` with its existing bounded deadline/yield policy; the
writer thread receives the first operation, then calls
`recv_timeout(BATCH_TIME_BUDGET)` for at most one millisecond and drains all
already-admitted operations before one SQLite transaction. This returns the
coalescing primitive to exactly the layer that owned it before e9, without an
extra daemon, dispatcher, HTTP hop, or TLS-aware storage behavior.

## Problem statement and root cause

The historical M4 TCP/f64 artifact at
`site/reports/send-message-benchmark/20260801-072846.375494-mac-arm64-01-tcp-f64.json`
records 24,958.69 msg/s at source
`3ec7ce1ff7269d8f43a65658c712778abbf2de14`. The later approximately 17k
same-harness target and the current approximately 5–6k results demonstrate a
material regression that predates TLS.

Commit `e9afc2e19093700f4424873ed8effb1bb38ead35`
(`perf(AL13): route Tokio writes through async storage admission`) changed the
writer ingress from `std::sync::mpsc::Receiver::recv_timeout` to
`tokio::sync::mpsc::Receiver`. Its `collect_batch` now drains only
`try_recv()` entries already queued when the first item is observed. A pipelined
f64 benchmark can therefore execute many one-message SQLite transactions
instead of coalescing arrivals over the historical 1 ms window. TLS did not
cause this change and must not be used to solve it.

## Invariants and non-goals

Required invariants:

- one writer thread remains the sole SQLite write owner;
- every caller receives the result for its own operation only after the
  transaction outcome is known;
- bounded channel capacity, 10-second submit/reply deadlines, shutdown drain,
  observability outcomes, and error codes remain explicit;
- the public request remains one canonical HTTP POST per message; there is no
  `message[]` endpoint;
- plaintext-test and mutual-TLS use the same admission/storage pipeline;
- the default daemon remains mTLS; plaintext-test remains runtime-selected and
  is never a compile-time alternative;
- no synchronous legacy daemon code is modified or reintroduced.

Out of scope:

- changes to the TLS handshake, certificates, peer discovery, HTTP resource,
  wire payload, or remote delivery/replay;
- raising batch time above 1 ms, adding an unbounded queue, or moving durable
  acknowledgement after the response;
- benchmark-account backup implementation (AO2.5 owns that);
- optimizing an unmeasured path before controlled parent/e9 comparison proof.

## Detailed design

### Writer ingress and batching

1. Retain `ReplyTx::Async(tokio::sync::oneshot::Sender<...>)` so Tokio request
   handlers await a reply without blocking an executor thread.
2. Replace only the writer ingress receiver with bounded
   `std::sync::mpsc::SyncSender/Receiver`, the same boundary used before e9.
   Synchronous callers retain their current bounded `try_send` retry behavior.
3. For async callers, make submission nonblocking: attempt `try_send`; when
   full, retain the returned message and await a bounded Tokio yield/sleep until
   the existing deadline. This is backpressure at the writer boundary, not a
   second queue or `spawn_blocking` hop.
4. After the writer receives its first `Submit`, record a monotonic deadline
   `first_received_at + BATCH_TIME_BUDGET` (one millisecond). Repeatedly call
   `recv_timeout(remaining)` until the deadline; submit operations join the
   batch, a `Shutdown` sets drain mode, disconnect initiates drain, and timeout
   closes the collection window.
5. Drain immediately available submits after a timeout/shutdown observation,
   then execute one immediate SQLite transaction and reply individually.
   The window begins after the first accepted operation, never before it, and
   cannot exceed one millisecond plus scheduler granularity.

This is deliberately a replacement of the changed ingress primitive, not an
adapter between Tokio and a second writer. The async-facing API remains async;
the dedicated writer remains synchronous because SQLite ownership already
requires that thread.

### Evidence and instrumentation

Add test-only batch/transaction observation at the writer seam, not on the
production HTTP hot path. It must prove that an operation arriving within the
one-millisecond window joins the first transaction and that an operation after
the window forms a later transaction. Production observability remains
aggregate/error-oriented and must not log a record per successful admission.

## Performance-regression prevention

The primary risk is solving an async submission concern by adding an extra
queue, runtime hop, lock, timer task, per-message allocation, or TLS decision
inside the admission path. Any of these can preserve correctness while losing
the throughput that batching was intended to protect.

Mitigation:

- the only new wait is the historical bounded 1 ms collection wait at the
  existing writer receiver;
- no TLS or peer configuration type may enter `atm-storage-rusqlite` writer
  code; static architecture coverage enforces this;
- async queue-full handling uses the existing deadline and avoids blocking a
  Tokio worker;
- transaction, reply, and shutdown semantics are characterized before the
  change and compared afterward;
- a parent/e9/current controlled benchmark series establishes causality before
  the implementation is declared a fix.

Measurement is mandatory after AO2.5 supplies the safe physical harness. On
the same host, released binaries, data profile, and account, retain raw and
compact evidence for `tcp` plaintext-test and `tcp-tls` mutual TLS, f1/f8/f64,
and sustained workloads. Compare p50 throughput, p95/p99 latency, accepted
count, error count, transaction count, and batch-size distribution against the
approved approximately 17k msg/s plaintext f64 baseline. Plaintext f64 must
meet the approved parity threshold; mTLS is measured separately and cannot
relax plaintext acceptance. If the parent/e9 experiment does not demonstrate
the predicted batching/transaction relationship, stop and revise this plan
before changing code.

## Work breakdown and dependencies

1. **AO2.6.1 — Causality baseline:** use AO2.5's isolated physical harness to
   run parent-of-e9, e9, and current release binaries with the same TCP f64
   profile. Record transaction/batch evidence. Blocks implementation.
2. **AO2.6.2 — Requirements/ADR review:** confirm that the writer seam change
   does not change durable acknowledgement, storage ownership, or TLS layering;
   update requirements/ADR only if the existing contracts do not state the
   one-ms bounded window and public-response behavior. Depends on 1.
3. **AO2.6.3 — Writer implementation:** restore the bounded ingress/window as
   specified above, preserving the async reply contract. Depends on 2.
4. **AO2.6.4 — Characterization and boundary tests:** cover within-window and
   after-window operation placement, queue-full deadline, shutdown drain,
   duplicate admission, and no TLS types/branches in storage. Depends on 3.
5. **AO2.6.5 — Physical parity proof:** run plaintext and mTLS matrix on M4,
   M5, and fastpc4/Windows when available; analyze logs and retain artifacts.
   Depends on 3 and 4.
6. **AO2.6.6 — Rollback drill:** prove a revert restores pre-change behavior
   without database migration or queued-message loss. Depends on 5.

## Acceptance criteria and test matrix

| Case | Required proof |
| --- | --- |
| One write | Completes after at most the bounded admission timeout and is durable before response. |
| Two writes inside 1 ms | One writer transaction, two correct replies, no response cross-talk. |
| Write after 1 ms | Separate transaction; no unbounded batching delay. |
| Channel full | Async handler stays nonblocking and returns the existing bounded timeout/error contract. |
| Shutdown | Queued operations receive explicit unavailable errors; writer checkpoint behavior remains bounded. |
| Plaintext vs mTLS | Same storage/writer code path; security selection occurs outside storage. |
| Static boundary | No `PeerWireMode`, TLS certificate, connector, or HTTP controller dependency in writer modules. |
| Live matrix | Safe-account `tcp` and `tcp-tls` artifacts meet the approved plaintext parity bar and retain all diagnostics. |

Required gates: focused Rust tests, architecture/boundary tests, `just lint`,
`just test`, then live isolated-account plaintext and mTLS proof before QA.
Any result below parity is a failure requiring code-path analysis, not a claim
that a lower minimum throughput floor passes.

## Rollback and recovery

The change has no schema or wire migration. Revert the writer-ingress commit,
rebuild the paired CLI/daemon, run the safe-account smoke, and compare the same
artifact matrix. Do not revert by disabling TLS, reducing logging, changing
the workload, or bypassing the public HTTP endpoint; those invalidate the
comparison rather than recover the writer behavior.

## Boundary and ADR impact review

- **`atm-storage-rusqlite`:** owns the only implementation change. The writer
  remains a sealed persistence adapter with one SQLite owner.
- **`atm-http-runtime`:** no handler, route, controller, or daemon topology
  change. It continues to call the async store API and await durable results.
- **TLS/peer-wire:** remains an outer transport wrapper. No TLS type or mode
  may cross into writer code.
- **CLI/graft/cross-host:** unchanged; canonical HTTP POST remains the write
  boundary.
- **ADR/requirements:** review required before code. Amend only if necessary
  to make batching latency, durable response, and isolation evidence normative;
  do not weaken ADR-026's one-root rule.
