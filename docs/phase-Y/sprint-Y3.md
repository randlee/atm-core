---
id: Y.3
title: Hard Write-Boundary Consolidation
status: complete
branch: feature/pY-s3-hard-write-boundary-consolidation
worktree: ../atm-core-worktrees/feature/pY-s3-hard-write-boundary-consolidation
target: integrate/phase-Y
---

# Sprint Y.3 — Hard Write-Boundary Consolidation

## Goal

Remove direct command ownership of compatibility inbox writes and consolidate
all remaining ATM-authored inbox/config writes behind one hard-owned boundary
before append-only work or smoke testing begins.

## Scope Summary

- delete the `send` and `ack` command-layer compatibility rewrite stacks
- retain exactly one normal runtime Claude-Code-only compatibility write owner
- retain explicit admin/repair exceptions only where they are justified and
  documented
- prepare the surviving runtime owner shape that `Y.4` will route through
- leave delivery-policy coordination and state-machine implementation to `Y.4`

## Governing Requirements

- `docs/plan-phase-Y.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- normal runtime JSONL append is allowed only for `Claude Code` harnesses
- harness selection is based on harness type, not model
- non-Claude harnesses must never receive ATM-authored JSONL append output
- the final runtime append path must be a single owned path

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
- the current sampled implementation call stacks recorded in that audit

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
  - `crates/atm-core/src/send/mod.rs:464`
  - `crates/atm-core/src/ack/mod.rs:347`
- narrow the shared helper at:
  - `crates/atm-core/src/send/mod.rs:483`
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
  - `persist_message_and_seed_workflow(...)`
  - decision: direct compatibility rewrite deleted; post-commit runtime refresh only
- `crates/atm-core/src/send/mod.rs:464`
  - `persist_message_and_seed_workflow(...)`
  - decision: direct compatibility rewrite deleted; post-commit runtime refresh only
- `crates/atm-core/src/ack/mod.rs:347`
  - `persist_message_and_seed_workflow(...)`
  - decision: reply emission retained as send-shaped persistence only
- `crates/atm-core/src/send/mod.rs:483`
  - `persist_message_and_seed_workflow(...)`
  - decision: narrowed to SQLite/workflow persistence plus post-commit runtime refresh
- `crates/atm-core/src/send/mod.rs:517`
  - `load_store_backed_mailbox_projection(...)`
  - decision: remove from the runtime write stack unless a separate read-only
    projection use remains justified
- `crates/atm-core/src/send/mod.rs:549`
  - `mirror_message_to_store(...)`
  - decision: retain or move as SQLite-only persistence helper
- `crates/atm-core/src/workflow.rs:166`
  - `commit_workflow_state(...)`
  - decision: stop coordinating normal runtime inbox-file writes
- `crates/atm-core/src/service_runtime.rs:51`
  - `refresh_compat_inbox_projection(...)`
  - decision: retain as the sole normal runtime compatibility rewrite owner
- `crates/atm-core/src/service_runtime.rs:192`
  - `load_store_backed_mailbox_projection(...)`
  - decision: retain behind the runtime refresh owner and load immutable stored envelopes
- `crates/atm-core/src/mailbox/store.rs:19`
  - `write_compat_mailbox_projection(...)`
  - decision: retain for repair/rebuild only after Y.3
- `crates/atm-core/src/mailbox/store.rs:27`
  - `write_compat_mailbox_projection_with_policy(...)`
  - decision: keep reachable only behind the retained owner or delete in Y.6
- `crates/atm-core/src/mailbox/store.rs:36`
  - `write_compat_source_projections(...)`
  - decision: retain behind the sole runtime owner
- `crates/atm-core/src/mailbox/atomic.rs:28`
  - `write_messages(...)`
  - decision: retain temporarily behind the sole owner until Y.6
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
- `crates/atm-core/src/service_runtime.rs:44`
  - `maybe_run_post_send_hook(...)`
  - decision: retain as side-effect trait only; no event legality here
- `crates/atm-core/src/service_runtime.rs:143`
  - `maybe_run_post_send_hook(...)`
  - decision: retain as runtime bridge only
- `crates/atm-core/src/send/mod.rs:705`
  - `maybe_run_post_send_hook(...)`
  - decision: retain only as thin façade or remove if NotificationSink absorbs
    it directly
- `crates/atm-core/src/send/hook.rs:57`
  - `hook::maybe_run_post_send_hook(...)`
  - decision: retain only as fallback side-effect executor
- `crates/atm-core/src/send/hook.rs:90`
  - `execute_post_send_hook(...)`
  - decision: retain under NotificationSink side-effect ownership only

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
- no delivery-policy implementation is added here that should instead land in
  `Y.4`

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

## Completion Notes

- normal `send` and missing-config notice flows now persist SQLite/workflow
  state first and reach compatibility rewrite only through
  `RetainedServiceRuntime::refresh_compat_inbox_projection(...)`
- `ack` no longer owns source-inbox compatibility rewrites; only reply emission
  reuses the narrowed send-shaped persistence helper
- `clear` remains SQLite/state-only and does not own a compatibility rewrite
- the retained runtime refresh path now exports immutable stored envelopes so
  post-send state transitions do not rewrite prior compatibility message state
- if an old dependency appears, remove or document it now rather than preserving
  it silently for later
