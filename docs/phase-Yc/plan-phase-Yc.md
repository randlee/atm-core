# Phase Yc Plan

## Goal

Close the final production-readiness gaps left on `integrate/phase-Y` after the
merged `Yb` line so `Phase Z` smoke/dogfood work resumes only after the
remaining delivery-contract and notification-boundary issues are actually
closed.

This is a planning-only phase on a worktree off `develop`. It does not start
implementation.

## Baseline

- planning branch: `plan/phase-Yc-y12-y13`
- planning worktree:
  `../atm-core-worktrees/plan/phase-Yc-y12-y13`
- branch base: `develop` at `812059b8`
- implementation baseline under review:
  `integrate/phase-Y` at `4d6bd883`
- `integrate/phase-Y` already includes:
  - merged `Yb` work through `Y.11`
  - the `PYB-PRR-1` retained-runtime factory wiring fix
- implementation target for approved `Yc` work remains:
  `integrate/phase-Y`

## Planning Rule

- every deliverable committed by `Yc` must land at a production-ready level for
  the scope its sprint claims
- no deliverable may be dropped, weakened into prose-only guidance, or silently
  deferred after implementation begins
- if a sprint cannot carry all of its deliverables at that level, the sprint
  must be split before implementation begins
- important traits, features, enums, protocol types, and boundary contracts
  must be shown with explicit code samples in the sprint docs

## Remaining Production-Readiness Blockers

1. `crates/atm-core/src/delivery_execution.rs` still allows partial Claude
   degraded delivery after SQLite failure.
   - `message[1]` can append
   - `message[2]` can fail
   - the runtime warns and continues
   - this violates the ADR-013 identical logical payload contract
2. `crates/atm-core/src/delivery_execution.rs` still routes notification side
   effects through `maybe_run_post_send_hook(...)` directly instead of the
   `NotificationSink` boundary.
   - the architecture already says notification is a boundary-owned side effect
   - the live executor path still bypasses that boundary

## Sprint Sequence

### Y.12 Claude Degraded Delivery Set Closure

Purpose:

- close the last remaining behavioral hole in the Claude compatibility path
- require one explicit compatibility-export seam that either materializes the
  full logical message set after SQLite failure or fails hard before the sprint
  can claim success

Authoritative sprint doc:
- [sprint-Y12.md](./sprint-Y12.md)

### Y.13 Notification Boundary Closure And Final Readiness Gate

Purpose:

- move production notification execution onto `NotificationSink`
- remove the direct post-send-hook helper bypass from the delivery executor
- prove the integrated `Phase Y` line is ready to hand back to `Phase Z`

Authoritative sprint doc:
- [sprint-Y13.md](./sprint-Y13.md)

## Exit Condition

Phase `Yc` closes only when:

- `Y.12` proves Claude SQLite-failure degraded delivery cannot partially emit a
  logical message set while still claiming success
- `Y.13` proves the production notification path executes through
  `NotificationSink`, not direct hook helpers
- the integrated line can pass a focused production-readiness review without
  reopening the same two issues
