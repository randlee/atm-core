# Phase R.17 — Watch, Reconcile, And Notifier Runtime

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.17"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s17-watch-reconcile
branch: feature/pR-s17-watch-reconcile
status: complete
estimated_scope: L
```

## Goal

Replace the one-shot watch/reconcile/notifier placeholders with runtime-owned long-lived subsystems.

## Scope Summary

This sprint closes the current background-runtime gap for filesystem intake, reconcile scheduling, and notifier/plugin delivery. The daemon should own real loops and degradation handling instead of forwarding to generic helpers.

Implementation closeout:
- watch polling is now runtime-owned and long-lived behind `WatchRuntime`
- reconcile scheduling is now runtime-owned with ordered debounce/coalesce completion
- notifier delivery is now a daemon-owned queued runtime with typed unavailable/backpressure behavior

## Governing Requirements

- watch/reconcile/notifier requirements in `docs/requirements.md`
- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-RUNTIME-001` in `docs/atm-core/requirements.md`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`

## Governing Boundaries

- `BOUNDARY-WatchEventSource-File`
- `BOUNDARY-ReconcileCoordinator-Daemon`
- `BOUNDARY-NotificationSink`

## Prerequisites

- `R.13` runtime lifecycle is complete
- `R.14` storage contract is complete

## Hard Dependencies

- `R.17` should not start before the daemon lifecycle root exists; these are long-lived background lanes

## Non-Goals

- peer transport
- heartbeat/status cache
- doctor health projection

## Sub-Tasks

1. Runtime watch loop
   Development work:
   - replace one-shot `poll_watch(...)` behavior with runtime-owned watch subscription lifecycle
   - add bounded wake/poll/degradation behavior
   Required tests:
   - deterministic watch event delivery tests
   - degradation / restart tests for watcher failure
   Required doc or boundary updates:
   - update watch boundary docs if a runtime watcher controller facade is introduced

2. Runtime reconcile scheduler
   Development work:
   - replace one-shot reconcile helper with runtime-owned scheduling, debounce, and coalescing
   - keep reconcile out of direct store/transport bypasses
   Required tests:
   - debounce/coalesce tests
   - trigger/completion ordering tests
   Required doc or boundary updates:
   - update reconcile ownership prose in requirements/architecture/boundaries

3. Notifier/plugin delivery
   Development work:
   - replace the logging-only notification placeholder with a real daemon-owned notifier adapter
   - define failure/degradation handling for local plugin/agent notification traffic
   Required tests:
   - notifier success/failure/degraded-path tests
   - no-direct-plugin-bypass boundary tests
   Required doc or boundary updates:
   - update notifier/plugin runtime sections and any related boundary inventory

## Split Recommendation

If the notifier runtime is blocked on separate plugin work, split it late. The watch and reconcile lanes should still land together because they share runtime ownership and event-flow semantics.

## Acceptance Criteria

- watch behavior is runtime-owned and long-lived rather than one-shot path discovery
- reconcile scheduling uses debounce/coalesce semantics and does not reach around store/transport boundaries
- notifier delivery is a real daemon-owned adapter with typed degraded/failure handling
- none of the three lanes still forward production behavior into generic placeholder helpers

## Required Validation

- `cargo test -p atm-daemon`
- `cargo test --workspace`
- `just lint`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/plans/phase-R/issues.md`

## Risks And Watchouts

- watch/reconcile loops must not become hidden side-effect threads outside the lifecycle root
- notifier delivery must not silently degrade into log-only behavior again
- background tests need explicit ready signals, not polling races
