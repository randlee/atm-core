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
- the temporary graft-named daemon substrate used by `U.9` must be cleaned up
  here rather than preserved as the final architecture

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
   - PRIORITY-1: removing the `atm-daemon` reference introduced by `U.9` is
     the top-priority deliverable of `U.10`; all other work is secondary to
     this removal
   - `U.9` was granted a conditional architectural exception to reference
     `atm-daemon`'s `graft_runtime`; `U.10` must unconditionally close that
     exception by removing all `atm-graft` -> `atm-daemon` references before
     this sprint is considered complete
   - describe the daemon-owned post-commit advisory event flow
   - keep hook execution and advisory delivery behind generic boundaries
   - treat the current graft-named daemon/session substrate as temporary
     compatibility only; `U.10` must delete or generify it rather than let it
     survive Phase U unchanged
   - own the generic replacement of the remaining graft-named advisory/session
     daemon surfaces:
     - `GraftSessionPort`
     - `NudgeEvent`
     - `GraftNudgeFetchRequest`
     - `GraftNudgeDrainRequest`
   - explicitly clean up the daemon-owned graft-specific implementation line
     that `U.9` temporarily depends on:
     - `crates/atm-daemon/src/graft_runtime.rs`
     - `crates/atm-daemon/src/tests_graft.rs`
     - graft-named daemon handling in `crates/atm-daemon/src/runtime_health.rs`
     - graft-named request/response variants and message kinds in
       `crates/atm-core/src/protocol.rs`
     - graft-named daemon protocol inventory entries in
       `docs/atm-daemon/protocol-icd.md`
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
   - add a lint boundary rule under `boundaries/atm-graft/` prohibiting
     `atm-graft` from referencing `atm-daemon`; the rule must be present in
     the `sc-lint-boundary` configuration so that `just lint` enforces it
     automatically
   - verify the boundary rule is enforced: `just lint` must fail if
     `atm-graft` imports `atm-daemon` after `U.10`
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
- the temporary graft-named daemon substrate used by `U.9` is gone or fully
  genericized by the end of `U.10`; no daemon-owned graft runtime line is left
  in place as an accidental permanent boundary
- `boundaries/atm-graft/` contains an explicit deny rule for `atm-daemon`
  references
- `just lint` enforces the `atm-graft` -> `atm-daemon` prohibition
- zero `atm-daemon` references exist in the `atm-graft` crate after `U.10`
- the daemon-side shape stays lean: one generic advisory surface rather than a
  stack of client-specific runtime abstractions
- `docs/atm-core/requirements.md` and `docs/atm-daemon/requirements.md`
  finalize retirement of graft-named packet-family language in favor of the
  shared ICD plus generic advisory-surface wording
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
- `docs/atm-core/requirements.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/phase-U/removal-inventory.md`

## Risks And Watchouts

- do not let “generic” naming hide client-specific daemon ownership
- do not introduce post-commit notifications that can outrun durable commit
- do not create a second protocol family when the shared ICD can carry the
  traffic
- do not build a notifier framework larger than the simple daemon-owned
  advisory-delivery problem requires
