---
id: Y.4
title: Mutable Compatibility-Field Removal And Dependency Exposure
status: planned
branch: feature/pY-s4-mutable-compatibility-field-removal
worktree: ../atm-core-worktrees/feature/pY-s4-mutable-compatibility-field-removal
target: integrate/phase-Y
---

# Sprint Y.4 — Mutable Compatibility-Field Removal And Dependency Exposure

```yaml
plan_type: sprint_plan
phase: Y
sprint: Y.4
worktree: ../atm-core-worktrees/feature/pY-s4-mutable-compatibility-field-removal
branch: feature/pY-s4-mutable-compatibility-field-removal
status: planned
estimated_scope: large
```

## Goal

Reduce the compatibility inbox payload to the minimum justified ATM-authored
field set and let hidden consumers fail early so obsolete logic can be removed
before release smoke work begins.

## Scope Summary

- justify every surviving ATM-authored shared-inbox field
- remove mutable workflow-state fields from compatibility output
- expose and delete hidden consumers that still depend on those fields
- keep field-removal behavior explicit inside the event-family state machines

## Governing Requirements

- `docs/atm-message-schema.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- only immutable correlation/context fields may survive on the shared inbox
  surface
- mutable workflow truth belongs in SQLite, not compatibility JSONL

## Governing ADRs

- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`
- `docs/adr/ADR-012-one-message-identity.md`

## Governing Boundaries

- `docs/atm-core/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`

## Prerequisites

- `Y.3` complete
- surviving runtime writer owner is established

## Hard Dependencies

- `docs/atm-message-schema.md`
- `docs/phase-Y/delivery-state-machines.md`
- any field-by-field justification table produced during planning review

## Non-Goals

- do not start append-only cutover here
- do not preserve mutable fields just because one consumer still exists

## Sub-Tasks

### 1. Produce the field-justification ledger

Development work:
- document every ATM-authored compatibility field as:
  - keep
  - remove
  - undecided and blocked
- for each kept field, record why SQLite alone is insufficient

Required tests:
- schema/export tests cover every surviving field intentionally

Required doc or boundary updates:
- update `docs/atm-message-schema.md`
- update `docs/phase-Y/sprint-Y4.md`
- update `docs/phase-Y/delivery-state-machines.md` if field removal changes any
  transition contracts

### 2. Remove mutable workflow-state fields

Development work:
- remove mutable fields such as read/ack/workflow projections that are not
  justified as immutable compatibility context
- delete any now-obsolete projection/join logic that only existed to support
  those fields

Required tests:
- reads still resolve from SQLite truth
- compatibility export still loads in Claude-compatible flows

Required doc or boundary updates:
- update state-machine diagrams if any state transitions simplify

## Split Recommendation

Do not split field justification and field removal into separate sprints. The
whole point of `Y.4` is to expose hidden dependencies immediately.

## Acceptance Criteria

- every surviving ATM-authored compatibility field has written justification
- no mutable workflow-state field survives without explicit approval
- any hidden dependency exposed by field removal is either deleted or tracked as
  a blocking finding before `Y.5`

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/atm-message-schema.md`
- `docs/phase-Y/sprint-Y4.md`
- `docs/project-plan.md`

## Risks And Watchouts

- “temporarily keep it” is usually how obsolete logic survives release prep
- remove the data first; let the failures show you what still needs deletion
