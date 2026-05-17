---
id: Y.4
title: Delivery Coordinator And Event-Family State Machines
status: planned
branch: feature/pY-s4-delivery-coordinator-and-state-machines
worktree: ../atm-core-worktrees/feature/pY-s4-delivery-coordinator-and-state-machines
target: integrate/phase-Y
---

# Sprint Y.4 — Delivery Coordinator And Event-Family State Machines

```yaml
plan_type: sprint_plan
phase: Y
sprint: Y.4
worktree: ../atm-core-worktrees/feature/pY-s4-delivery-coordinator-and-state-machines
branch: feature/pY-s4-delivery-coordinator-and-state-machines
status: planned
estimated_scope: large
```

## Goal

Land the central delivery-policy coordinator and the required event-family
state machines so harness routing, write ownership, failure behavior, and
observability are encoded once and audited once rather than scattered across
command and daemon code.

## Scope Summary

- introduce one central delivery-policy coordinator that dispatches by event
  family and `RosterHarness`
- land separate event-family state machines instead of generic send branches
- encode the harness gate and the SQL-failure/original+error companion-message
  rule before append-only or field-removal work begins
- land explicit enums and transition tables for every required write-affecting
  event family before any implementation-specific simplification is considered
- make state transitions observable and QA-auditable

## Governing Requirements

- `docs/plan-phase-Y.md`
- `docs/phase-Y/delivery-state-machines.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- event-family routing must occur through one central coordinator rather than
  through scattered `if` branches in command code
- `NewMessageStateMachine` and `ThreadUpdateStateMachine` are separate machines
  with separate QA transition tables
- JSONL append is allowed only for `Claude Code` harnesses
- harness selection is based on harness type, not model
- non-Claude harnesses must never receive ATM-authored JSONL append output
- SQLite failure must still emit:
  - the original outward message
  - an additional `atm-system@<team>` error message
  - mirrored nudge behavior for both messages

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`

## Governing Boundaries

- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`

## Prerequisites

- `Y.3` complete
- the surviving runtime writer owner has been reduced to the approved boundary

## Hard Dependencies

- `docs/phase-Y/delivery-state-machines.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- `docs/phase-Y/state-diagrams.md`

## Non-Goals

- do not remove mutable compatibility fields here
- do not start append-only cutover here
- do not push policy back into generic writer helpers or command code

## Sub-Tasks

### 1. Land the central delivery-policy coordinator

Development work:
- introduce one central delivery-policy coordinator that:
  - dispatches by event family
  - branches by `RosterHarness`
  - emits observable transition events
- ensure downstream command or daemon callers invoke the coordinator rather
  than carrying harness-specific delivery rules themselves

Required tests:
- coordinator routing tests cover each supported event family
- coordinator routing tests cover both `Claude Code` and non-Claude harnesses
- observability confirms each routed transition explicitly

Required doc or boundary updates:
- update `docs/phase-Y/delivery-state-machines.md`
- update `docs/phase-Y/state-machine-coverage-audit.md`

### 2. Land the required event-family machines

Development work:
- land:
  - `NewMessageStateMachine`
  - `ThreadUpdateStateMachine`
  - `AckReplyStateMachine`
  - `InboxRepairStateMachine`
  - `RestoreInboxRebuildStateMachine`
- ensure the Claude/non-Claude split for new-message handling is encoded in the
  machine definitions, not in scattered call-site conditionals

Required tests:
- one transition test matrix for `ClaudeHarnessNewMessage`
- one transition test matrix for `NonClaudeHarnessNewMessage`
- one transition test matrix for `ThreadUpdateStateMachine`
- one transition test matrix for `AckReplyStateMachine`
- one transition test matrix for `InboxRepairStateMachine`
- one transition test matrix for `RestoreInboxRebuildStateMachine`
- observability confirms each state transition explicitly

Required doc or boundary updates:
- update `docs/phase-Y/delivery-state-machines.md`
- update `docs/phase-Y/state-diagrams.md`

### 3. Encode the exact failure and nudge contract

Development work:
- encode only the approved cases:
  - SQLite success -> original outward delivery path
  - SQLite failure on `Claude Code` harness -> original message output plus
    `atm-system@<team>` error message output
  - SQLite failure on non-Claude harness -> original message delivery plus
    `atm-system@<team>` error-message delivery through the non-Claude path
  - append/nudge failure -> post-send-hook fallback for notification
    degradation only
- do not add alternate fallback branches

Required tests:
- explicit acceptance tests for each approved branch
- no hidden alternate path exists

Required doc or boundary updates:
- update `docs/phase-Y/delivery-state-machines.md`
- update `docs/phase-Y/state-diagrams.md`

## Acceptance Criteria

- one central delivery-policy coordinator owns harness-specific routing
- each required event family has an explicit enum, transition table, and
  observable transition names
- the exact approved SQLite-failure/original+error rule is covered by tests
  and docs
- no command path retains local harness/delivery policy branches that should
  belong to the coordinator or state machines

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-Y/delivery-state-machines.md`
- `docs/phase-Y/state-diagrams.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not let the coordinator become a god object; it routes to machines, it
  does not absorb all event logic
- share side-effect executors where appropriate, but do not collapse separate
  event-legality rules into one generic machine
