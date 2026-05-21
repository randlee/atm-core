# ADR-015 — Daemon Runtime Snapshot Publication And Worker Ownership

- status: accepted
- date: 2026-05-20
- deciders:
  - team-lead
  - arch-ctm
- tags:
  - daemon
  - ownership
  - concurrency
  - runtime

## Context

The current daemon runtime still carries three lock-heavy coordination surfaces
whose real ownership model is narrower than the implementation suggests:

1. `RuntimeStatusCache`
   - read-mostly daemon health/status projection
   - currently coordinated through `Mutex<RuntimeStatusCacheState>`
2. `NotificationRuntime`
   - single background worker lane
   - currently coordinated through `Mutex<NotificationState> + Condvar`
3. `ReconcileRuntime`
   - single worker that coalesces, debounces, executes, and replies
   - currently coordinated through `Mutex<ReconcileState> + Condvar`

These designs keep working state correct enough, but they do not make the
runtime ownership model explicit. They also create poisonable daemon-shared
locks in places where immutable publication or worker-owned message passing is
the real design.

## Decision

The post-`Phase Y` daemon runtime line adopts these rules:

1. Read-mostly daemon health/status projection must use immutable snapshot
   publication.
   - `RuntimeStatusCache` publishes coherent immutable snapshots through
     `ArcSwap`
   - readers load snapshots directly
   - writers build and publish next snapshots atomically

2. Single-owner daemon worker lanes must use bounded command-channel or
   equivalent actor ownership.
   - `NotificationRuntime` uses a bounded channel to hand events to the worker
   - the worker owns queue/drain/persistence state
   - lifecycle/degraded status is published as immutable runtime state instead
     of caller-visible queue/lifecycle locking
   - `ReconcileRuntime` uses a bounded command channel plus per-request reply
     channel
   - the worker owns debounce, coalescing, completion fanout, and fingerprint
     registry state

3. Shared mutable queue/debounce/completion locks are not the accepted final
   design for these daemon lanes.

4. A narrow worker-join ownership helper is acceptable where needed to own one
   background thread handle.
   - `JoinHandleOwner` may use `Mutex<Option<JoinHandle<()>>>` for one-slot
     bounded join ownership only
   - that helper must not own queue state, debounce state, completion fanout,
     or any caller-visible control-plane coordination
   - review must treat it as a bounded lifecycle helper, not as an exception
     that reauthorizes lock-heavy runtime state

## Consequences

### Positive

- reader paths for daemon health/status no longer depend on one shared mutable
  lock
- notification and reconcile ownership become easier to reason about in review
- queue backpressure and worker-lane lifecycle are expressed through the same
  bounded command model
- daemon documentation can state one consistent rule:
  - immutable snapshots for read-mostly state
  - worker-owned channels for active coordination state

### Negative

- `ReconcileRuntime` cutover is larger than a local cleanup and requires a
  dedicated contract-first sprint
- shutdown and degraded-state publication need explicit redesign instead of
  lock-state inspection
- additional tests are required to prove coalescing, reply fanout, and bounded
  worker shutdown under the new designs

## Alternatives Considered

### Keep the current mutex/condvar designs

Rejected because the remaining designs are ownership mismatches, not merely
style issues.

### Convert all daemon lanes to async/Tokio actors

Rejected for this phase because the immediate need is to fix ownership clarity
and lock-heavy coordination without broad runtime-model churn.

### Use `RwLock` for `RuntimeStatusCache`

Rejected because the real design intent is immutable snapshot publication, not
reader/writer lock tuning.

## Implementation Plan

- `Y.19`:
  - `RuntimeStatusCache` -> immutable snapshot publication via `ArcSwap`
- `Y.20`:
  - `NotificationRuntime` -> bounded command channel + immutable runtime-status
    publication + worker-owned persistence state
- `Y.21`:
  - `ReconcileRuntime` actor contract and reply-path foundation, including
    shared `JoinHandleOwner` reuse and per-request reply fanout
- `Y.22`:
  - `ReconcileRuntime` actor cutover, worker-owned fingerprint-registry
    ownership, and deletion of the shared-state runtime path
- `Y.23`: phase-end proof, readiness record, and ADR acceptance on the final line
