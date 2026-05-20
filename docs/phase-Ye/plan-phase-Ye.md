# Phase Ye Plan

## Goal

Remove the remaining lock-shaped daemon control planes whose true ownership
model is either:

- immutable snapshot publication, or
- single-owner worker/actor coordination

`Phase Ye` is a post-`Phase Y` architecture line. It exists to simplify daemon
runtime ownership, not to reopen `Phase Y` delivery closure or to absorb
`Phase Z` rollout work.

## Baseline

- planning branch:
  - `plan/phase-Y-lock-removal`
- planning worktree:
  - `../atm-core-worktrees/plan/phase-Y-lock-removal`
- current `develop` planning baseline:
  - `84764175`
- implementation prerequisite:
  - `Phase Y` must first land on `develop`
- future integration branch:
  - `integrate/phase-Ye`
- issue inventory:
  - [issues.md](./issues.md)

## Governing Documents

`Phase Ye` planning and implementation must remain aligned with:

- product requirements and architecture:
  - `docs/requirements.md`
  - `docs/architecture.md`
- daemon requirements, architecture, and boundaries:
  - `docs/atm-daemon/requirements.md`
  - `docs/atm-daemon/architecture.md`
  - `docs/atm-daemon/boundaries.md`
- core boundaries consumed by the daemon worker lanes:
  - `docs/atm-core/boundaries.md`
  - `docs/atm-core/requirements.md`
- ADR inventory:
  - `docs/adr/INDEX.md`
  - `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
  - `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md`
- testing guidance:
  - `docs/testing-guidelines.md`

## Design Rule

`Phase Ye` adopts one explicit daemon-ownership simplification rule:

- read-mostly runtime-health/status projection must publish immutable snapshots
  instead of sharing one mutable cache lock
- worker-owned daemon lanes must receive commands through bounded channels and
  own their mutable queue/debounce/completion state inside the worker lane

If a sprint cannot justify its design against that rule, the sprint is not
hardened yet.

## Scope

`Phase Ye` exists to:

- replace `RuntimeStatusCache` lock-based reader/writer coordination with
  immutable snapshot publication
- replace `NotificationRuntime` lock-based queue/lifecycle coordination with a
  bounded worker-owned channel model
- replace `ReconcileRuntime` lock-based pending/completion/debounce
  coordination with a worker-owned actor model
- update daemon ADR and requirement docs so the new ownership rules are
  explicit and reviewable

`Phase Ye` does not exist to:

- redesign daemon transport
- reopen delivery-path correctness that already closed in `Phase Y`
- change product surface or CLI behavior
- merge `Phase Z` rollout or canary work into daemon architecture cleanup

## Sprint Sequence

### Y.19 Runtime Status Snapshot Publication

Purpose:

- replace `RuntimeStatusCache` shared mutable cache locking with immutable
  snapshot publication through `ArcSwap`

Authoritative sprint doc:

- [sprint-Y19.md](./sprint-Y19.md)

### Y.20 Notification Runtime Channel Ownership

Purpose:

- replace `NotificationRuntime` shared queue/lifecycle locking with a bounded
  command-channel design and worker-owned persistence state

Authoritative sprint doc:

- [sprint-Y20.md](./sprint-Y20.md)

### Y.21 Reconcile Runtime Actor Foundation

Purpose:

- define and land the worker-owned reconcile actor contract, command types, and
  reply-path model without claiming the final cutover in the same sprint

Authoritative sprint doc:

- [sprint-Y21.md](./sprint-Y21.md)

### Y.22 Reconcile Runtime Cutover And Proof

Purpose:

- complete the reconcile actor cutover, delete the shared-state runtime path,
  and leave one phase-end proof that the daemon lock-removal line closed
  cleanly

Authoritative sprint doc:

- [sprint-Y22.md](./sprint-Y22.md)

## Exit Condition

`Phase Ye` closes only when:

- `RuntimeStatusCache` no longer uses a shared mutable lock for reader/writer
  coordination
- `NotificationRuntime` no longer uses `Mutex<NotificationState>` or
  `Condvar` for queue ownership
- `ReconcileRuntime` no longer uses `Mutex<ReconcileState>` or `Condvar` for
  request/worker ownership
- `docs/adr/ADR-015-daemon-runtime-snapshot-and-worker-ownership.md` is
  accepted and reflected in daemon requirement/architecture docs
- the daemon validation stack passes on the final accepted `Phase Ye` line
