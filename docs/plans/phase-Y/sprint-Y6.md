---
id: Y.6
title: Append-Only Compatibility Export Cutover
status: complete
branch: feature/pY-s6-append-only-compatibility-export
worktree: ../atm-core-worktrees/feature/pY-s6-append-only-compatibility-export
target: integrate/phase-Y
---

# Sprint Y.6 — Append-Only Compatibility Export Cutover

## Goal

If the approved wire contract allows it, replace array-style compatibility
rewrites with append-only Claude-Code-compatible output and eliminate normal
runtime lock dependence on inbox-file rewrites.

## Current Status

- complete on `feature/pY-s6-append-only-compatibility-export`
- normal retained Claude Code compatibility writes now append one JSONL record
  at a time after the durable SQLite/workflow step succeeds
- non-Claude harnesses still never receive ATM-authored JSONL append output
- JSON-array inbox files are migrated through explicit rebuild/re-export the
  first time the retained runtime encounters them
- SQLite failure now follows the approved degraded outward-delivery contract:
  - Claude Code harnesses append the original message plus an
    `atm-system@<team>` companion error message
  - non-Claude harnesses skip JSONL append but still emit the mirrored
    companion nudge plan
- append degradation after successful SQLite persistence stays on the
  post-send-hook fallback path

## Scope Summary

- replace full-file runtime rewrites with append-only output where permitted
- keep repair/rebuild flows separate
- preserve the exact success/failure contract agreed during planning
- keep append/no-append behavior inside harness-specific state-machine
  transitions

## Governing Requirements

- `docs/plans/phase-Y/inbox-write-path-audit.md`
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
- `Y.5` complete
- compatibility field set approved

## Hard Dependencies

- final accepted wire-format decision
- final accepted SQLite-failure/original+error emission contract
- `docs/plans/phase-Y/delivery-state-machines.md`

## Non-Goals

- do not relax the exact failure contract during implementation
- do not broaden JSONL append to non-Claude harnesses
- do not push append/no-append decisions back into generic writer helpers

## Sub-Tasks

### 1. Cut runtime writer over to append-only if approved

Development work:
- replace runtime array rewrite usage with append-only output
- keep explicit rebuild/repair flows staged and separate
- delete leftover lock-coupled runtime rewrite code
- keep the append choice owned by the harness-specific `NewMessage` machines

Required tests:
- append-only success on Claude Code harness
- no JSONL append on non-Claude harness
- long-run repeated writes do not require mailbox rewrite locks

Required doc or boundary updates:
- update `docs/plans/phase-Y/inbox-write-path-audit.md`
- update `docs/atm-message-schema.md` if wire-format wording changes
- update `docs/plans/phase-Y/delivery-state-machines.md` if the append contract
  changes any machine transitions

### 2. Encode the exact failure truth table

Development work:
- implement only the approved cases:
  - SQLite success -> original message output
  - SQLite failure on `Claude Code` harness -> original message append +
    `atm-system@<team>` error-message append
  - SQLite failure on non-Claude harness -> original message delivery +
    `atm-system@<team>` error-message delivery through the non-Claude path
  - append failure -> post-send-hook fallback for notification degradation
- do not add alternate fallback branches

Required tests:
- explicit acceptance tests for each approved branch
- no hidden alternate path exists

Required doc or boundary updates:
- update `docs/plans/phase-Y/sprint-Y6.md`

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

- `docs/plans/phase-Y/inbox-write-path-audit.md`
- `docs/plans/phase-Y/sprint-Y6.md`
- `docs/project-plan.md`

## Risks And Watchouts

- append-only is not justified if the wire contract still requires array
  replacement
- the SQLite-failure/original+error rule must remain exact; do not “simplify”
  it during implementation
