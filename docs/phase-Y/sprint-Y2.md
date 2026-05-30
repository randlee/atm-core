---
id: Y.2
title: Pre-Smoke Easy Fixes And Validation
status: complete
branch: feature/pY-s2-pre-smoke-easy-fixes
worktree: ../atm-core-worktrees/feature/pY-s2-pre-smoke-easy-fixes
target: integrate/phase-Y
---

# Sprint Y.2 — Pre-Smoke Easy Fixes And Validation

## Goal

- land the second approved small implementation slice before heavy smoke work
- close only narrow, low-risk fixes that are explicitly identified during
  planning and `Y.1`
- validate that the release line is ready for the larger write-boundary work
  that begins in `Y.3`

## Hard Dependencies

- `docs/plan-phase-Y.md`
- `docs/phase-Y/sprint-Y1.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- approved `Y.1` closeout notes

## Exact Targets

- `crates/atm/src/commands/help.rs`
- `docs/phase-Y/help.md`
- `docs/atm/commands/help.md`
- `docs/phase-Y/sprint-Y2.md`
- `docs/project-plan.md`

## Required Work

- complete the explicitly deferred `Y.1` tier-2 help follow-ups only:
  - add worked `hooks` authoring/troubleshooting examples
  - add operator-facing `identity` precedence and override examples
  - add `skills` workflow examples while keeping harness-vs-model boundaries explicit
- keep scope tight enough that `Y.3` still owns the first serious
  compatibility-write boundary refactor
- validate that `Y.1` and `Y.2` together do not change the working inbox wire
  contract accidentally
- record any larger architectural leftovers for `Y.3+` instead of absorbing
  them here
- do not introduce JSON input work, compatibility writer changes, or boundary
  refactors in this sprint

## Acceptance Criteria

- only the approved tier-2 help/UX easy fixes land
- no accidental compatibility-inbox format change is introduced
- larger boundary work remains queued for `Y.3` rather than partially started
  here
- the three tier-2 topics no longer contain `Y.2 will ...` placeholder language
- the docs clearly record that `Y.2` completed the deferred `Y.1` help follow-ups

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `git diff --check`
