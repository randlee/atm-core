# AC.4 `atm-core` Storage Boundary Adoption

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.4
worktree: ../atm-core-worktrees/feature/pAC-s4-atm-core-storage-boundary-adoption
branch: feature/pAC-s4-atm-core-storage-boundary-adoption
status: planned
estimated_scope: large
```

## Goal

Refactor `atm-core`, runtime, and daemon paths so they depend on the shared
storage traits instead of concrete backend logic.

## Scope Summary

This sprint removes the storage-specific logic that leaked upward because the
shared contract was missing. It moves orchestration back above the storage
trait line and keeps backend behavior below it.

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/sprint-AC1.md`
- `docs/phase-AC/sprint-AC2.md`
- `docs/phase-AC/sprint-AC3.md`
- `crates/atm-core/src/`
- `crates/atm-daemon/src/`
- `crates/atm-runtime/src/`

## Prerequisites

- `AC.2`
- `AC.3`

## Out Of Scope

- no SQL Server backend implementation yet
- no final DTO deletion pass yet

## Deliverables

- `atm-core` consumes `atm-storage` traits instead of concrete storage seams
- daemon/runtime/core no longer carry concrete SQLite or Claude storage logic above the approved composition seam
- composition roots assemble concrete backends and inject them through the shared traits
- any remaining backend-specific behaviors above the trait line are either deleted or moved to a backend crate

## Acceptance Criteria

- `rg -n "atm_rusqlite|atm_storage_claude" crates/atm-core crates/atm-daemon -S` is clean outside approved composition seams
- the runtime bundle/orchestration layer depends on semantic storage traits
- backend-specific branching in core orchestration is removed

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `rg -n "atm_rusqlite|atm_storage_claude" crates/atm-core crates/atm-daemon crates/atm-runtime -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC4.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- core/runtime/daemon architecture docs

## Risks And Watchouts

- if composition roots remain mixed with backend logic, the daemon leak problem will recur
- if this sprint keeps “temporary” direct backend seams, later deletion will become much harder
