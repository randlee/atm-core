# AA.5 Boundary Relock And Permanent Enforcement

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.5
worktree: ../atm-core-worktrees/feature/pAA-s5-boundary-relock-and-permanent-enforcement
branch: feature/pAA-s5-boundary-relock-and-permanent-enforcement
status: planned
estimated_scope: medium
```

## Goal

Re-establish the daemon-to-SQLite boundary and add a second enforcement layer
so widening the boundary cannot hide behind TOML-only changes again. This
includes the dedicated `boundary-guard` QA agent for plan review and
phase-ending review.

## Scope Summary

This sprint is the final closure gate. It restores the forbidden edge and adds
permanent protection beyond the existing boundary TOMLs and lint rules, both in
code and in review workflow.

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-RUSQLITE-STORE-001`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`

## Governing Boundaries

- `boundaries/atm-runtime/runtime-composition.toml`
- `boundaries/atm-rusqlite/sqlite-boundary-assembly.toml`
- `boundaries/atm-rusqlite/mail-store-sqlite.toml`
- `boundaries/atm-rusqlite/roster-store-sqlite.toml`
- `boundaries/atm-rusqlite/task-store-sqlite.toml`

## Prerequisites

- `AA.2`
- `AA.3`
- `AA.4`

## Hard Dependencies

- daemon production code is already free of direct SQLite coupling

## Out Of Scope

- no new subsystem behavior
- no broad release-surface redesign beyond the corrected boundary

## Deliverables

- The SQLite boundary TOMLs are relocked. The minimum closure set is:
  - remove `atm-daemon` from SQLite boundary allowlists
  - restore explicit forbidden edges
  - restore any visibility/constructor privacy that was widened only to permit
    daemon-side SQLite assembly
  - make the SQLite boundary TOMLs agree with
    `boundaries/atm-runtime/runtime-composition.toml` on the forbidden
    `atm-daemon -> atm-rusqlite` edge so no policy contradiction remains after
    relock

- A second repository-owned architecture guard exists. The minimum executable
  surface is frozen now:
  - `scripts/check-boundary-guard.py`
  - `scripts/test_boundary_guard.py`
  - `.claude/agents/boundary-guard.md`

- The second guard enforces both code-edge and policy-edge checks. The minimum
  machine-checked contract is frozen now:

  ```json
  {
    "status": "PASS | FAIL",
    "forbidden_edges": ["atm-daemon -> atm-rusqlite"],
    "policy_relaxations": [
      {
        "field": "allowed_dependents",
        "change": "added atm-daemon",
        "requires_approval": true
      }
    ]
  }
  ```

- Boundary-policy widening is explicitly documented as an architecture change,
  not routine lint-data churn.
- The temporary `AA.2` through `AA.4` transition rule is closed, meaning there
  is no longer a split between target-state boundary policy in
  `runtime-composition.toml` and authoritative lint policy in the SQLite
  boundary TOMLs.

- The `boundary-guard` QA agent is defined as a required review participant.
  The exact relaxation table is frozen:

  | Field | Relaxation direction |
  |---|---|
  | `allowed_dependents` | any addition |
  | `forbidden_edges` | any removal |
  | `visibility` | any promotion toward public |
  | `constructor` | any promotion toward public |
  | `forbidden_test_bypasses` | any removal |
  | `forbidden` | any removal |

- The `boundary-guard` workflow trigger points are frozen:
  1. after `critical-plan-reviewer` and before `team-lead` approval on any
     plan branch that modifies a boundary TOML
  2. as a required reviewer in every phase-ending review packet

- The `boundary-guard` blocking posture is frozen:
  - any `Blocking` severity finding from `.claude/agents/boundary-guard.md`
    stops merge on plan branches that modify boundary TOMLs
  - any `Blocking` severity finding from `.claude/agents/boundary-guard.md`
    stops merge on phase-ending review branches
  - `Minor` findings remain advisory only

- A synthetic boundary-relaxation fixture exists and is required self-test
  coverage for the second guard.

## Split Recommendation

This must remain the final sprint. Relocking the boundary before the daemon is
actually clean will either fail immediately or produce more policy cheating.

## Acceptance Criteria

- `atm-daemon` is forbidden again as a dependent of SQLite assembly/store
  boundaries
- `boundaries/atm-runtime/runtime-composition.toml` and the SQLite boundary
  TOMLs agree on the same forbidden daemon-to-SQLite edge after relock
- a second architecture-enforcement layer exists beyond the TOML lint
- boundary-policy widening is treated as an architecture change, not routine
  config churn
- `.claude/agents/boundary-guard.md` exists, `boundary-guard` fires at the two
  named workflow trigger points, and `Blocking` findings are required to stop
  merge on both plan branches touching boundary TOMLs and phase-ending review
  branches
- the sprint docs include the exact relaxation table and a machine-checkable
  output shape for the second guard
- the daemon-to-SQLite edge fails both the TOML-based guard and the
  code-driven architecture guard if reintroduced

## Required Validation

- `just lint boundaries`
- `python3 scripts/check-boundary-guard.py --base-ref origin/integrate/phase-AA`
- `python3 -m unittest scripts.test_boundary_guard`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/project-plan.md`
- `docs/plan-phase-AA.md`
- `docs/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`
- CI / testing documentation for the second enforcement layer
- review workflow / QA protocol documentation for the new boundary-enforcement
  agent

## Risks And Watchouts

- if this sprint ships without the second enforcement layer, the repo is still
  trusting boundary TOML as the final authority
- if the QA agent is advisory-only instead of required, boundary-policy drift
  can still be normalized during planning or phase closeout
