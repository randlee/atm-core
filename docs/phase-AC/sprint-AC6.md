# AC.6 Cleanup, Deletion, And SQL Server Readiness

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.6
worktree: ../atm-core-worktrees/feature/pAC-s6-cleanup-deletion-and-sqlserver-readiness
branch: feature/pAC-s6-cleanup-deletion-and-sqlserver-readiness
status: planned
estimated_scope: large
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

## Ledger-Driven Deletion Sweep

`AC.6` is the explicit closure sprint against the `AC.0` type ledger.

Minimum deletion scope:

- all `MailStore*Request` / `MailStore*Response` wrappers still left after `AC.1`
- all `TaskStore*Request` / `TaskStore*Response` wrappers still left after `AC.1`
- all `RosterStore*Request` / `RosterStore*Response` wrappers still left after `AC.1`
- `MailStoreRequest` / `MailStoreResponse`
- `TaskStoreRequest` / `TaskStoreResponse`
- `RosterStoreRequest` / `RosterStoreResponse`
- any surviving `InboxIngress*` and `InboxExport*` wrapper families not explicitly retained as backend-internal implementation detail
- any surviving backend bundles or public helpers that the ledger marked `delete-bundle`

Minimum scope-reduction proof:

- Claude-only projections remain internal to `atm-storage-claude`
- SQLite-only observability helpers remain internal to `atm-storage-rusqlite`
- no deleted wrapper family survives in docs or code only because the old architecture used it

## Execution Checklist

Implementation order for `AC.6`:

1. Run the full ledger as a deletion checklist, not just grep-driven cleanup.
2. Remove any wrapper family still standing from:
   - code
   - docs
   - boundary TOMLs
   - tests
3. Reconfirm that backend-only Claude and SQLite types are still internal after the deletion pass.
4. Write the SQL Server readiness proof against the actual resulting contract, not the planned one.
5. Update the ledger with final closure notes so later phases do not resurrect deleted seams by accident.

Proof this sprint must leave behind:

- the old storage/RPC wrapper architecture is materially gone from the repo
- the remaining shared contract is small enough for direct manual audit
- SQL Server readiness is a demonstrated property of the simplified contract surface

## Acceptance Criteria

- the shared storage contract remains small enough to audit directly
- no remaining obsolete wrapper families survive only for compatibility with the old design
- the repo documents SQL Server readiness as a consequence of the cleaned contract, not as a hypothetical wish
- the deletion sweep is checked against `docs/phase-AC/type-ledger.md`, not only ad hoc grep patterns

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `rg -n "MailStore.*Request|MailStore.*Response|TaskStore.*Request|TaskStore.*Response|RosterStore.*Request|RosterStore.*Response" crates docs -S`
- `rg -n "InboxIngress.*Request|InboxIngress.*Response|InboxExport.*Request|InboxExport.*Response|SqliteBoundaryAssembly|RuntimeBundle" crates docs -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC6.md`
- `docs/phase-AC/readiness.md`
- `docs/phase-AC/issues.md`
- `docs/project-plan.md`
- `docs/phase-AC/type-ledger.md`

## Risks And Watchouts

- if deletion is deferred, the old architecture will remain readable and therefore reusable by accident
- if SQL Server readiness is claimed without a truly backend-neutral contract, the phase will false-close
- if docs keep stale wrapper names alive after code deletion, future work will quietly rebuild the old model
