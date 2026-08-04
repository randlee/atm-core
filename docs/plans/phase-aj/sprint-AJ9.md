---
id: AJ.9
title: Runtime Observation Governing Contract Reconciliation
status: complete
branch: feature/pAJ-s9-runtime-observation-contract-reconciliation
worktree: ../atm-core-worktrees/feature/pAJ-s9-runtime-observation-contract-reconciliation
target: integrate/phase-aj
---

# Sprint AJ.9 — Runtime Observation Governing Contract Reconciliation

## Goal

Reconcile the governing requirements, ADR, architecture, and team-member-state
contract against the merged AJ.1–AJ.8 implementation. AJ.9 changes only those
documents; AJ.10 owns phase status and closure.

After AJ.6, `phase-aj-research.md` remains planning context rather than a hard
dependency: AJ.9 reconciles merged implementation and contracts.

## Hard Dependencies

- AJ.1 through AJ.8 development heads merged forward into this branch
- AJ.8's final boundary record is present through the immediate AJ.8 → AJ.9
  merge-forward; AJ.8 QA/PR completion is not a dev-start gate
- `docs/plans/phase-aj/plan-phase-aj.md`

## Dependency Relation

- `must_follow` AJ.8 because governing contracts must agree with the final
  named boundary gate and implemented behavior.
- AJ.10 `must_follow`s AJ.9 because phase status may change only after these
  governing documents agree with implementation.
- AJ.9 begins immediately after AJ.8 → AJ.9 merge-forward; it does not wait
  for AJ.8 QA. Repeat that merge before every AJ.9 dev/fix round. AJ.9's PR
  completes only after AJ.8's PR merges. No AJ pair is `parallel_safe`.
- On AJ.9 development-head push, AJ.10 begins immediately by merging
  AJ.9 → AJ.10; AJ.10 must complete that merge before any dev/fix round and
  does not wait for AJ.9 QA.

## Exact Targets

- `docs/requirements.md` (`REQ-CORE-RUNTIME-002` / `-004`)
- `docs/adr/ADR-045-runtime-observation-attribution.md`
- `docs/architecture.md` (runtime-health observation boundary)
- `docs/team-member-state.md`

## Interfaces To Add Or Modify

None. AJ.9 adds no production code, DTO, persistence, transport, or static
test behavior.

## Deliverables

- The four documents agree with actual code on the closed ingress set,
  accepted-ingress order, no-default-overwrite behavior, lifecycle meanings,
  retained audit evidence, raw JSON/short human projection, and no durable/mail
  telemetry.
- They prohibit observation-driven routing, nudge, notification, retry,
  admission, delivery, and policy logic, without reintroducing live-pid
  rejection, durable PID ownership, or an admin-assume-identity path.

## Required Validation

- Perform a clause-by-clause diff of every governing-document claim against
  merged AJ.1–AJ.8 source and the named tests that prove it. Record a source
  symbol and test name for each clause; any clause without both remains marked
  as a planned target rather than being reviewed through as prose.
- `just lint`
- `git diff --check`

## Acceptance Criteria

- All four governing documents agree with the final AJ boundary record and
  implementation without ambiguous planned/current wording.
- AJ.9 is production-ready only for governing contract reconciliation; it does
  not flip phase/sprint status or claim phase closure.
- AJ.9 must_follow AJ.8 under the merge-forward and PR-completion rule above.
