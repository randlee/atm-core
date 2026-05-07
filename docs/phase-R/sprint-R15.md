# Phase R.15 — Heartbeat, Status Cache, And Doctor Health

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.15"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s15-status-heartbeat
branch: feature/pR-s15-status-heartbeat
status: planned
estimated_scope: L
```

## Goal

Implement the runtime-owned member heartbeat path, live status cache, and daemon-backed doctor health surface.

## Scope Summary

This sprint turns the current placeholder health/status layer into a real runtime subsystem: new heartbeat requests, runtime member-state ownership, a live daemon cache, and a doctor projection that distinguishes liveness from readiness.

## Governing Requirements

- `REQ-P-RUNTIME-002`
- `REQ-CORE-RUNTIME-001`
- daemon-health requirements in `docs/requirements.md`
- `docs/team-member-state.md`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-AtmProtocol`
- `BOUNDARY-RequestDispatcher-Daemon`
- `BOUNDARY-StatusSource`

## Prerequisites

- `R.13` lifecycle root is complete
- `R.14` storage and thread semantics are locked

## Hard Dependencies

- `R.15` should land before any production-ready team-lead/status-bot automation work that depends on active-member truth

## Non-Goals

- peer daemon transport
- watch/reconcile runtime
- notifier/plugin delivery

## Sub-Tasks

1. Heartbeat protocol family
   Development work:
   - add typed heartbeat request/response payloads to the shared protocol
   - add dispatcher routing and daemon handler entry for member heartbeats
   - model PID ownership conflict detection and typed takeover outcomes
   Required tests:
   - protocol encode/decode tests for heartbeat envelopes
   - handler tests for active/idle/offline transitions and PID conflict paths
   Required doc or boundary updates:
   - update `docs/atm-core/boundaries.md` if the protocol boundary grows new request families

2. Runtime-owned status cache
   Development work:
   - replace placeholder `snapshot_status()` with a daemon-memory status cache
   - carry durable PID continuity and runtime-owned `last_active_at`
   - define restart rebuild / eviction behavior, including startup hydration of
     configured members as explicit `unknown` and bounded demotion of evicted
     live entries back to `unknown`
   Required tests:
   - cache rebuild test on startup from durable state
   - bounded cache behavior / eviction tests
   Required doc or boundary updates:
   - update `docs/atm-daemon/architecture.md` and `docs/atm-daemon/boundaries.md` if cache ownership becomes its own runtime facade

3. Doctor health projection
   Development work:
   - replace direct `run_doctor()` passthrough with daemon-backed health projection
   - report singleton ownership, runtime liveness/readiness, status-cache summary,
     and degraded-ingest/backlog state
   - distinguish `ready`, `degraded`, and `unavailable` readiness outcomes from
     daemon-member runtime truth instead of collapsing them into one generic
     health status
   Required tests:
   - doctor integration tests for ready, degraded, and unavailable states
   - liveness/readiness split tests
   Required doc or boundary updates:
   - update doctor requirements and runtime-health architecture sections

## Split Recommendation

Keep heartbeat, status cache, and doctor together. Splitting them risks inventing a placeholder cache contract that doctor immediately depends on. If a split is unavoidable, make doctor follow heartbeat/cache, never precede it.

## Acceptance Criteria

- the protocol and dispatcher support a typed heartbeat request family
- the daemon owns a live member-state/status cache with durable PID continuity and runtime `last_active_at`
- doctor output is backed by daemon runtime state rather than a generic placeholder helper
- liveness and readiness are distinct and test-covered
- PID conflict and takeover paths return typed results instead of ambiguous generic failures

## Required Validation

- `cargo test -p atm-daemon`
- `cargo test --workspace`
- `just lint`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/team-member-state.md`
- `docs/phase-R/issues.md`

## Risks And Watchouts

- a status cache without heartbeat truth will just recreate the current placeholder problem
- doctor must not invent its own health state separate from the runtime cache
- PID ownership and takeover rules need exact typed results, not ad hoc strings
