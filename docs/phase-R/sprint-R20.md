# Phase R.20 — Daemon Partitioning And Enforcement Hardening

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.20"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s20-daemon-partitioning
branch: feature/pR-s20-daemon-partitioning
status: planned
estimated_scope: L
```

## Goal

Use the post-`PR #200` integrated daemon review as the basis for one cleanup
planning sprint that:
- partitions `atm-daemon` into smaller daemon-private ownership modules
- tightens architecture, requirements, and boundary docs so the partitioning
  work is enforceable
- removes vague or conflicting language before any implementation sprint starts

## Scope Summary

This is a planning-and-documentation sprint only. It does not implement the
partitioning itself. It defines the daemon cleanup sprint that follows the
integrated `phase-R` review and hardens the docs until they are consistent with
the production-ready daemon target.

## Governing Requirements

- `REQ-P-RUNTIME-002`
- `REQ-DAEMON-RUNTIME-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-DAEMON-RUNTIME-003`
- `REQ-DAEMON-RUNTIME-004`
- `REQ-DAEMON-CONFIG-001`
- `REQ-DAEMON-HEALTH-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `docs/adr/ADR-006-sighup-reload-deferral.md`

## Governing Boundaries

- `BOUNDARY-ServerTransport-Socket`
- `BOUNDARY-RequestDispatcher-Daemon`
- `BOUNDARY-StatusSource-Daemon`
- `BOUNDARY-WatchEventSource-File`
- `BOUNDARY-ReconcileCoordinator-Daemon`
- `BOUNDARY-NotificationSink-Daemon`

## Prerequisites

- `R.13` through `R.18` are merged into `integrate/phase-R`
- planning is based on the post-`PR #200` integrated daemon state, not the
  earlier `R.18` implementation branch in isolation

## Non-Goals

- implementing the daemon partition directly
- changing product behavior outside the daemon cleanup scope
- introducing a new public composition crate in this sprint

## Planning Inputs

The planning review must start from the integrated daemon code and current
documentation set:
- `crates/atm-daemon/src/lib.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-daemon/requirements.md`
- `docs/plan-phase-R.md`

Known review themes that the plan must address:
- singleton ownership teardown safety
- request-work ownership and shutdown accounting
- background-lane startup/shutdown rollback behavior
- status-cache cap semantics
- overgrown daemon-private modules and mixed responsibilities

## Sub-Tasks

1. Integrated-state daemon review
   Development work:
   - review the integrated daemon after `PR #200`, not the earlier sprint
     branches
   - classify the current daemon issues into:
     - control-plane ownership
     - server runtime / drain ownership
     - request execution ownership
     - runtime status / reload / doctor projection
     - background-lane lifecycle
   Required outputs:
   - one explicit ownership map of the current daemon-private surfaces
   - one explicit issue list that the cleanup sprint will resolve

2. Daemon partitioning plan
   Development work:
   - define the target daemon-private module partition
   - assign owned responsibilities for each partition
   - state which current files/types move into each target module
   Required outputs:
   - target partitions must include at least:
     - singleton / ownership admission
     - server runtime / connection registry / drain
     - request execution ownership
     - runtime status cache / reload hydration / doctor projection
     - peer transport
     - watch runtime
     - reconcile runtime
     - notification runtime

3. Enforcement hardening
   Development work:
   - tighten `docs/atm-daemon/architecture.md`
   - tighten `docs/atm-daemon/requirements.md`
   - tighten `docs/atm-daemon/boundaries.md`
   - update `docs/plan-phase-R.md` so the daemon follow-on sprint is part of
     the authoritative Phase R continuation plan
   Required outputs:
   - explicit rules for singleton cleanup safety
   - explicit rules for tracked request work
   - explicit rules for rollback-safe background-lane lifecycle
   - explicit rules for true bounded status-cache capacity
   - explicit partition names and ownership lines

4. Plan-hardening loop
   Development work:
   - review the plan, daemon docs, and current code together
   - look for:
     - missing requirements or boundaries
     - contradictory statements
     - vague ownership language
     - acceptance criteria that are not testable or reviewable
   - patch the docs
   - repeat until the daemon planning set is internally consistent
   Required outputs:
   - one hardened document set that is ready to drive the follow-on daemon
     cleanup sprint without hidden assumptions

## Acceptance Criteria

- the integrated daemon review is reflected in the sprint plan rather than
  implied from chat or ATM history
- the target daemon-private partition map is explicit and names the current
  files/types that each partition owns
- architecture, requirements, and boundaries agree on:
  - singleton teardown safety
  - tracked request-work ownership
  - rollback-safe background-lane lifecycle
  - true bounded status-cache semantics
- the plan-hardening loop has removed vague or conflicting language that would
  make the cleanup sprint underspecified
- the final planning docs are consistent with a production-ready daemon target
  and are strict enough to support later QA/lint enforcement

## Required Validation

- `just lint`
- one final doc/code consistency review pass before push

## Required Document Updates

- `docs/plan-phase-R.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-daemon/requirements.md`

## Risks And Watchouts

- do not let the sprint stay at the “split big files” level; ownership and
  invariants are the primary design problem
- do not document fictional boundaries that are not intended to become review
  surfaces
- do not leave the singleton or request-work ownership rules implicit
- do not declare Phase R fully production-ready while the daemon partitioning
  and enforcement gaps remain unplanned
