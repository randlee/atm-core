# Sprint U.3 — Thread / Update / Supersede Hardening

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.3"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u3-thread-update-hardening
branch: feature/pU-u3-thread-update-hardening
status: planned
estimated_scope: M
```

## Goal

Make `add-details` and `supersede` explicit, distinct, and fully tested across
validation, selection, ack, and nudge behavior.

## Scope Summary

This sprint removes successor-chain shortcuts that hide mode semantics and
replaces them with explicit product behavior plus mode-specific regression
coverage.

## Governing Requirements

- `REQ-P-THREAD-001`
- `REQ-P-THREAD-002`
- `REQ-P-THREAD-003`
- `REQ-P-THREAD-004`
- `REQ-P-THREAD-005`
- `REQ-CORE-WORKFLOW-001`
- `REQ-P-SEND-001`
- `REQ-P-READ-001`
- `REQ-P-ACK-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-MailStore`
- `BOUNDARY-NotificationSink-Daemon`

## Prerequisites

- one-message-identity cleanup from `U.2` is complete

## Hard Dependencies

- `U.2`

## Non-Goals

- replacing the mutable-state table split
- broad roster/task redesign
- graft/daemon partitioning work

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Clarify mode semantics
   Development work:
   - state the exact behavioral difference between `add-details` and
     `supersede`
   - identify every read/list/ack/nudge path that must distinguish them
   Required tests:
   - mode-by-mode unit tests proving the intended differences
   Required doc or boundary updates:
   - update requirements/architecture docs and sprint notes

2. Remove generic-chain shortcuts
   Development work:
   - patch current code where successor-chain collapse ignores mode semantics
   - ensure “historical vs current” behavior follows the intended mode rules
   Required tests:
   - read/list selection tests for both modes
   Required doc or boundary updates:
   - update diagrams or docs that currently describe only generic terminal-node
     collapse

3. Ack and nudge reopen behavior
   Development work:
   - make ack reopen and successor-after-read behavior explicit for both modes
   Required tests:
   - ack reopen tests
   - nudge tests when successor arrives after predecessor was already read
   Required doc or boundary updates:
   - update thread/ack/nudge requirements and architecture wording

## Split Recommendation

Do not split unless one mode requires a new product decision. The default
deliverable is one explicit behavioral matrix for both modes.

## Acceptance Criteria

- mode-specific tests exist for both `add-details` and `supersede`
- send validation, list/read selection, ack reopen behavior, and nudge behavior
  are all covered
- the code no longer stores `thread_mode` as metadata whose real behavior is
  only implied

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/project-plan.md`
- `docs/plan-phase-U.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Risks And Watchouts

- do not accept “generic successor-chain coverage” as proof of per-mode
  correctness
- do not keep branchless read/list code if it erases approved mode semantics
- do not let nudge behavior stay implicit
