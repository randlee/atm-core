---
id: AJ.7
title: Runtime Observation Contract Closeout
status: planned
branch: feature/pAJ-s7-runtime-observation-closeout
worktree: ../atm-core-worktrees/feature/pAJ-s7-runtime-observation-closeout
target: integrate/phase-AJ
---

# Sprint AJ.7 — Runtime Observation Contract Closeout

## Goal

Reconcile the governing runtime-observation contracts with AJ.1–AJ.6's merged
implementation and complete Phase AJ's documentation/status closure. AJ.7 adds
no production feature, transport, DTO, or persistence behavior.

## Hard Dependencies

- AJ.1 through AJ.6 merged forward into this branch
- `integrate/phase-AJ` contains AJ.6's merged public snapshot and source-use
  guard
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `docs/requirements.md` (`REQ-CORE-RUNTIME-002` / `-004`)
- `docs/adr/ADR-045-runtime-observation-attribution.md`

## Dependency Relation

- `must_follow` AJ.6 because this sprint documents the actual public snapshot,
  local-only cache update boundary, and source-use guard it delivered.
- No AJ sprint is `parallel_safe`: AJ.7 completes the serial phase line. Merge
  AJ.6 → AJ.7 before every dev/fix round; AJ.7's PR completes after AJ.6's PR.
  Development may start after AJ.6's development commit is pushed and does not
  wait for parent QA.

## Exact Targets

- `docs/requirements.md` (`REQ-CORE-RUNTIME-002` / `-004`)
- `docs/adr/ADR-045-runtime-observation-attribution.md`
- `docs/architecture.md` (runtime-health observation boundary)
- `docs/team-member-state.md`
- `docs/atm-daemon/boundaries.md`
- `boundaries/atm-daemon/daemon-status-source.toml`
- `docs/plans/phase-aj/plan-phase-aj.md` (exit criteria)
- `docs/plans/phase-aj/sprint-AJ1.md` through `sprint-AJ7.md` (frontmatter)
- `docs/project-plan.md` (AJ sprint status table)

## Interfaces To Add Or Modify

None. AJ.7 does not alter production Rust types, trait boundaries, database
schema, wire payloads, transport code, or source-use guard logic. It records
the tested AJ.1–AJ.6 result precisely.

## Deliverables

- Reconcile requirements, ADR-045, architecture, and team-member-state with
  the merged closed ingress set, no durable/mail telemetry, accepted-ingress
  order, non-overwrite behavior, lifecycle meanings, raw JSON/short human
  projection, and non-authoritative policy boundary.
- Replace the explicit pre-AJ planned labels in daemon boundary prose with the
  implemented contract only after AJ.6's required-positive source-use guard and
  end-to-end tests are green.
- Add `runtime_observation_non_authoritative` to
  `BOUNDARY-StatusSource-Daemon` only after the implemented guard exists. Do
  not broaden I/O ownership or dependencies.
- Mark AJ.1 through AJ.7 complete only after every required validation and
  parent merge gate has passed; check every Phase AJ exit criterion and update
  the project-plan AJ table in the same commit.

## Required Validation

- Review each Exact Target against the merged AJ.1–AJ.6 source and tests; no
  contract may claim an unimplemented behavior or omit an implemented public
  behavior.
- `just lint`
- `just test`
- The AJ.6 source-use guard passes and its required-positive checks remain
  present.
- `git diff --check`

## Acceptance Criteria

- Governing documents agree with the implemented closed ingress set and never
  reintroduce durable pid ownership, live-pid rejection, an admin-assume-
  identity path, or observation-driven policy.
- The machine-readable and human daemon boundary records accurately describe
  the implemented AJ behavior and named review gate.
- All seven AJ sprint docs, the phase exit checklist, and the project-plan AJ
  table report complete only after AJ.7's validation and dependency gate pass.
- AJ.7 must_follow AJ.6 under the merge-forward and PR-completion rule in the
  phase plan.
