# Phase Yd Plan

## Goal

Close the remaining `Phase Y` blockers before the line lands on `develop`, and
leave one explicit readiness record that unblocks `Phase Z`.

This is a planning-only phase on a worktree off `develop`. It does not start
implementation on this branch.

## Baseline

- planning branch: `plan/phase-Yd-z-gate`
- planning worktree:
  - `../atm-core-worktrees/plan/phase-Yd-z-gate`
- branch base:
  - `develop` at `fa6428bb`
- implementation baseline under review:
  - `integrate/phase-Y`
- blocking issue inventory:
  - [../phase-Y/issues.md](../phase-Y/issues.md)

## Governing Documents

Phase `Yd` hardening and implementation must remain aligned with:

- product requirements and architecture:
  - `docs/requirements.md`
  - `docs/architecture.md`
- crate requirements, architecture, and boundary inventories:
  - `docs/atm-core/requirements.md`
  - `docs/atm-core/architecture.md`
  - `docs/atm-core/boundaries.md`
  - `docs/atm-daemon/requirements.md`
  - `docs/atm-daemon/architecture.md`
  - `docs/atm-daemon/boundaries.md`
- ADR inventory and controlling decisions:
  - `docs/adr/INDEX.md`
  - `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- machine-readable boundary definitions for the concrete closures in scope:
  - `boundaries/atm-core/inbox-export.toml`
  - `boundaries/atm-core/notification-sink.toml`
  - `boundaries/atm-core/status-source.toml`
  - `boundaries/atm-daemon/daemon-notification-sink.toml`
  - `boundaries/atm-daemon/daemon-status-source.toml`
- testing and validation guidance:
  - `docs/testing-guidelines.md`
- issue and readiness tracking:
  - `docs/phase-Y/issues.md`
  - `docs/phase-Yd/readiness.md`

If any sprint-level deliverable cannot be justified against this governing set,
the sprint is not hardened yet.

## Scope

`Phase Yd` is not a new architecture line. It is a develop-gate closeout line.

It exists to:

- close the remaining `Phase Y` runtime, boundary, composition, and readiness
  blockers
- absorb the accepted end-of-phase fix line into the final merge candidate
- leave a named readiness record that says whether `Phase Y` may land on
  `develop`
- keep `Phase Z` blocked until that record says the line is ready

It does not exist to:

- redesign the `Phase Y` architecture again
- reopen broad `Phase Z` rollout planning inside the closeout sprints
- add unrelated lint-only or docs-only reinterpretations of prior `Y.12` /
  `Y.13` runtime deliverables

## Sprint Sequence

### Y.14 Recovered Claude Logical-Message-Set Closure

Purpose:

- close the recovered Claude behavioral correctness blocker on the final
  `Phase Y` line

Authoritative sprint doc:

- [sprint-Y14.md](./sprint-Y14.md)

### Y.15 Production Notification Boundary Closure

Purpose:

- close the production `NotificationSink` boundary bypass on the final
  `Phase Y` line

Authoritative sprint doc:

- [sprint-Y15.md](./sprint-Y15.md)

### Y.16 Retained-Runtime Composition Closure

Purpose:

- close the retained-runtime composition blocker

Authoritative sprint doc:

- [sprint-Y16.md](./sprint-Y16.md)

### Y.17 Candidate Closure

Purpose:

- verify the accepted `Phase Y` merge candidate includes the required
  end-of-phase fix line and is validation-clean

Authoritative sprint doc:

- [sprint-Y17.md](./sprint-Y17.md)

### Y.18 Thin Liveness Closure And Final Develop Gate

Purpose:

- leave the final `Phase Y` develop-gate readiness record
- prove or explicitly reclassify the minimal operational/liveness requirement
  without adding logic bloat to `runtime_health`
- explicitly unblock `Phase Z` only if the line is actually ready

Authoritative sprint doc:

- [sprint-Y18.md](./sprint-Y18.md)

Named readiness record produced by `Y.18`:

- [readiness.md](./readiness.md)

## Exit Condition

`Phase Yd` closes only when:

- the blockers in [../phase-Y/issues.md](../phase-Y/issues.md) are either
  closed or explicitly reclassified as non-blocking with documented rationale
- the final accepted `Phase Y` merge candidate includes the required
  end-of-phase fixes
- the `Phase Yd` readiness record says the line is ready to land on `develop`
- `Phase Z` is explicitly unblocked by that readiness record
