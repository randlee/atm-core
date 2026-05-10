# Sprint S.14 Daemon Runtime Hardening Plan

**Branch**: feature/pS-s14-runtime-hardening  
**Base**: integrate/phase-S @ 77badd5  
**PR target**: integrate/phase-S  
**Status**: Planning

## Goal

Close the remaining daemon-runtime hardening gaps left open after S.13. The
current `atm-daemon` runtime still has five production-significant issues:
- lifecycle wake workers are still fire-and-forget
- reconcile shutdown can detach and still report success
- watch shutdown can detach and still report success
- runtime status-cache cardinality is not enforced at insert time
- runtime status-cache eviction demotes entries instead of actually removing
  them

The remaining lower-severity items are still part of this sprint because they
affect bounded-state guarantees, retained-log durability, doctor completeness,
and architecture clarity.

## Current Failure Shape

The current integrated runtime has four recurring failure classes:
- helper-thread lifetime is justified by comments but not owned by an explicit
  join contract
- bounded shutdown paths still treat timed-out worker joins as clean success in
  some lanes
- bounded caches are not always bounded in actual retained cardinality
- daemon status and retained-log health data exist internally but are not
  always projected with the same fidelity through the doctor surface

S.14 therefore hardens:
- lifecycle-control worker ownership
- reconcile/watch shutdown result semantics
- retained-state caps and cleanup
- daemon observability flush correctness
- doctor-facing health projection detail

## Scope And Non-Goals

In scope:
- lifecycle wake-worker ownership and bounded join behavior
- reconcile/watch shutdown error semantics
- runtime status-cache bounded cardinality
- doctor finding projection completeness
- reconcile fingerprint-state and completed-waiter cleanup bounds
- retained-log shutdown flush correctness
- architecture documentation needed to record the accepted Windows polling
  exception and WatchRuntime drop dependency

Out of scope:
- local IPC transport redesign already covered by S.13
- peer transport changes
- SQLite write-worker changes planned in S.15
- daemon-spawn test exceptions

## Design Areas

### 1. Lifecycle Wake Worker Ownership

#### `S14-001`

- File: `crates/atm-daemon/src/lifecycle_control.rs:274-282`, `311-337`
- Problem:
  - Unix lifecycle wake worker and Windows lifecycle polling worker are both
    spawned and intentionally dropped.
  - S.13 only documented the exception; it did not give daemon shutdown a way
    to join those workers.
- Fix approach:
  - extend the shared lifecycle-control singleton state with stored wake-worker
    ownership:
    - one `Option<JoinHandle<()>>`
    - one shutdown signal that the worker observes in addition to the existing
      terminate/reload flags
  - keep the install-once process-global model, but make repeated
    `LifecycleControlSourceAdapter::install()` calls reuse the same stored
    worker instead of spawning detached replacements
  - add a daemon-private `shutdown_worker_with_timeout()` path that:
    - requests worker stop
    - wakes the worker
    - joins with a documented bounded deadline
    - returns a typed `AtmError` on timeout or panic rather than silently
      detaching
  - wire that shutdown into daemon runtime teardown so the lifecycle helper is
    part of the same bounded shutdown contract as other runtime workers
- Test / proof:
  - add Unix and Windows lifecycle-control tests that prove:
    - the stored worker is reused across repeated install calls
    - shutdown joins the worker within the bounded deadline
    - panic/timeout surfaces as typed daemon-unavailable failure

### 2. Reconcile And Watch Shutdown Semantics

#### `S14-002`

- File: `crates/atm-daemon/src/reconcile_runtime.rs:238-282`
- Problem:
  - `ReconcileRuntime::shutdown()` drops the join-helper on timeout and still
    returns `Ok(())`.
  - `RuntimeComposition::shutdown_background_lanes()` therefore cannot
    distinguish clean drain from detached worker timeout.
- Fix approach:
  - preserve the bounded timeout, but change the timeout branch to return a
    typed `Err(AtmError::daemon_unavailable(...).with_recovery(...))`
  - keep the warning log, but make the timeout observable to callers
  - require background-lane shutdown to log the timeout as a degraded shutdown
    outcome, not a clean stop
- Test / proof:
  - update reconcile shutdown tests to assert timeout returns `Err`, not
    success
  - add/adjust integration coverage through runtime composition if needed so
    lane-shutdown callers observe the typed failure

