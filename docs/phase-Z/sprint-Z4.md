---
id: Z.4
title: Final Fixes And Release Sign-Off
status: complete
branch: feature/pZ-smoke-atm-graft
worktree: ../atm-core-worktrees/integrate/atm-core-worktrees/feature/pZ-smoke-atm-graft
target: integrate/phase-Z
---

# Sprint Z.4 — Final Fixes And Release Sign-Off

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.4
worktree: ../atm-core-worktrees/integrate/atm-core-worktrees/feature/pZ-smoke-atm-graft
branch: feature/pZ-smoke-atm-graft
status: complete
estimated_scope: medium
```

## Goal

Close the `atm-dev` canary handoff, rerun the final validation set, and
produce the final release-readiness verdict.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `Z.3` canary findings are the only input finding source for this sprint
- `Z.23` must have closed the coverage-report line first

## Prerequisites

- `Z.3` complete
- `Z.23` complete
- the canary findings ledger is frozen

## Exact Targets

- `docs/phase-Z/release-checklist.md`
- `docs/phase-Z/readiness.md`
- the `integrate/phase-Z` closeout candidate under final release review

## Deliverables

- fixed closeout branch containing only `Z.3` finding closure work
- `docs/phase-Z/release-checklist.md` populated with the final executable
  validation and release checklist result
- `docs/phase-Z/readiness.md` populated with the final release-ready /
  not-ready verdict and evidence

## Required Work

- fix only the findings promoted from `Z.3`
- only findings recorded in `docs/phase-Z/canary-findings-ledger.md` are in
  scope; newly discovered issues found during `Z.4` must be recorded but not
  fixed in this sprint, using the `notes` field and `status: out_of_scope`
  marker in `docs/phase-Z/canary-findings-ledger.md`
- rerun `docs/phase-Z/release-checklist.md`
- produce the final release-ready / not-ready decision with evidence in
  `docs/phase-Z/readiness.md`

## Outcome

- `Z.17` promoted no validated `Z.3` findings, so no canary-fix code changes
  were required in `Z.4`
- `Z4-OOS-001` was fixed on `feature/pZ-smoke-atm-graft @ 84935774`, so the
  normal smoke lane now accepts the single expected invalid-ack validation
  error without weakening the healthy-path log gate
- final release validation then passed on `feature/pZ-smoke-atm-graft @ daabc8c3`
  and the closeout candidate advanced to a truthful `READY` / `PASS` release
  verdict

## Acceptance Criteria

- every `Z.3` finding is either fixed or explicitly deferred with `team-lead`
  approval recorded in `docs/phase-Z/canary-findings-ledger.md`
- no `Z.3` finding may remain in an undecided state before the release
  verdict; every deferred `Z.3` finding must record explicit `team-lead`
  approval in `docs/phase-Z/canary-findings-ledger.md`, and any missing
  approval is blocking for this sprint
- the final validation set is rerun on the closeout branch
- all release-checklist rows that passed before the final fix round still pass
  after the `Z.4` fixes; any new failure is a blocking finding for this sprint
- `docs/phase-Z/readiness.md` records the release-readiness verdict
- `docs/phase-Z/readiness.md` `authorized_by` records an explicit named
  authority before the release verdict is considered final
- eligible `authorized_by` authorities are limited to `team-lead` by ATM
  identity or explicit user approval recorded by name, and the final
  authorization record must cite the approving ATM message id or one
  equivalent named approval artifact in `docs/phase-Z/readiness.md`
- `docs/phase-Z/release-checklist.md` and `docs/phase-Z/readiness.md`
  identify one authoritative closeout result for the `integrate/phase-Z`
  candidate

## Non-Closure

- `Z.4` does not reopen smoke or canary scope outside the frozen ledgers

## Production-Ready Expectation

Every listed `Z.4` deliverable is expected to land at a production-ready level
for the release-signoff scope this sprint claims: the release checklist and
readiness record must be sufficient for a ship/no-ship decision without
additional undocumented evidence.

## Required Validation

- `cargo build --release` or equivalent release build that refreshes the
  closeout executable baseline under review
- `docs/phase-Z/release-checklist.md`
- `docs/phase-Z/readiness.md`
- `cargo test --workspace`
- `git diff --check`
