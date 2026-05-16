---
id: Y.3
title: Hard Write-Boundary Consolidation
status: planned
branch: feature/pY-s3-hard-write-boundary-consolidation
worktree: ../atm-core-worktrees/feature/pY-s3-hard-write-boundary-consolidation
target: integrate/phase-Y
---

# Sprint Y.3 — Hard Write-Boundary Consolidation

```yaml
plan_type: sprint_plan
phase: Y
sprint: Y.3
worktree: ../atm-core-worktrees/feature/pY-s3-hard-write-boundary-consolidation
branch: feature/pY-s3-hard-write-boundary-consolidation
status: planned
estimated_scope: large
```

## Goal

Remove direct command ownership of compatibility inbox writes and consolidate
all remaining ATM-authored inbox/config writes behind one hard-owned boundary
before append-only work or smoke testing begins.

## Scope Summary

- delete the `send` and `ack` command-layer compatibility rewrite stacks
- retain exactly one normal runtime Claude-Code-only compatibility write owner
- retain explicit admin/repair exceptions only where they are justified and
  documented
- encode the harness gate and the SQL-failure/original+error companion-message
  rule before implementation begins

## Governing Requirements

- `docs/plan-phase-Y.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- normal runtime JSONL append is allowed only for `Claude Code` harnesses
- harness selection is based on harness type, not model
- non-Claude harnesses must never receive ATM-authored JSONL append output
- the final runtime append path must be a single owned path
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

- `Y.1` and `Y.2` are complete
- the line-numbered write ledger in `docs/phase-Y/inbox-write-path-audit.md`
  has been reviewed and accepted as the authoritative removal list

## Hard Dependencies

- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- the current `feature/pY-trivial-fixes` call stacks sampled in that audit

## Non-Goals

- do not change the compatibility wire contract to append-only in this sprint
- do not start broad smoke/dogfood here
- do not preserve direct `send`/`ack` compatibility write behavior merely to
  avoid refactoring

## Sub-Tasks

### 1. Delete command-owned inbox rewrite entrypoints

Development work:
- remove direct compatibility inbox ownership from:
  - `crates/atm-core/src/send/mod.rs:280`
  - `crates/atm-core/src/send/mod.rs:463`
  - `crates/atm-core/src/ack/mod.rs:391`
- delete or narrow the shared helper at:
  - `crates/atm-core/src/send/mod.rs:482`
- ensure `crates/atm-core/src/workflow.rs:166` no longer coordinates mailbox
  file writes for normal runtime send/ack

Required tests:
- send succeeds without command-owned mailbox rewrite
- ack succeeds without command-owned mailbox rewrite
- no direct `send`/`ack` path can reach the low-level compatibility writer

Required doc or boundary updates:
- update `docs/phase-Y/inbox-write-path-audit.md`
- update crate boundary docs if any public/private owner edge changes

### 2. Retain exactly one normal runtime inbox writer owner

Development work:
- keep or move the runtime owner path around the current export stack:
  - `crates/atm-core/src/direct_boundaries.rs:38`
  - `crates/atm-core/src/boundary_support.rs:147`
  - `crates/atm-core/src/mailbox/mod.rs:137`
  - `crates/atm-core/src/mailbox/store.rs:36`
- make that path daemon-private or tightly subsystem-owned
- prevent arbitrary command code from calling the retained writer directly

Required tests:
- only the approved owner path can produce a normal runtime compatibility
  export
- non-owner attempts fail mechanically or are impossible to compile

Required doc or boundary updates:
- update `docs/phase-Y/inbox-write-path-audit.md`
- update `docs/atm-core/boundaries.md`
- update `docs/atm-daemon/boundaries.md`

### 3. Preserve only approved exceptional write paths

Development work:
- retain admin/repair inbox creation only at:
  - `crates/atm-core/src/team_admin.rs:313`
  - `crates/atm-core/src/team_admin.rs:455`
- retain `config.json` ownership only at:
  - `crates/atm-core/src/team_admin.rs:333`
  - `crates/atm-core/src/team_admin/restore.rs:97`
  - `crates/atm-core/src/team_admin.rs:486`
- confirm adjacent restore-state writers remain under explicit restore
  ownership and do not leak into runtime send/ack flows

Required tests:
- admin add-member still creates inbox/config deterministically
- restore still rebuilds inbox/config/task state deterministically

Required doc or boundary updates:
- update `docs/phase-Y/inbox-write-path-audit.md`
- update `docs/atm-core/modules/team_admin.md`

## Removal Ledger

Keep/delete/move decisions are authoritative and must be traced by file,
function, and line number.

- `crates/atm-core/src/send/mod.rs:280`
  - `append_mailbox_message_and_seed_workflow(...)`
  - decision: delete command ownership
- `crates/atm-core/src/send/mod.rs:463`
  - `append_mailbox_message_and_seed_workflow(...)`
  - decision: delete command ownership
- `crates/atm-core/src/ack/mod.rs:391`
  - `append_mailbox_message_and_seed_workflow(...)`
  - decision: delete command ownership
- `crates/atm-core/src/send/mod.rs:482`
  - `append_mailbox_message_and_seed_workflow(...)`
  - decision: delete or narrow to non-compatibility persistence only
- `crates/atm-core/src/send/mod.rs:516`
  - `load_store_backed_mailbox_projection(...)`
  - decision: remove from the runtime write stack unless a separate read-only
    projection use remains justified
- `crates/atm-core/src/send/mod.rs:548`
  - `mirror_message_to_store(...)`
  - decision: retain or move as SQLite-only persistence helper
- `crates/atm-core/src/workflow.rs:166`
  - `commit_workflow_state(...)`
  - decision: stop coordinating normal runtime inbox-file writes
- `crates/atm-core/src/mailbox/store.rs:19`
  - `write_compat_mailbox_projection(...)`
  - decision: retain for repair/rebuild only after Y.3
- `crates/atm-core/src/mailbox/store.rs:27`
  - `write_compat_mailbox_projection_with_policy(...)`
  - decision: keep reachable only behind the retained owner or delete in Y.5
- `crates/atm-core/src/mailbox/store.rs:36`
  - `write_compat_source_projections(...)`
  - decision: retain behind the sole runtime owner
- `crates/atm-core/src/mailbox/atomic.rs:28`
  - `write_messages(...)`
  - decision: retain temporarily behind the sole owner until Y.5
- `crates/atm-core/src/direct_boundaries.rs:38`
  - `export_source_files(...)`
  - decision: move/retain as sole runtime owner entrypoint
- `crates/atm-core/src/boundary_support.rs:147`
  - `export_source_files(...)`
  - decision: retain only if daemon-private and harness-gated
- `crates/atm-core/src/mailbox/mod.rs:137`
  - `export_compat_source_projections(...)`
  - decision: retain only behind the sole runtime owner
- `crates/atm-core/src/direct_boundaries.rs:44`
  - `reexport_messages(...)`
  - decision: retain for explicit repair/rebuild only
- `crates/atm-core/src/boundary_support.rs:169`
  - `reexport_messages(...)`
  - decision: retain for explicit repair/rebuild only
- `crates/atm-core/src/mailbox/mod.rs:143`
  - `export_compat_mailbox_projection(...)`
  - decision: retain only for explicit repair/rebuild
- `crates/atm-core/src/team_admin.rs:492`
  - `atomic_write(...)`
  - decision: retain only behind `write_team_config(...)`

## Split Recommendation

If the runtime owner extraction and the admin/repair exception cleanup do not
touch disjoint files cleanly, keep them in one sprint. If they separate
cleanly, do the runtime-owner extraction first and the admin/repair tightening
second, but do not leave the repo in a mixed command-owned state between them.

## Acceptance Criteria

- no normal `send` or `ack` command path can rewrite a compatibility inbox file
- exactly one runtime writer owner remains for Claude-Code harness export
- non-Claude harnesses still have no JSONL append path
- only the documented admin/repair exceptions remain outside the runtime owner

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/plan-phase-Y.md`
- `docs/project-plan.md`
- any boundary docs touched by the retained owner shape

## Risks And Watchouts

- this sprint must not invent a second runtime writer under a different name
- keep harness gating explicit; model-based branching is incorrect
- if an old dependency appears, remove or document it now rather than preserving
  it silently for later