#### `S14-003`

- File: `crates/atm-daemon/src/watch_runtime.rs:209-254`
- Problem:
  - `WatchRuntime::shutdown()` has the same detach-plus-`Ok(())` pattern as
    reconcile.
- Fix approach:
  - mirror the reconcile fix exactly for watch runtime shutdown:
    - warning stays
    - timeout becomes typed `Err`
    - caller must not interpret timeout as clean shutdown
- Test / proof:
  - update the bounded watch shutdown tests to assert typed timeout failure
  - keep the bounded-time guarantee while changing the result contract

### 3. Retained Observability Flush Correctness

#### `S14-004`

- File: `crates/atm-daemon/src/daemon_observability.rs:107-131`
- Problem:
  - `best_effort_flush_blocking()` flushes the logger, then reopens
    `active_log_path` and `sync_all()`s that reopened file.
  - if log rotation happened between `flush()` and the reopen, the sync may hit
    the post-rotation file instead of the descriptor that actually received the
    final writes.
- Fix approach:
  - change the daemon observability flush path to sync the same file
    descriptor/handle that the logger just flushed
  - avoid reopening by path for the best-effort shutdown sync step
  - if the lower logger abstraction does not currently expose the active sink
    handle, extend the crate-private logger integration so shutdown flush can
    obtain a syncable handle to the pre-rotation file
- Test / proof:
  - add retained-log rotation coverage proving shutdown sync targets the
    pre-rotation file handle rather than a reopened path
  - keep the flush step best-effort and bounded per the existing architecture
    contract

### 4. Runtime Status Cache Bounds

#### `S14-005`

- File: `crates/atm-daemon/src/runtime_health.rs:121-149`
- Problem:
  - heartbeat ingest inserts first and enforces the cap afterward
  - under concurrent inserts, the map can temporarily exceed the documented
    cap, which violates `REQ-DAEMON-RUNTIME-004`
- Fix approach:
  - enforce the status-cache cap atomically while holding the cache lock:
    - if the key already exists, update in place
    - if the key is new and the map is at capacity, select an eviction
      candidate before inserting the new record
    - then insert the new record once space is guaranteed
  - preserve the existing “do not evict current key or identity-conflict
    entries first” selection policy unless a stricter one is documented in the
    implementation sprint
- Test / proof:
  - add runtime-health tests that drive new-key insertion at the cap and prove
    cardinality never exceeds `MAX_STATUS_CACHE_ENTRIES`
  - add a concurrent-heartbeat-style regression test that checks post-ingest
    cardinality under repeated inserts

#### `S14-006`

- File: `crates/atm-daemon/src/runtime_health.rs:129-148`
- Problem:
  - eviction currently demotes the selected entry to `Unknown` instead of
    removing it
  - the map therefore never shrinks, which defeats bounded retained cardinality
- Fix approach:
  - replace demotion-in-place with actual `cache.members.remove(&evicted_key)`
  - keep the warning emission, but update the warning text to describe bounded
    eviction/removal rather than demotion
  - let missing entries project as `Unknown` through snapshot/doctor scope
    rules instead of retaining a permanent dead map entry
- Test / proof:
  - add a status-cache test that inserts past the cap and proves:
    - retained map size stays bounded
    - evicted members disappear from the internal map
    - scoped snapshot/doctor projection still treats missing members as unknown

### 5. Doctor Projection Completeness

#### `S14-007`

- File: `crates/atm-daemon/src/runtime_health.rs:599-645`, `765-799`
- Problem:
  - `RuntimeStatusSnapshot` already carries `singleton_owner_pid`,
    `sqlite_ready`, and `degraded_ingest`
  - `runtime_status_finding()` collapses the doctor-facing message to member
    counts and readiness only, so those fields are not projected explicitly in
    the runtime-status doctor finding
- Fix approach:
  - extend `runtime_status_finding()` so the emitted `DoctorFinding` message or
    remediation text explicitly surfaces:
    - `singleton_owner_pid`
    - `sqlite_ready`
    - `degraded_ingest`
  - keep `report.runtime_status = Some(runtime_status)` as the machine-readable
    payload, but align the human-facing doctor finding with
    `docs/atm-daemon/architecture.md §3.5` and `REQ-DAEMON-HEALTH-001`
