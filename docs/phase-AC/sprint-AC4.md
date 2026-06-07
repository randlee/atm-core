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

Primary closure rule:
- `AC.4` is the primary closure sprint for core/runtime/daemon consumer cutover
  and for any seam that is intentionally retained outside `atm-storage`
- this sprint does not redefine shared storage types or backend-internal
  policies already closed by `AC.1` through `AC.3`

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

- The adoption seam is explicit:

  ```rust
  pub struct StorageBackends<M, R, T> {
      pub messages: M,
      pub rosters: R,
      pub tasks: T,
  }

  // core/runtime/daemon consume shared traits here, not concrete backend types
  ```

## Ledger-Driven Deletion Targets

`AC.4` is where the repo stops depending on the old core-owned backend seams.
The main ledger-owned deletions or migrations in this sprint are:

- `delivery_execution::ClaudeInboxWriter`
- `RuntimeBundle`
- any residual direct ownership of:
  - inbox import/export orchestration that belongs in `atm-storage-claude`
  - replay/finalizer wiring that belongs in backend capability seams
  - concrete SQLite composition helpers

Transport and workflow surfaces that remain outside `atm-storage` should still
be left standing if they are not backend seams:

- `AtmProtocol`
- `ClientTransport`
- `ServerTransport`
- `RequestDispatcher`
- `AdvisoryStreamSink`
- `StatusSource`
- `WatchEventSource`
- `ReconcileCoordinator`

## Execution Checklist

Implementation order for `AC.4`:

1. Replace core-owned storage trait imports with `atm-storage` traits.
2. Move backend assembly to approved composition seams only.
3. Delete direct Claude storage seams still owned by `atm-core`.
4. Delete direct SQLite storage seams still owned by `atm-core`, `atm-runtime`, or `atm-daemon`.
5. Re-run the forbidden-edge and grep checks before touching docs so the code graph is proven clean first.

Proof this sprint must leave behind:

- `atm-core` orchestrates semantic storage behavior only
- backend crates own backend mechanics
- daemon/runtime no longer need storage-specific knowledge above composition
- rows marked `retain-outside-storage` are classified here so `AC.6` only
  verifies drift did not reappear

## Acceptance Criteria

- a generic `StorageBackends<M, R, T>` seam exists at the approved
  daemon/runtime assembly root and is the sole location where concrete backend
  storage types are named
- `rg -n "atm_rusqlite|atm_storage_claude" crates/atm-core crates/atm-daemon -S` is clean outside approved composition seams
- the runtime bundle/orchestration layer depends on semantic storage traits
- backend-specific branching in core orchestration is removed

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `rg -n "StorageBackends<|struct StorageBackends" crates/atm-core crates/atm-runtime crates/atm-daemon -S`
- `rg -n "atm_rusqlite|atm_storage_claude" crates/atm-core crates/atm-daemon crates/atm-runtime -S`
- `rg -n "ClaudeInboxWriter|RuntimeBundle|InboxIngress|InboxExport" crates/atm-core crates/atm-runtime crates/atm-daemon -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC4.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- core/runtime/daemon architecture docs

## Risks And Watchouts

- if composition roots remain mixed with backend logic, the daemon leak problem will recur
- if this sprint keeps “temporary” direct backend seams, later deletion will become much harder
- if old boundary types remain imported in core just for convenience, the storage reset has not actually crossed the crate line
