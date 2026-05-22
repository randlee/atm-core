---
id: Z.2
title: Fix And Revalidate
status: complete
branch: feature/pZ-s2-fix-and-revalidate
worktree: ../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate
target: integrate/phase-Z
---

# Sprint Z.2 — Fix And Revalidate

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.2
worktree: ../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate
branch: feature/pZ-s2-fix-and-revalidate
status: complete
estimated_scope: medium
```

## Goal

Close the verified smoke findings from `Z.1` and re-run the executable
validation set on the fixed branch.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `Z.1` smoke results are the only finding source for this sprint

## Prerequisites

- `Z.1` complete
- the validated `Z.1` smoke findings ledger is frozen

## Exact Targets

- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- the approved fix branch on `integrate/phase-Z`

## Deliverables

- fixed branch containing only `Z.1` finding closure work
- updated `docs/phase-Z/smoke-findings-ledger.md` with fix/defer disposition
  for every carried `Z.1` finding
- smoke revalidation result on the fixed branch

## Required Work

- fix only the findings promoted from `Z.1`
- only findings recorded in `docs/phase-Z/smoke-findings-ledger.md` are in
  scope; newly discovered issues found during `Z.2` must be recorded but not
  fixed in this sprint
- keep the branch aligned with the approved `Phase Y` architecture and state
  machines
- rerun the frozen `docs/phase-Z/smoke-checklist.md` checklist after fixes
  land

## Acceptance Criteria

- every `Z.1` finding is either fixed or explicitly deferred with `team-lead`
  approval recorded in `docs/phase-Z/smoke-findings-ledger.md`
- any deferred `Z.1` finding is resolved or explicitly waived by `team-lead`
  before `Z.3` may begin
- the smoke checklist is rerun on the fixed branch
- all checklist rows that passed in `Z.1` still pass after the `Z.2` fixes;
  any new failure is a blocking finding for this sprint
- `docs/phase-Z/smoke-findings-ledger.md` records final per-finding
  disposition and revalidation outcome

## Non-Closure

- `Z.2` does not widen the smoke checklist
- `Z.2` does not begin canary or release-signoff work

## Production-Ready Expectation

Every listed `Z.2` deliverable is expected to land at a production-ready level
for the smoke-fix scope this sprint claims: the branch must be suitable for
promotion into canary, and the updated smoke findings ledger must fully close
the `Z.1` handoff without silent carry-forward.

## Required Validation

- rerun `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `cargo test --workspace`
- `git diff --check`
