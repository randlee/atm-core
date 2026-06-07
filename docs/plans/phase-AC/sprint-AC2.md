# AC.2 `atm-storage-claude` Extraction

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.2
worktree: ../atm-core-worktrees/feature/pAC-s2-atm-storage-claude-extraction
branch: feature/pAC-s2-atm-storage-claude-extraction
status: planned
estimated_scope: large
```

## Goal

Extract the Claude inbox storage implementation into `crates/atm-storage-claude`
behind the shared `atm-storage` contract.

## Scope Summary

This sprint moves Claude inbox file-backed storage behavior out of `atm-core`
and behind the new traits. JSON salvage, source discovery, file locking, and
atomic rewrite remain internal implementation details of the Claude backend.

Production-ready commitment:
- every deliverable listed in this sprint is expected to land at a
  production-ready level for the Claude-backend extraction scope this sprint
  claims; a docs-only backend split or partial trait claim is not accepted

Primary closure rule:
- `AC.2` is the primary closure sprint for Claude-backend-only projection,
  import/export, repair, and writer seams
- later sprints may cut consumers over or verify no public leakage remains,
  but they do not own the Claude internalization decision

## Governing Sources

- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/plans/phase-AC/sprint-AC1.md`
- `crates/atm-core/src/mailbox/`
- `crates/atm-core/src/read/`
- `crates/atm-core/src/list/`
- `crates/atm-core/src/clear/`

## Prerequisites

- `AC.1`

## Out Of Scope

- no SQLite backend convergence yet
- no daemon/runtime composition refactor yet
- no new legal schema expansion beyond existing accepted Claude contract work

## AC.2 / AC.3 Parallel Ownership Protocol

`AC.2` and `AC.3` may execute in parallel only while they respect this
non-overlapping ownership split inside `atm-core`:

- `AC.2` may touch:
  - `crates/atm-core/src/mailbox/`
  - `crates/atm-core/src/read/`
  - `crates/atm-core/src/list/`
  - `crates/atm-core/src/clear/`
  - `crates/atm-core/src/delivery_execution.rs`
- `AC.3` may touch:
  - `crates/atm-core/src/boundary/`
  - `crates/atm-runtime/`
  - `crates/atm-rusqlite/` or `crates/atm-storage-rusqlite/`

Merge-forward rule:
- if either sprint needs to touch a file outside its declared surface, `AC.2`
  must merge first and `AC.3` must rebase / merge-forward from the updated
  branch before continuing
- no sprint may silently co-own an `atm-core` file with the other sprint

## Deliverables

- `crates/atm-storage-claude` exists as a concrete backend crate
- the Claude backend implements the shared storage traits it can satisfy
- the Claude backend task-store policy is explicit:
  - `ClaudeStorageBackend` implements `MessageStore` and `RosterStore`
  - `ClaudeStorageBackend` does **not** implement `TaskStore`
  - any needed task-store placeholder must be a clearly named external
    composition adapter, not an implicit partial-CRUD claim by the Claude
    backend itself
- backend-specific behavior remains internal:
  - JSON parsing and fail-soft salvage
  - file locking
  - source discovery
  - projection rewrite

- Claude storage consumes the shared canonical structs. It does not define a parallel domain model just for file-backed storage.

- Unsupported ATM-only fields may be ignored or degraded by implementation policy, but the shared contract types remain the same.

- The backend-facing implementation shape is explicit and small:

  ```rust
  pub struct ClaudeStorageBackend {
      // private backend fields only
  }

  impl MessageStore for ClaudeStorageBackend { /* ... */ }
  impl RosterStore for ClaudeStorageBackend { /* ... */ }
  ```

- `TaskStore` is intentionally not implemented by `ClaudeStorageBackend`.
  The backend may participate in compositions that supply a null/degraded task
  adapter elsewhere, but the Claude backend must not silently claim task
  persistence support it does not actually provide.

- Claude-specific roster and inbox projection helpers do not become shared
  public API. If they survive, they survive as private or backend-local types
  inside `atm-storage-claude`.

## Ledger-Driven Type Work

`AC.2` owns every type the `AC.0` ledger marked as Claude-backend-only or as a
Claude seam that must move below the trait line.

These must become internal `atm-storage-claude` concerns rather than shared
contract types:

