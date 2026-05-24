---
id: Z.22
title: Smoke Findings Review And Major Rework Triage
status: complete
branch: feature/pZ-s22-smoke-findings-review-and-major-rework-triage
worktree: ../atm-core-worktrees/feature/pZ-s22-smoke-findings-review-and-major-rework-triage
target: integrate/phase-Z
---

# Sprint Z.22 — Smoke Findings Review And Major Rework Triage

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.22
worktree: ../atm-core-worktrees/feature/pZ-s22-smoke-findings-review-and-major-rework-triage
branch: feature/pZ-s22-smoke-findings-review-and-major-rework-triage
status: complete
estimated_scope: small
```

## Goal

- provide the durable place to record smoke findings that are too large to fix
  inside the active smoke sprints
- separate minor in-sprint fixes from significant rework
- connect accepted smoke findings to readiness, canary entry, and binary
  baseline records

## Scope Summary

This sprint is the authoritative review and triage layer for larger smoke
findings. It exists so the fast, normal, and thorough smoke sprints can fix
small local blockers directly without pretending they also own major redesign
or cross-cutting rework.

## Governing Requirements

- `REQ-P-SMOKE-002`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RuntimeFactory`

## Prerequisites

- `Z.21` complete

## Hard Dependencies

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/phase-Z/smoke-findings-review.md`

## Exact Targets

- `docs/phase-Z/smoke-findings-review.md`
- `docs/phase-Z/readiness.md`
- `docs/plan-phase-Z.md`
- `docs/project-plan.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint,
the sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- authoritative smoke findings review artifact
- explicit distinction between in-sprint minor fixes and promoted major rework
- readiness linkage for accepted binary-baseline notes and major smoke findings
- clear disposition rules for follow-on execution after the smoke line

## Required Work

- define the authoritative finding record fields
- define what qualifies as an in-sprint fix versus promoted rework
- define how accepted smoke findings are reflected in readiness and later
  canary/release evidence
- ensure promoted findings have an explicit recommended sprint or owner

## Canonical Finding Record

Every promoted finding in `docs/phase-Z/smoke-findings-review.md` must use
this canonical serialization format:

```json
{
  "finding_id": "SMOKE-FIND-001",
  "smoke_level": "fast",
  "flow_or_command": "atm send --requires-ack z1-recipient \"hello\" --json",
  "observed_behavior": "send succeeded but retained logs omitted delivery_policy.new_message.primary_nudge",
  "expected_behavior": "successful ack-required send emits the primary_nudge event in retained logs",
  "root_cause": "smoke-fast debug logging path does not emit the expected nudge event on the durable happy path",
  "disposition": "promote-follow-on",
  "recommended_sprint": "Z.19",
  "owner": "atm-dev",
  "notes": "capture retained log artifact path and follow-on logging fix recommendation"
}
```

## This Sprint Does Not Close

- executing the rework for promoted findings
- canary/dogfood execution
- final release sign-off

## Acceptance Criteria

- `docs/phase-Z/smoke-findings-review.md` exists as the single authoritative
  major-findings queue
- the artifact includes the canonical concrete finding-record format shown in
  this sprint plan
- the artifact distinguishes minor in-sprint fixes from promoted major rework
- readiness linkage for smoke findings and accepted binary notes is explicit
- the project plan and phase plan both reference the findings-review artifact
  consistently

## Required Validation

- `git diff --check`

## Split Recommendation

If the sprint starts absorbing actual code fixes for promoted major findings,
stop and create explicit follow-on implementation sprints instead.

## Production-Ready Expectation

Every listed `Z.22` deliverable is expected to land at a production-ready
level for findings triage and readiness linkage.

## Required Document Updates

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/plan-phase-Z.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not let the findings-review artifact turn into a second informal sprint
  plan
- keep promoted findings explicit enough that ownership can be assigned
  without rereading smoke output

## Execution Notes

- accepted upstream smoke line:
  - `Z.19 @ bd301ae7`
  - `Z.20 @ 63e9edc8`
  - `Z.21 @ e9e395b1`
- the authoritative findings-review artifact is still empty after the accepted
  `fast`, `normal`, and `thorough` smoke runs
- no smoke discrepancy in the accepted execution line exceeded the
  `fix-in-active-sprint` threshold, so no promoted `Z.22` follow-on record was
  required for this batch
