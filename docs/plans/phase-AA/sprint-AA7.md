# AA.7 Rust Boundary Enforcement Crate

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.7
worktree: ../atm-core-worktrees/feature/pAA-s7-atm-architecture-crate
branch: feature/pAA-s7-atm-architecture-crate
status: complete
estimated_scope: medium
```

## Goal

Make the repository-owned Rust boundary crate the visible, required
second-layer architecture guard for the daemon-to-SQLite relock line.

## Scope Summary

This sprint is the code-driven enforcement closeout for the `AA.5` boundary
relock. It lands the Rust guard as a first-class workspace crate, removes the
superseded Python boundary scripts, and makes the required merge gate obvious
in normal Rust validation flow.

## Governing Sources

- `docs/plan-phase-AA.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/architecture.md`
- `docs/testing-guidelines.md`
- `.claude/agents/boundary-guard.md`

## Prerequisites

- `AA.5`

## Out Of Scope

- no new daemon/runtime behavior
- no additional SQLite-boundary redesign beyond the already accepted relock
- no reintroduction of Python as a code-driven architecture guard

## Deliverables

- `crates/atm-architecture/` exists as a test-only workspace crate and is the
  canonical code-driven boundary guard.

- The required forbidden-edge coverage is present for every currently accepted
  Phase AA boundary rule:

  ```rust
  // Representative guard surface. The exact implementation may differ,
  // but the crate must fail closed when any listed edge is relaxed.
  const FORBIDDEN_EDGES: &[(&str, &str)] = &[
      ("atm-daemon", "atm-rusqlite"),
      ("atm", "atm-rusqlite"),
      ("atm-runtime", "atm-daemon"),
      ("atm-graft", "atm-rusqlite"),
  ];
  ```

- The deleted Python scripts are not part of the active code-driven guard
  surface anymore.

- The docs and review workflow say the same thing:
  - `cargo test --package atm-architecture` is the required code-driven guard
  - `.claude/agents/boundary-guard.md` is the required review-layer guard

- A synthetic relaxation fixture exists and proves that removing a forbidden
  edge or weakening a boundary TOML causes the Rust guard to fail.

## Implementation Summary

- `crates/atm-architecture/` landed and now owns the workspace-visible
  forbidden-edge tests.
- The now-superseded Python boundary scripts were deleted.
- The sprint/readiness/project-plan docs were corrected so the Rust crate is
  the active enforcement layer.

## Split Recommendation

Keep this sprint limited to enforcement visibility and guard ownership. Do not
mix it with follow-on schema-contract, inbox-runtime, or SQLite-removal work.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA7.md` exists with the correct `branch`, `worktree`,
  and `status: complete`
- `crates/atm-architecture/` is present in the workspace and is named in the
  project-plan crate inventory
- `cargo test --package atm-architecture` is the named required merge gate
- the deleted Python boundary scripts are not described as the active
  code-driven enforcement layer anywhere in Phase AA docs
- a synthetic boundary-relaxation fixture proves the Rust guard fails closed

## Required Validation

- `cargo test --package atm-architecture`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/sprint-AA7.md`
- `docs/phase-AA/readiness.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/architecture.md`
- `docs/testing-guidelines.md`
- `.claude/agents/boundary-guard.md`

## Risks And Watchouts

- if the Rust guard becomes optional or hidden behind a non-default review
  path, policy widening can drift back into routine branch churn
- if sprint docs still describe deleted Python scripts as active, QA will read
  the wrong enforcement model from the source of truth
