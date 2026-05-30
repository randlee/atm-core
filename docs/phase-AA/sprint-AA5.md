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

## Non-Goals

- no new subsystem behavior
- no broad release-surface redesign beyond the corrected boundary

## Sub-Tasks

- Relock the TOML boundaries.
  Development work: remove `atm-daemon` from SQLite boundary allowlists,
  restore explicit forbidden edges, and close any visibility widened only to
  permit daemon-side SQLite assembly.
  Required tests: `just lint boundaries` or equivalent must fail if the edge
  returns.
  Required doc or boundary updates: boundary TOMLs and crate-local boundaries
  docs.

- Add a second architecture enforcement layer.
  Development work: add a repository-owned guard at
  `scripts/check-boundary-guard.py` plus a self-test at
  `scripts/test_boundary_guard.py`, modeled on the Ironclaw-style
  dependency-boundary check, so crate-edge regressions fail even if TOML
  policy is edited incorrectly. The script must inspect the workspace graph and
  fail if `atm-daemon -> atm-rusqlite` reappears or if forbidden edge
  removals / allowed dependent expansions are present in the changed boundary
  TOMLs without an explicit approval artifact.
  Required tests:
  - `python3 scripts/check-boundary-guard.py --base-ref <target>`
  - `python3 -m unittest scripts.test_boundary_guard`
  Required doc or boundary updates: testing docs, project plan, and Phase AA
  readiness record.

- Add governance for boundary-policy changes.
  Development work: make widening `allowed_dependents`, removing forbidden
  edges, or widening adapter visibility an explicit reviewed architecture
  change rather than ordinary lint data churn.
  Required tests: CI guard or repo-local lint for boundary-policy drift.
  Required doc or boundary updates: project plan and architecture docs.

- Add the `boundary-guard` QA agent.
  Development work: define and wire a dedicated QA/review agent named
  `boundary-guard` that examines
  planning PRs and phase-ending review packets for boundary loosening. The
  agent must treat any of the following TOML field changes as a relaxation
  requiring explicit architectural justification:

  | Field | Relaxation direction |
  |---|---|
  | `allowed_dependents` | any addition |
  | `forbidden_edges` | any removal |
  | `visibility` | any promotion toward public |
  | `constructor` | any promotion toward public |
  | `forbidden_test_bypasses` | any removal |
  | `forbidden` | any removal |

  The agent fires at two named points in the plan workflow: (1) after
  critical-plan-reviewer and before team-lead approval on any plan branch
  that modifies a boundary TOML, and (2) as a required reviewer in every
  phase-ending review packet.
  Required tests: a synthetic boundary-TOML relaxation fixture that the agent
  must catch, unconditionally, as a required self-test; prove the review
  workflow requires this agent at both named trigger points.
  Required doc or boundary updates: review workflow docs, project plan,
  readiness record, and any team/QA protocol docs that assign the new review
  responsibility.

## Split Recommendation

This must remain the final sprint. Relocking the boundary before the daemon is
actually clean will either fail immediately or produce more policy cheating.

## Acceptance Criteria

- `atm-daemon` is forbidden again as a dependent of SQLite assembly/store
  boundaries
- a second architecture-enforcement layer exists beyond the TOML lint
- boundary-policy widening is treated as an architecture change, not routine
  config churn
- `boundary-guard` exists, fires at the two named workflow
  trigger points (post-critical-plan-review and phase-ending review), and a
  finding at BLOCKING severity prevents the plan branch or phase PR from
  merging
- the daemon-to-SQLite edge fails both the TOML-based guard and the
  code-driven architecture guard if reintroduced

## Required Validation

- `just lint boundaries`
- `python3 scripts/check-boundary-guard.py --base-ref <target>`
- `python3 -m unittest scripts.test_boundary_guard`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/readiness.md`
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