- Test / proof:
  - add doctor projection coverage that verifies degraded SQLite / degraded
    ingest / singleton-owner details are present in the finding output

### 6. Reconcile Runtime Bounded Auxiliary State

#### `S14-008`

- File: `crates/atm-daemon/src/reconcile_runtime.rs:449-481`
- Problem:
  - `notification_fingerprints` is an unbounded `HashMap<ReconcileKey, HashSet<String>>`
  - repeated unique targets can grow the map without limit
- Fix approach:
  - introduce a documented cardinality cap for tracked fingerprint sets
  - keep the current keying model, but add bounded retention:
    - if inserting a new key at capacity, evict the oldest or least-recently
      refreshed key before inserting the new one
  - record the accepted cap in daemon requirements/architecture under
    `REQ-DAEMON-RUNTIME-004`
- Test / proof:
  - add reconcile notification tests that prove the fingerprint registry:
    - stays within the documented cap
    - still emits notifications correctly after eviction

#### `S14-011`

- File: `crates/atm-daemon/src/reconcile_runtime.rs:348-354`, `552-555`
- Problem:
  - timed-out waiters call `release_waiter(waiter_id)` and exit
  - if the worker later completes, it inserts an outcome for those waiter ids
    into `state.completed`
  - those orphaned completed entries are never observed and can accumulate as a
    slow leak
- Fix approach:
  - make waiter release remove the waiter id from both pending and future
    completion paths
  - before inserting worker outcomes, filter out waiter ids that are no longer
    active
  - as a defensive cleanup, prune stale completed entries on timeout/release
    paths
- Test / proof:
  - add reconcile timeout regression coverage proving `state.completed` does
    not grow after waiters time out before background completion

### 7. WatchRuntime Drop Contract And Windows Exception Record

#### `S14-009`

- File: `crates/atm-daemon/src/watch_runtime.rs:351-355`
- Problem:
  - `Drop` only calls `shutdown()` when `Arc::strong_count(&self.inner) == 1`
  - under the normal daemon runtime layout, clones can keep the strong count
    above one, so the drop fallback is effectively inert
- Fix approach:
  - document that explicit runtime teardown through
    `RuntimeComposition::shutdown_background_lanes()` is the authoritative stop
    path for watch runtime
  - add a structural comment and a `debug_assert!` documenting that `Drop` is
    only a last-resort cleanup path, not the primary ownership contract
  - do not rely on `Drop` for correctness when the runtime intentionally shares
    `Arc` ownership
- Test / proof:
  - documentation closeout plus targeted debug assertion is sufficient unless
    implementation discovers a safe structural simplification

#### `S14-010`

- File: `crates/atm-daemon/src/lifecycle_control.rs:328-331`
- Problem:
  - the Windows `25ms` lifecycle polling sleep is referenced in code and in
    `plan-phase-S.md`, but it does not yet have a durable architecture
    exception record inside daemon architecture docs
- Fix approach:
  - add the accepted Windows polling exception to
    `docs/atm-daemon/architecture.md`
  - cross-reference the reason:
    - `signal_hook::flag` exposes no matching blocking Windows wake primitive
    - the loop is bounded, documented, and intentionally isolated to the
      lifecycle adapter
- Test / proof:
  - documentation-only closeout

## Implementation Notes

- All new shutdown timeout failures must return typed `AtmError`s with
  recovery text, not bare warnings plus `Ok(())`.
- All bounded caches must remain bounded in actual retained cardinality, not
  just in semantic labels or doctor projection.
- New tests must preserve the no-fixed-sleep and bounded-wait contract already
  recorded for Phase S.

## Acceptance Mapping

S.14 is complete when:
- lifecycle wake workers are explicitly owned and joined with timeout
- reconcile/watch shutdown timeout is observable as typed failure
- retained-log shutdown sync targets the flushed file descriptor
- runtime status-cache insert/evict logic never exceeds the documented cap and
  truly removes evicted entries
- doctor runtime-status findings expose singleton-owner, SQLite readiness, and
  degraded-ingest state explicitly
- reconcile auxiliary state (`notification_fingerprints`, `completed`) remains
  bounded
- WatchRuntime drop semantics and the Windows `25ms` lifecycle exception are
  both documented precisely
