---
id: AD.8
title: Claude JSON Mailbox And Inbox Nudge Removal
status: planned
branch: feature/pAD-s8-claude-json-mailbox-and-inbox-nudge-removal
worktree: ../atm-core-worktrees/feature/pAD-s8-claude-json-mailbox-and-inbox-nudge-removal
target: integrate/phase-AD
---

# Sprint AD.8 — Claude JSON Mailbox And Inbox Nudge Removal

## Goal

- remove Claude JSON mailbox support and all post-send nudge/context-injection
  logic that still depends on Claude inbox JSON append behavior

## Hard Dependencies

- `AD.1` complete
- `AD.2` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-Y/delivery-state-machines.md`

## Exact Targets

- `scripts/atm-nudge.py`
- `crates/atm-storage-claude/src/lib.rs`
- `scripts/test_atm_nudge.py`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/service_runtime.rs`
- docs/diagrams/state machines that still treat Claude inbox append as nudge
- code/tests that still assume inbox append can serve as post-send context
  injection
- operator docs that still describe Claude inbox append as an approved mailbox
  or nudge path

## Paths To Delete

- `scripts/atm-nudge-xml-1.py`
- `crates/atm-storage-claude/src/backend.rs`
- `crates/atm-storage-claude/src/compat.rs`
- `crates/atm-storage-claude/src/mailbox.rs`
- `crates/atm-storage-claude/src/paths.rs`

## Modified Surfaces

- modify remaining storage/runtime composition so no accepted production path
  depends on Claude mailbox JSON read, write, ingest, export, or context
  injection
- rewrite docs/diagrams/tests so Claude inbox append is historical only
- modify any surviving local nudge tooling so it no longer models Claude inbox
  append as part of delivery

## Obsolescence Instructions

- any temporary compile scaffolding left in `atm-storage-claude` or
  `delivery_execution` must be marked
  `Phase AD obsolete: historical Claude mailbox compatibility only`
- obsolete compatibility helpers may remain only long enough to complete module
  deletion; they must not gain new production call sites or new documented
  behavior

## Deliverables

- no accepted code ships Claude JSON mailbox support
- no accepted code or docs still claim Claude inbox JSON append is a mailbox,
  nudge, or context-injection path
- the surviving local nudge path, if any, no longer depends on Claude inbox
  append semantics

## Required Work

- delete Claude JSON mailbox support that is no longer used by Claude Code
- delete or rewrite obsolete nudge/context-injection logic
- rewrite state-machine/documentation text that still models Claude append as a
  mailbox or nudge path
- delete duplicate or stale nudge helpers when they exist only to preserve the
  retired inbox/context-injection model

## This Sprint Does Not Close

- local or graft emitter implementation
- roster drift repair
- smoke/readiness closeout

## Acceptance Criteria

- no accepted doc states that Claude inbox JSON append is an approved mailbox,
  nudge, or context-injection mechanism
- no accepted code path still depends on Claude inbox append for mailbox
  ingestion or post-send emission
- every path listed under `Paths To Delete` is absent from the accepted line

## Required Validation

- doc/code grep gates for obsolete Claude nudge wording/logic
- `test ! -e scripts/atm-nudge-xml-1.py`
- `test ! -e crates/atm-storage-claude/src/backend.rs`
- `test ! -e crates/atm-storage-claude/src/compat.rs`
- `test ! -e crates/atm-storage-claude/src/mailbox.rs`
- `test ! -e crates/atm-storage-claude/src/paths.rs`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`
