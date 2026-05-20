---
id: Y.19
title: Runtime Status Snapshot Publication
status: planned
branch: feature/pYe-s19-runtime-status-snapshot-publication
worktree: ../atm-core-worktrees/feature/pYe-s19-runtime-status-snapshot-publication
target: integrate/phase-Y
---

# Sprint Y.19 — Runtime Status Snapshot Publication

## Goal

- replace `RuntimeStatusCache` shared mutable locking with immutable snapshot
  publication through `ArcSwap`
- make reader snapshots lock-free and writer publication atomic on the
  production path

## Motivation / Problem Statement

`RuntimeStatusCache` is read-mostly daemon state. Heartbeat writers and
doctor/status readers currently coordinate through one
`Mutex<RuntimeStatusCacheState>`, even though the intended contract is:

- readers observe one coherent snapshot
- writers publish a next coherent snapshot

The current design therefore exposes a poisonable shared lock where immutable
publication is the real ownership model.

## Hard Dependencies

- `Phase Y` must be landed on `develop`
- `docs/phase-Ye/plan-phase-Ye.md`
- `docs/phase-Ye/issues.md`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

ADR-015 ownership in this sprint:

- update the `Decision` section so `RuntimeStatusCache` explicitly owns
  immutable snapshot publication through `ArcSwap`
- update the `Implementation Plan` section so `Y.19` is the only sprint that
  closes the runtime-status snapshot cutover

## Governing Requirements And ADRs

- `REQ-DAEMON-STATUS-001`
- `REQ-DAEMON-STATUS-003`
- `REQ-DAEMON-STATUS-004`
- `REQ-DAEMON-HEALTH-001`
- `ADR-015`

## Exact Targets

- `Cargo.toml`
- `crates/atm-daemon/Cargo.toml`
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
  - update the `DaemonStatusSourceAdapter` record
  - keep the review-visible `RuntimeStatusCache` control-plane note aligned

## Proposed Design

### Types

```rust
use arc_swap::ArcSwap;

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeStatusCacheState {
    members: HashMap<RuntimeMemberKey, RuntimeMemberRecord>,
    sqlite_ready: bool,
    sqlite_detail: Option<String>,
    degraded_ingest: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeStatusCache {
    state: Arc<ArcSwap<RuntimeStatusCacheState>>,
    observability: SubsystemObservability,
}
```

```rust
impl RuntimeStatusCache {
    pub(crate) fn snapshot(&self) -> Arc<RuntimeStatusCacheState>;

    pub(crate) fn snapshot_for_members(
        &self,
        members: &[RuntimeMemberKey],
    ) -> Arc<RuntimeStatusCacheState>;

    pub(crate) fn publish_state(
        &self,
        next: RuntimeStatusCacheState,
    );
}
```

```rust
impl RuntimeStatusCache {
    pub(crate) fn publish_state(&self, next: RuntimeStatusCacheState) {
        self.state.store(Arc::new(next));
    }
}
```

### Ownership

- writers own state transitions by cloning the current snapshot, mutating the
  next snapshot, and publishing it atomically
- readers own only snapshot load + projection; they must not block on a shared
  mutable cache lock
- poison-driven error handling disappears because the cache is no longer
  coordinated by a daemon-shared mutex

### Data Flow

1. heartbeat/update path loads current snapshot
2. writer builds next `RuntimeStatusCacheState`
3. writer publishes next snapshot through `ArcSwap`
4. doctor/status path reads one snapshot and derives counts/readiness from that
   immutable value

## Deliverables

- `RuntimeStatusCache` uses immutable snapshot publication through `ArcSwap`
- `Mutex<RuntimeStatusCacheState>` is removed from the production path
- `snapshot()` and `snapshot_for_members(...)` read one immutable published
  snapshot without poisoning or blocking on shared mutable state
- heartbeat and sqlite-readiness writers publish coherent next snapshots
- daemon requirements and architecture docs explicitly state that live status
  projection is a snapshot-publication surface, not a shared mutable cache lock
- `ADR-015` is updated to include the final snapshot-publication contract for
  `RuntimeStatusCache`; phase-end acceptance remains a `Y.23` deliverable

## Required Work

- add `arc_swap` to the workspace `Cargo.toml` dependency table and to
  `crates/atm-daemon/Cargo.toml` before the runtime-status snapshot work lands
- if `REQ-DAEMON-STATUS-004` and the `ADR-015` snapshot-publication rule are
  not yet present on the accepted implementation line when this sprint begins,
  this sprint must land them on that line as part of closure
- replace the production `RuntimeStatusCache` state holder with an immutable
  `ArcSwap<RuntimeStatusCacheState>` publication surface
- convert heartbeat and sqlite-readiness writers to clone, mutate, and publish
  a coherent next snapshot instead of mutating shared state behind a mutex
- update doctor/status snapshot reads so they load one immutable published
  snapshot and derive health/readiness from that value
- delete poison-oriented cache recovery behavior that exists only because the
  production path currently uses a shared mutable mutex
- update daemon requirements, architecture, and boundary docs so the cache is
  documented as a snapshot-publication surface rather than a lock-owned cache
- update `ADR-015` so `Y.19` is the only sprint that closes the snapshot
  publication cutover

## Paths To Delete

- `crates/atm-daemon/src/runtime_status_cache.rs`
  - delete production `Mutex<RuntimeStatusCacheState>` ownership
  - delete poison-driven cache read/write recovery paths that are only needed
    for the old shared-mutex design

## Acceptance Criteria

- `runtime_status_cache_heartbeat_publish_is_atomically_visible`
- `runtime_status_cache_scoped_snapshot_reads_do_not_require_shared_locking`
- `runtime_status_cache_sqlite_readiness_flip_publishes_one_coherent_snapshot`
- all listed deliverables land at a production-ready level for the sprint
  scope; no reader path still depends on `Mutex<RuntimeStatusCacheState>`
- the accepted design leaves one production publication seam, not a hybrid of
  mutex mutation plus snapshot reads
- the authoritative reader interface is `snapshot()` and
  `snapshot_for_members(...)`; no parallel `snapshot_state()` contract remains
- daemon docs and `ADR-015` describe immutable snapshot publication as the
  accepted runtime-status ownership rule

## Closure Invariants

- no production `RuntimeStatusCache` reader path depends on
  `Mutex<RuntimeStatusCacheState>`
- all doctor/status snapshots come from one immutable published state value
- writer paths publish complete next snapshots instead of mutating state in
  place behind a daemon-shared lock

## Explicit Non-Closure

- no daemon-health redesign beyond replacing the cache ownership model
- no change to roster-store truth ownership
- no notification or reconcile runtime redesign in this sprint

## Scope Estimate

This sprint is credibly closable in one sprint because the cache is read-mostly
and does not own a background worker or completion-routing protocol.

If the implementation needs a broader daemon-health redesign than snapshot
publication plus projection updates, the sprint must split before
implementation.

## Required Validation

- `rg -n 'arc_swap' Cargo.toml crates/atm-daemon/Cargo.toml`
- `rg -n "Mutex<RuntimeStatusCacheState>|lock poisoned" crates/atm-daemon/src/runtime_status_cache.rs`
- `cargo test --workspace runtime_status_cache_heartbeat_publish_is_atomically_visible -- --nocapture`
- `cargo test --workspace runtime_status_cache_scoped_snapshot_reads_do_not_require_shared_locking -- --nocapture`
- `cargo test --workspace runtime_status_cache_sqlite_readiness_flip_publishes_one_coherent_snapshot -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
