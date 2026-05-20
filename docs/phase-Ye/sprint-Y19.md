---
id: Y.19
title: Runtime Status Snapshot Publication
status: draft
branch: feature/pYe-s19-runtime-status-snapshot-publication
worktree: ../atm-core-worktrees/feature/pYe-s19-runtime-status-snapshot-publication
target: integrate/phase-Y
---

# Sprint Y.19 — Runtime Status Snapshot Publication

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

## Exact Targets

- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/composition.rs`
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`

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
    pub(crate) fn snapshot_state(&self) -> Arc<RuntimeStatusCacheState>;

    pub(crate) fn publish_state(
        &self,
        next: RuntimeStatusCacheState,
    );

    pub(crate) fn update_state<F>(&self, mutate: F)
    where
        F: FnOnce(&mut RuntimeStatusCacheState);
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

## Required Deliverables

- `RuntimeStatusCache` uses immutable snapshot publication through `ArcSwap`
- `Mutex<RuntimeStatusCacheState>` is removed from the production path
- `snapshot()` and `snapshot_for_members(...)` read one immutable published
  snapshot without poisoning or blocking on shared mutable state
- heartbeat and sqlite-readiness writers publish coherent next snapshots
- daemon requirements and architecture docs explicitly state that live status
  projection is a snapshot-publication surface, not a shared mutable cache lock
- `ADR-015` is updated to include the final snapshot-publication contract for
  `RuntimeStatusCache`; phase-end acceptance remains a `Y.23` deliverable

## Named Acceptance Tests

- `runtime_status_cache_heartbeat_publish_is_atomically_visible`
- `runtime_status_cache_scoped_snapshot_reads_do_not_require_shared_locking`
- `runtime_status_cache_sqlite_readiness_flip_publishes_one_coherent_snapshot`

## Closure Invariants

- no production `RuntimeStatusCache` reader path depends on
  `Mutex<RuntimeStatusCacheState>`
- all doctor/status snapshots come from one immutable published state value
- writer paths publish complete next snapshots instead of mutating state in
  place behind a daemon-shared lock

## Scope Estimate

This sprint is credibly closable in one sprint because the cache is read-mostly
and does not own a background worker or completion-routing protocol.

If the implementation needs a broader daemon-health redesign than snapshot
publication plus projection updates, the sprint must split before
implementation.

## Required Validation

- `rg -n "Mutex<RuntimeStatusCacheState>|lock poisoned" crates/atm-daemon/src/runtime_status_cache.rs`
- `cargo test --workspace runtime_status_cache_heartbeat_publish_is_atomically_visible -- --nocapture`
- `cargo test --workspace runtime_status_cache_scoped_snapshot_reads_do_not_require_shared_locking -- --nocapture`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `git diff --check`
