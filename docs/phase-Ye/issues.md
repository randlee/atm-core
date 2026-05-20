# Phase Ye Daemon Ownership Simplification Issues

## Purpose

Record the daemon-runtime design issues that remain open after the `Phase Y`
closeout line and that justify a dedicated follow-on architecture phase rather
than ad hoc cleanup commits.

This is a planning artifact on a worktree off `develop`. It does not authorize
implementation on this branch.

## Baseline

- planning branch:
  - `plan/phase-Y-lock-removal`
- planning worktree:
  - `../atm-core-worktrees/plan/phase-Y-lock-removal`
- implementation prerequisite:
  - the accepted `Phase Y` line must land on `develop` before `Phase Ye`
    implementation begins

## Confirmed Design Gaps

1. `RuntimeStatusCache` is a read-mostly daemon-health projection, but it still
   uses one shared `Mutex<RuntimeStatusCacheState>`.
   - issue class:
     - ownership / read-path coordination
   - why it is a problem:
     - doctor/status readers and heartbeat writers currently depend on one
       poisonable shared lock
     - the design hides the real intent, which is immutable snapshot
       publication rather than cross-thread mutable ownership
   - required end state:
     - immutable status snapshots published through `ArcSwap`
     - lock-free reader snapshots
     - explicit snapshot replacement ownership on write paths

2. `NotificationRuntime` is a single-owner background worker, but it still uses
   `Mutex<NotificationState> + Condvar + VecDeque`.
   - issue class:
     - worker-lane ownership
   - why it is a problem:
     - producer threads share queue and lifecycle state that should belong to
       the worker lane
     - queue backpressure, degradation, and lifecycle are coupled through one
       lock-heavy mutable state surface
   - required end state:
     - bounded channel input
     - worker-owned queue and persistence state
     - only minimal lifecycle/degraded publication outside the worker

3. `ReconcileRuntime` is functionally an actor, but it still uses
   `Mutex<ReconcileState> + Condvar` for debounce, pending work, completions,
   and waiter tracking.
   - issue class:
     - coordination model mismatch
   - why it is a problem:
     - request submission, coalescing, execution, notification dedupe, and
       completion routing all share mutable runtime state
     - the design obscures single-owner reconcile semantics behind lock-based
       shared-state coordination
   - required end state:
     - bounded command channel into one worker-owned reconcile state machine
     - per-request reply channel for completion
     - worker-owned fingerprint registry, debounce state, and completion
       routing

4. Daemon requirements and architecture docs do not yet name the new ownership
   rules explicitly.
   - issue class:
     - documentation contract gap
   - why it is a problem:
     - the code currently signals one design, but the daemon documentation does
       not yet set the post-`Phase Y` target clearly enough for review and QA
     - without an ADR and explicit daemon requirements, future reviews will
       drift back toward lock-heavy shared mutable state
   - required end state:
     - one repository ADR naming the snapshot-publication and single-owner
       worker-lane rules
     - daemon requirements and architecture docs updated to reference those
       rules directly

## Non-Goals

These items are explicitly out of scope for `Phase Ye`:

1. Reopening `Phase Y` delivery-path or `NotificationSink` boundary closure.
2. Replacing daemon threading with async/Tokio actor work.
3. General daemon-runtime partition cleanup unrelated to these three ownership
   surfaces.
4. `Phase Z` rollout, smoke, or canary planning.

## Sprint Mapping

- `Y.19` closes the `RuntimeStatusCache` snapshot-publication redesign.
- `Y.20` closes the `NotificationRuntime` bounded-channel ownership redesign.
- `Y.21` closes the `ReconcileRuntime` actor foundation and public contract
  shift.
- `Y.22` closes the `ReconcileRuntime` actor cutover.
- `Y.23` closes the phase-end proof, ADR acceptance, and architecture
  validation for the full lock-removal line.
