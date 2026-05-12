# Sprint U.9 — Client-Owned Graft Runtime

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.9"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u9-client-owned-graft-runtime
branch: feature/pU-u9-client-owned-graft-runtime
status: planned
estimated_scope: M
```

## Goal

Restack the abandoned earlier graft-runtime work so all client-specific
receive-loop, injection, and host-integration behavior stays in `atm-graft`,
not in the daemon.

## Scope Summary

This sprint redraws ownership so the daemon stays generic while `atm-graft`
owns its own client runtime behavior on top of shared `atm-core` contracts.

Lean-design rule:
- one persistent receive thread
- one open daemon nudge connection
- one minimal client-side pending queue until host consumption
- one host wake/event path
- no extra runtime layers beyond those required for production behavior

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-DAEMON-002`
- `REQ-P-CONTRACT-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-ClientTransport`
- `BOUNDARY-AtmProtocol`
- `BOUNDARY-NotificationSink`
- boundary lint must continue to forbid `atm-graft` -> `atm-daemon`

## Prerequisites

- `U.8` is complete

## Hard Dependencies

- `U.8`

## Non-Goals

- generic daemon notification registration design
- SQLite/query cleanup
- roster/member redesign

## Sub-Tasks

1. Move client-specific runtime ownership
   Development work:
   - keep receive-loop, injection, host-facing queueing, and host wake/event
     logic in `atm-graft`
   - remove any daemon-owned client-named runtime concept
   - require exactly one persistent receive thread per active session and keep
     the daemon nudge connection open while the session is active
   - require a minimal client-side pending queue until the host consumes the
     nudge and a host wake/event callback when new nudges arrive during host
     inactivity
   - start from the reusable current-develop seams rather than inventing new
     daemon-private ones:
     - `crates/atm-core/src/transport/testing.rs` `FakeClientTransport`
     - `crates/atm-core/src/transport/testing.rs` `LoopbackClientTransport`
     - `crates/atm-daemon/src/test_support.rs` `DoctorOnlyDispatcher`
   Required tests:
   - client-runtime tests in the owning thin client crate
   - targeted integration tests modeled after the current daemon local-IPC
     tests in `crates/atm-daemon/src/local_ipc_transport.rs`
   Required doc or boundary updates:
   - update architecture docs to state that client-specific runtime logic is
     owned by the client crate

2. Leave only generic daemon responsibilities
   Development work:
   - ensure daemon responsibilities remain request serving, post-commit
     notification, and generic runtime composition only
   Required tests:
   - boundary/lint checks or review-driven tests proving no daemon-owned
     client runtime leak
   Required doc or boundary updates:
   - tighten daemon and core boundary docs

## Acceptance Criteria

- client-specific graft runtime logic is owned by `atm-graft`
- daemon does not own a graft-named runtime concept
- shared interfaces needed by `atm-graft` live in `atm-core`, not `atm-daemon`
- the client runtime is production-simple: one persistent receive thread, one
  open daemon nudge connection, one minimal pending queue, one host wake/event
  path
- the owning plugin crate still passes boundary lint with no `atm-daemon`
  dependency

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/plan-phase-U.md`
- `docs/project-plan.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/phase-U/removal-inventory.md`

## Risks And Watchouts

- do not keep client runtime ownership in the daemon under generic-looking
  names
- do not let shared helpers in `atm-core` become a disguised daemon
  implementation surface
- do not add scheduler/framework abstraction layers that hide a simple
  socket-to-action runtime path
