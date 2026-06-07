# AC.6 Cleanup, Deletion, And SQL Server Readiness

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.6
worktree: ../atm-core-worktrees/feature/pAC-s6-cleanup-deletion-and-sqlserver-readiness
branch: feature/pAC-s6-cleanup-deletion-and-sqlserver-readiness
status: planned
estimated_scope: medium
```

## Goal

Delete the obsolete storage/RPC scaffolding and prove the resulting contract is
small enough and clean enough for a future SQL Server backend.

## Scope Summary

This sprint is the closeout line for the contract reset. It removes obsolete
request/response wrappers, deletes backend-shaped leftovers, and records the
future SQL Server readiness claim explicitly.

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/readiness.md`
- `docs/phase-AC/issues.md`
- the new `atm-storage` and backend crates

## Prerequisites

- `AC.4`
- `AC.5`

## Out Of Scope

- no SQL Server implementation yet
- no new storage semantics beyond what earlier AC sprints defined

## Deliverables

- obsolete storage request/response wrappers are deleted
- obsolete RPC/storage/domain clone structs are deleted
- backend-specific seams that survived only because of the old architecture are deleted
- docs explicitly state that a future `atm-storage-sqlserver` backend should implement the same contract without requiring a new architectural reset

## Acceptance Criteria

- the shared storage contract remains small enough to audit directly
- no remaining obsolete wrapper families survive only for compatibility with the old design
- the repo documents SQL Server readiness as a consequence of the cleaned contract, not as a hypothetical wish

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `rg -n "MailStore.*Request|MailStore.*Response|TaskStore.*Request|TaskStore.*Response|RosterStore.*Request|RosterStore.*Response" crates docs -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC6.md`
- `docs/phase-AC/readiness.md`
- `docs/phase-AC/issues.md`
- `docs/project-plan.md`

## Risks And Watchouts

- if deletion is deferred, the old architecture will remain readable and therefore reusable by accident
- if SQL Server readiness is claimed without a truly backend-neutral contract, the phase will false-close