- `ClaudeCodeRosterMember`
- `ClaudeCodeTeamRoster`
- `InboxSourceFileRecord`
- `ClaudeCompatibilityDeliveryMode`
- the `InboxIngress*` wrapper family
- the `InboxExport*` wrapper family
- the `delivery_execution::ClaudeInboxWriter` seam

The canonical shared contract must remain above these projections:

- `RosterMember`
- `RosterSnapshot`
- `Message`
- `MessageQuery`

Required scope-reduction rule:

- if a Claude type exists only to project canonical data into `.json` inbox
  layout, that type must be backend-internal or deleted, not promoted into
  `atm-storage`

## Execution Checklist

Implementation order for `AC.2`:

1. Create `crates/atm-storage-claude` and make it depend on `atm-storage`, not `atm-core`.
2. Move Claude inbox read/write/repair ownership out of `atm-core` module seams.
3. Rework roster projection so:
   - canonical `RosterMember` / `RosterSnapshot` stay shared
   - `ClaudeCodeRosterMember` / `ClaudeCodeTeamRoster` become backend-internal translation helpers or disappear entirely
4. Rework inbox import/export ownership so:
   - `InboxIngress*` and `InboxExport*` wrappers no longer define the shared contract
   - Claude-specific compatibility machinery lives behind backend-internal functions or private helper structs
5. Delete or internalize `delivery_execution::ClaudeInboxWriter`.
6. Update boundary TOMLs to make `atm-storage-claude -> atm-storage` explicit and `atm-storage-claude -X-> atm-core` explicit.

Proof this sprint must leave behind:

- the Claude backend owns its own projection and repair logic
- `atm-core` no longer exposes Claude-specific public storage types as if they were shared domain types
- the shared contract remains clean even if the backend still has rich internal helpers
- any `InboxIngress*` / `InboxExport*` / `ClaudeInboxWriter` cleanup left for
  `AC.6` is verification-only, not deferred ownership

## Acceptance Criteria

- `atm-core` no longer owns Claude inbox storage internals that belong in the backend crate
- Claude storage implements the shared contract without widening it to path/file-specific APIs
- Claude task storage policy is explicit and reviewable:
  `ClaudeStorageBackend` does not implement `TaskStore`, and that omission is
  documented as accepted degraded capability rather than left ambiguous
- malformed-ingress and file-lock behavior remain below the trait line
- `ClaudeCodeRosterMember` and `ClaudeCodeTeamRoster` do not survive as shared public contract types
- the `InboxIngress*` and `InboxExport*` wrapper families are not promoted into `atm-storage`

## Required Validation

- `cargo test -p atm-storage-claude`
- `cargo clippy -p atm-storage-claude -- -D warnings`
- `cargo tree -p atm-storage-claude`
- `cargo test -p atm-core`
- `git diff --check`
- `rg -n "ClaudeCodeRosterMember|ClaudeCodeTeamRoster|InboxIngress|InboxExport|ClaudeInboxWriter" crates/atm-storage crates/atm-storage-claude crates/atm-core -S`
- verify `atm-core` is not present in the transitive dependency tree for `atm-storage-claude`

## Required Document Updates

- `docs/plans/phase-AC/sprint-AC2.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/project-plan.md`
- storage architecture docs that currently treat Claude inbox logic as `atm-core` internals
- create `boundaries/atm-storage-claude/` TOML records covering the Claude backend implementation of the shared contracts
- annotate the Claude backend boundary notes/TOMLs so the `TaskStore`
  omission is explicit and reviewable rather than implied by absence
- each `boundaries/atm-storage-claude/` TOML record must include `allowed_dependents = ["atm-runtime", "atm-daemon"]` via composition only; `atm-core` must NOT appear in `allowed_dependents`
- that `allowed_dependents` direction is intentional here because the boundary
  records live under `atm-storage-claude` and describe which crates may depend
  on the backend crate
- document the forbidden edge `atm-storage-claude -> atm-core` in those boundary records and the owning boundary notes

## Risks And Watchouts

- if file paths or lock semantics leak into the shared trait surface, the backend extraction has widened the contract incorrectly
- if Claude storage introduces a backend-specific `Message` variant, interchangeability is already lost
- if Claude-specific public types remain visible outside the backend crate only because tests or compatibility code still use them, the extraction is incomplete
