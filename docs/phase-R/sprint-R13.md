# Phase R.13 — Runtime Admission And Lifecycle

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.13"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s13-runtime-admission
branch: feature/pR-s13-runtime-admission
status: planned
estimated_scope: L
```

## Goal

Finish host-wide singleton enforcement and replace the current daemon startup/shutdown scaffold with a real runtime lifecycle root.

## Scope Summary

This sprint closes the current runtime-admission gap: host-wide launch/serve ownership, typed singleton rejection paths, explicit runtime lifecycle transitions, and shutdown behavior that is owned by one runtime root instead of scattered socket helpers.

## Governing Requirements

- `REQ-P-RUNTIME-001`
- `REQ-P-RUNTIME-002`
- `REQ-CORE-RUNTIME-001`
- `REQ-CORE-TEST-RUNTIME-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`

## Governing Boundaries

- `BOUNDARY-ServerTransport-Socket`
- `BOUNDARY-ClientTransport`
- `BOUNDARY-RequestDispatcher-Daemon`

## Prerequisites

- `sc-lint` inventory-parity and planning-metadata gates are available for use as the Phase R continuation implementation gate.
- Process gates `PG-001` through `PG-004` from `docs/phase-R/issues.md` are approved and in effect before coding starts.

## Hard Dependencies

- starts from `integrate/phase-R` after the planning-doc set on `feature/pR-s11-planning` is accepted
- no downstream runtime-lane sprint (`R.15` through `R.17`) should start before `R.13` lands

## Non-Goals

- heartbeat/status cache behavior
- doctor runtime health projection
- peer delivery
- watch/reconcile runtime

## Sub-Tasks

1. Host-wide admission gate
   Development work:
   - replace socket-derived launch and singleton lock paths with one host-wide ownership root independent of `ATM_HOME` and `ATM_DAEMON_SOCKET`
   - add and wire `ATM_DAEMON_LAUNCH_GATE_REJECTED`, `ATM_DAEMON_SERVING_STATE_REJECTED`, `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED`, and `ATM_DAEMON_AUTO_START_FAILED`
   Required tests:
   - cross-config test proving two different `ATM_HOME` / socket paths still contend on one host-wide gate
   - typed-error tests covering launch-gate rejection and serving-gate rejection
   Required doc or boundary updates:
   - update singleton path examples in requirements/architecture if the final host-wide lock root differs from current planning prose

2. Runtime lifecycle root
   Development work:
   - implement `RuntimeComposition::start()` as the only legal daemon bootstrap path
   - add explicit runtime lifecycle state (`Starting`, `Running`, `Draining`, `Stopped`) and prevent illegal transitions
   - route `run_daemon()` through the new runtime lifecycle root instead of directly calling `serve()`
   Required tests:
   - unit tests for legal/illegal lifecycle transitions
   - integration test proving failed startup transitions `Starting -> Stopped` with typed error
   Required doc or boundary updates:
   - update `docs/atm-daemon/architecture.md` and `docs/atm-daemon/boundaries.md` if the lifecycle controller becomes its own documented runtime facade

3. Shutdown hardening
   Development work:
   - remove `expect(...)` from `DaemonShutdownSignals::install()`
   - wire force-cancel to interrupt blocked connection threads instead of only polling to `process::exit(1)`
   - keep shutdown logging and state transitions under the lifecycle root
   Required tests:
   - deterministic test for blocked-read interruption on force-cancel
   - no-panic test around repeated signal-install attempts
   Required doc or boundary updates:
   - document the final graceful-drain and force-cancel behavior in the daemon architecture if it differs from current prose

## Split Recommendation

Do not split unless the lifecycle state machine lands cleanly but the host-wide path migration stalls. If forced, split into:
- `R.13.1` host-wide gate + error-code wiring
- `R.13.2` runtime lifecycle + shutdown hardening

## Acceptance Criteria

- two daemon start attempts with different `ATM_HOME` or socket paths still resolve to one host-wide launch/serve ownership gate
- `run_daemon()` enters the daemon only through `RuntimeComposition::start()`
- lifecycle transitions are explicit and tested; `Running -> Starting` and `Stopped -> Running` without reinit are impossible
- startup and shutdown rejection paths return typed `AtmError` failures instead of panic/`expect(...)`
- the five singleton-related error codes exist in `error_codes.rs` and are emitted by real runtime rejection paths

## Required Validation

- `cargo test -p atm-daemon`
- `cargo test --workspace`
- `just lint`

## Required Document Updates

- `docs/phase-R/issues.md`
- `docs/plan-phase-R.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md` when lifecycle/control-plane facades change

## Risks And Watchouts

- fixing only one of the two lock paths recreates split ownership under alternate config
- lifecycle code must not reintroduce daemon-internal bypasses around the dispatcher or transport boundaries
- shutdown tests must not rely on open-ended sleeps or timing luck

