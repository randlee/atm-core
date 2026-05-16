---
id: Y.5
title: Append-Only Compatibility Export Cutover
status: planned
branch: feature/pY-s5-append-only-compatibility-export
worktree: ../atm-core-worktrees/feature/pY-s5-append-only-compatibility-export
target: integrate/phase-Y
---

# Sprint Y.5 — Append-Only Compatibility Export Cutover

```yaml
plan_type: sprint_plan
phase: Y
sprint: Y.5
worktree: ../atm-core-worktrees/feature/pY-s5-append-only-compatibility-export
branch: feature/pY-s5-append-only-compatibility-export
status: planned
estimated_scope: large
```

## Goal

If the approved wire contract allows it, replace array-style compatibility
rewrites with append-only Claude-Code-compatible output and eliminate normal
runtime lock dependence on inbox-file rewrites.

## Scope Summary

- replace full-file runtime rewrites with append-only output where permitted
- keep repair/rebuild flows separate
- preserve the exact success/failure contract agreed during planning

## Governing Requirements

- `docs/phase-Y/inbox-write-path-audit.md`
- only the Claude Code harness may receive JSONL append output
- non-Claude harnesses must never receive JSONL append output
- SQLite and outward delivery error behavior must match the planning contract
  exactly

## Governing ADRs

- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`

## Governing Boundaries

- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Prerequisites

- `Y.3` complete
- `Y.4` complete
- compatibility field set approved

## Hard Dependencies

- final accepted wire-format decision
- final accepted SQLite-failure/original+error emission contract

## Non-Goals

- do not relax the exact failure contract during implementation
- do not broaden JSONL append to non-Claude harnesses

## Sub-Tasks

### 1. Cut runtime writer over to append-only if approved

Development work:
- replace runtime array rewrite usage with append-only output
- keep explicit rebuild/repair flows staged and separate
- delete leftover lock-coupled runtime rewrite code

Required tests:
- append-only success on Claude Code harness
- no JSONL append on non-Claude harness
- long-run repeated writes do not require mailbox rewrite locks

Required doc or boundary updates:
- update `docs/phase-Y/inbox-write-path-audit.md`
- update `docs/atm-message-schema.md` if wire-format wording changes

### 2. Encode the exact failure truth table

Development work:
- implement only the approved cases:
  - SQLite success -> original message output
  - SQLite failure -> original message + `atm-system@<team>` error message
  - append failure -> post-send-hook fallback for notification degradation
- do not add alternate fallback branches

Required tests:
- explicit acceptance tests for each approved branch
- no hidden alternate path exists

Required doc or boundary updates:
- update `docs/phase-Y/sprint-Y5.md`

## Split Recommendation

Keep the append cutover and the exact failure truth-table implementation in the
same sprint. Splitting them would make it too easy for the repo to carry a
partial, ambiguous notification contract.

## Acceptance Criteria

- normal runtime compatibility output is append-only if the approved wire
  contract allows it
- non-Claude harnesses still never receive JSONL append output
- the exact approved failure contract is covered by tests and docs
- no hidden lock-coupled runtime rewrite path survives

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/sprint-Y5.md`
- `docs/project-plan.md`

## Risks And Watchouts

- append-only is not justified if the wire contract still requires array
  replacement
- the SQLite-failure/original+error rule must remain exact; do not “simplify”
  it during implementation
