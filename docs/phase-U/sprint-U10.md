# Sprint U.10 — Generic Daemon Advisory-Notification Surface

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.10"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u10-generic-advisory-notification
branch: feature/pU-u10-generic-advisory-notification
status: planned
estimated_scope: M
```

## Goal

Restack the abandoned earlier graft-notification work as a generic daemon
advisory-notification surface rather than graft-specific daemon ownership.

## Scope Summary

This sprint defines the generic post-commit notification/nudge surface that the
daemon may own. `atm-graft` is just one consumer of that surface.

Lean-design rule:
- the daemon emits one generic post-commit advisory signal
- the daemon keeps one bounded pending queue per registered consumer and one
  simple live advisory stream per active consumer session
- the client consumes that signal over the shared ICD family and the live
  advisory stream
- avoid extra daemon runtime layers or client-specific protocol concepts

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-TRANSPORT-001`
- `REQ-P-CONTRACT-001`
- `REQ-P-RELIABILITY-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-NotificationSink`
- `BOUNDARY-AtmProtocol`
- `BOUNDARY-RequestDispatcher`
- boundary lint must preserve plugin isolation from `atm-daemon`

## Prerequisites

- `U.8` and `U.9` are complete

## Hard Dependencies

- `U.8`
- `U.9`

## Non-Goals

- introducing daemon-owned client-specific concepts
- a separate client-specific protocol family

## Sub-Tasks

1. Define the generic post-commit notification surface
   Development work:
   - describe the daemon-owned post-commit advisory event flow
   - keep hook execution and advisory delivery behind generic boundaries
   - own the generic replacement of the remaining graft-named advisory/session
     daemon surfaces:
     - `GraftSessionPort`
     - `NudgeEvent`
     - `GraftNudgeFetchRequest`
     - `GraftNudgeDrainRequest`
   - keep the daemon contract production-simple: registration, notification
     delivery, bounded pending state, one live advisory stream per active
     session, and typed backpressure only
   - anchor the shared message family in the current `develop` protocol line:
     - `crates/atm-core/src/protocol.rs`
     - `docs/atm-daemon/protocol-icd.md`
     - current `develop @ b6506ef` reusable references:
       - `crates/atm-daemon/src/graft_runtime.rs`
       - `crates/atm-daemon/src/tests.rs`
       - `docs/atm-daemon/protocol-icd.md`
   Required tests:
   - delivery/error-path tests at the generic boundary
   - reuse/extend the current protocol and transport-style tests rather than
     inventing graft-only daemon fixtures
   Required doc or boundary updates:
   - update daemon/core architecture docs and ICD docs

2. Express client consumption generically
   Development work:
   - make the persistent advisory stream the production `atm-graft` delivery
     path
   - if registration/fetch/drain messages are still needed, define them as
     generic shared-ICD messages rather than graft-specific daemon APIs
   - keep fetch/drain as optional companion CLI/debug surfaces only
   - keep the advisory stream in the same shared ICD family rather than
     inventing a parallel client API
   Required tests:
   - protocol tests proving the messages remain part of the shared ICD line
   - boundary lint proving the client/plugin crate still does not reference
     `atm-daemon`
   Required doc or boundary updates:
   - update thin-client docs and protocol inventory

## Acceptance Criteria

- daemon owns only a generic advisory-notification surface
- `atm-graft` is documented as one consumer of that surface, not a daemon-owned
  subsystem
- any extra message family needed for advisory delivery remains part of the
  shared ICD used by CLI/thin clients
- the daemon-side shape stays lean: one generic advisory surface rather than a
  stack of client-specific runtime abstractions
- U.10 owns daemon-side generic replacement of `GraftSessionPort`,
  `NudgeEvent`, `GraftNudgeFetchRequest`, and `GraftNudgeDrainRequest`
- production embedded delivery is one live advisory stream per active session;
  fetch/drain may remain only as companion debug or CLI surfaces
- the plugin crate remains isolated from daemon implementation crates by lint
  as well as by design text

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/plan-phase-U.md`
- `docs/project-plan.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/phase-U/removal-inventory.md`

## Risks And Watchouts

- do not let “generic” naming hide client-specific daemon ownership
- do not introduce post-commit notifications that can outrun durable commit
- do not create a second protocol family when the shared ICD can carry the
  traffic
- do not build a notifier framework larger than the simple daemon-owned
  advisory-delivery problem requires
