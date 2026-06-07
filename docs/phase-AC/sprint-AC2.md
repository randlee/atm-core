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

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/sprint-AC1.md`
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

## Deliverables

- `crates/atm-storage-claude` exists as a concrete backend crate
- the Claude backend implements the shared storage traits it can satisfy
- backend-specific behavior remains internal:
  - JSON parsing and fail-soft salvage
  - file locking
  - source discovery
  - projection rewrite

- Claude storage consumes the shared canonical structs. It does not define a parallel domain model just for file-backed storage.

- Unsupported ATM-only fields may be ignored or degraded by implementation policy, but the shared contract types remain the same.

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

## Acceptance Criteria

- `atm-core` no longer owns Claude inbox storage internals that belong in the backend crate
- Claude storage implements the shared contract without widening it to path/file-specific APIs
- malformed-ingress and file-lock behavior remain below the trait line
- `ClaudeCodeRosterMember` and `ClaudeCodeTeamRoster` do not survive as shared public contract types
- the `InboxIngress*` and `InboxExport*` wrapper families are not promoted into `atm-storage`

## Required Validation

- `cargo test -p atm-storage-claude`
- `cargo clippy -p atm-storage-claude -- -D warnings`
- `cargo tree -p atm-storage-claude`
- `cargo test -p agent-team-mail-core`
- `git diff --check`
- `rg -n "ClaudeCodeRosterMember|ClaudeCodeTeamRoster|InboxIngress|InboxExport|ClaudeInboxWriter" crates/atm-storage crates/atm-storage-claude crates/atm-core -S`
- verify `atm-core` is not present in the transitive dependency tree for `atm-storage-claude`

## Required Document Updates

- `docs/phase-AC/sprint-AC2.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- storage architecture docs that currently treat Claude inbox logic as `atm-core` internals
- create `boundaries/atm-storage-claude/` TOML records covering the Claude backend implementation of the shared contracts
- each `boundaries/atm-storage-claude/` TOML record must include `allowed_dependents = ["atm-runtime", "atm-daemon"]` via composition only; `atm-core` must NOT appear in `allowed_dependents`
- document the forbidden edge `atm-storage-claude -> atm-core` in those boundary records and the owning boundary notes

## Risks And Watchouts

- if file paths or lock semantics leak into the shared trait surface, the backend extraction has widened the contract incorrectly
- if Claude storage introduces a backend-specific `Message` variant, interchangeability is already lost
