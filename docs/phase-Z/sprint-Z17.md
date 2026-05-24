---
id: Z.17
title: `atm-dev` Canary And Dogfood
status: complete
branch: feature/pZ-s17-smoke-z3-rerun
worktree: ../atm-core-worktrees/feature/pZ-s17-smoke-z3-rerun
target: integrate/phase-Z
---

# Sprint Z.17 — `atm-dev` Canary And Dogfood

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.17
worktree: ../atm-core-worktrees/feature/pZ-s17-smoke-z3-rerun
branch: feature/pZ-s17-smoke-z3-rerun
status: complete
estimated_scope: medium
```

## Goal

Execute the `atm-dev` canary/dogfood pass on the accepted post-`Z.16`
integration baseline and truthfully record the `Z.3` verdict.

## Scope Summary

This sprint owns:

- freezing the active `atm-dev` participant list
- recording the accepted binary baseline under canary evaluation
- executing retained command, send, and read flows on the live `atm-dev` team
- stamping the `Z.3` readiness row if the canary closes cleanly

This sprint does not fix findings or produce the final release verdict.

## Governing Requirements

- `REQ-CORE-DAEMON-001`
- `REQ-CORE-RUNTIME-001`
- `REQ-CORE-TEAM-001`
- `REQ-CORE-BOUNDARY-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RequestDispatcher`
- `RosterStore`
- `SqliteWriter`

## Prerequisites

- `Z.2` complete
- `Z.11` through `Z.16` complete
- accepted canary binary baseline merged to `integrate/phase-Z`

## Hard Dependencies

- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Exact Targets

- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`
- `docs/plan-phase-Z.md`
- `docs/phase-Z/sprint-Z17.md`

## Delete / Narrow Inventory

- keep `Z.17` scoped to canary execution and evidence capture only
- do not widen into release-signoff or new product-fix work

## Non-Goals

- no `Z.4` release checklist execution
- no code changes unless canary execution exposes a real blocker

## Sub-Tasks

1. Freeze the canary baseline and participant list.
   Development work:
   - use `integrate/phase-Z @ 97518da5` as the accepted binary baseline
   - freeze the active participant minimum as `team-lead` and `arch-ctm`
   Required docs:
   - update `docs/phase-Z/canary-dogfood-checklist.md`

2. Execute canary operator flows on `atm-dev`.
   Development work:
   - run `atm doctor --json`
   - run `atm teams --json`
   - run `atm members --team atm-dev --json`
   - run `atm send team-lead ... --requires-ack --json`
   - run `atm read --all --json`
   - run `atm log snapshot --json`
   Required docs:
   - update `docs/phase-Z/canary-dogfood-checklist.md`
   - update `docs/phase-Z/canary-findings-ledger.md`

3. Stamp closure records.
   Development work:
   - record the final `Z.3` verdict
   - add the `Z.17` sprint ledger row
   Required docs:
   - update `docs/phase-Z/readiness.md`
   - update `docs/project-plan.md`
   - update `docs/plan-phase-Z.md`

## Split Recommendation

If the canary exposes a validated product issue that needs code changes, stop
and promote it to `Z.4` instead of widening `Z.17`.

## Acceptance Criteria

- `docs/phase-Z/sprint-Z17.md` exists with `status: complete`
- `docs/phase-Z/canary-dogfood-checklist.md` records the frozen participant
  list, binary baseline, operator-report path, and final verdict rows
- `docs/phase-Z/canary-findings-ledger.md` truthfully records the canary
  findings state for this sprint
- `docs/phase-Z/readiness.md` records `Z.3` with a non-`PENDING` verdict
- `cargo build --release -p agent-team-mail -p atm-daemon` passes
- `cargo test --workspace` passes
- `python3 .just/run_lint.py all` passes

## Non-Closure

- `Z.17` does not fix canary findings
- `Z.17` does not produce the final release verdict

## Production-Ready Expectation

The canary checklist and findings ledger must be immediately usable by `Z.4`
without manual reconstruction of participants, baseline, or verdicts.

## Required Validation

- `cargo build --release -p agent-team-mail -p atm-daemon`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`
- `docs/plan-phase-Z.md`
- `docs/phase-Z/sprint-Z17.md`

## Risks And Watchouts

- keep the participant list frozen once recorded
- do not treat unrelated historical mailbox warnings from other teams as
  `atm-dev` canary findings
- if a real `atm-dev` canary issue appears, promote it explicitly rather than
  hiding it in checklist notes

## Execution Notes

- accepted binary baseline under evaluation: `97518da5`
- frozen participant minimum:
  - `team-lead`
  - `arch-ctm`
- operator-report path:
  - record validated canary issues in `docs/phase-Z/canary-findings-ledger.md`
  - report branch status to `team-lead` over ATM
- canary evidence summary:
  - `atm doctor --json` completed with warning-only status and no runtime
    errors
  - `atm teams --json` and `atm members --team atm-dev --json` succeeded on
    the accepted baseline
  - `atm send team-lead "z17 canary baseline check from arch-ctm" --requires-ack --json`
    succeeded
  - `atm read --all --json` and `atm log snapshot --json` both succeeded on
    the accepted baseline
