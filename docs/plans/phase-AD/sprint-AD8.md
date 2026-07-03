---
id: AD.8
title: Claude Backend And Inbox Nudge Retirement
status: planned
branch: feature/pAD-s8-claude-backend-and-inbox-nudge-retirement
worktree: ../atm-core-worktrees/feature/pAD-s8-claude-backend-and-inbox-nudge-retirement
target: integrate/phase-AD
---

# Sprint AD.8 — Claude Backend And Inbox Nudge Retirement

## Goal

- retire `atm-storage-claude` and remove all post-send nudge/context-injection
  logic that still depends on Claude inbox JSON append behavior

## Hard Dependencies

- `AD.1` complete
- `AD.2` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-Y/delivery-state-machines.md`
- `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md`
- `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`

## Exact Targets

- `crates/atm-storage-claude/Cargo.toml`
- `crates/atm-storage-claude/src/lib.rs`
- `crates/atm-storage-claude/src/backend.rs`
- `crates/atm-storage-claude/src/compat.rs`
- `crates/atm-storage-claude/src/mailbox.rs`
- `crates/atm-storage-claude/src/paths.rs`
- `crates/atm-storage-claude/src/roster.rs`
- `boundaries/atm-storage-claude/message-store.toml`
- `boundaries/atm-storage-claude/roster-store.toml`
- `scripts/atm-nudge.py`
- `scripts/test_atm_nudge.py`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/service_runtime.rs`
- `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md`
- `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`
- docs/diagrams/state machines that still treat Claude inbox append as nudge
- code/tests that still assume inbox append can serve as post-send context
  injection
- operator docs that still describe Claude inbox append as an approved mailbox
  or nudge path

## Paths To Delete

- `crates/atm-storage-claude/Cargo.toml`
- `crates/atm-storage-claude/src/lib.rs`
- `crates/atm-storage-claude/src/backend.rs`
- `crates/atm-storage-claude/src/compat.rs`
- `crates/atm-storage-claude/src/mailbox.rs`
- `crates/atm-storage-claude/src/paths.rs`
- `crates/atm-storage-claude/src/roster.rs`
- `boundaries/atm-storage-claude/message-store.toml`
- `boundaries/atm-storage-claude/roster-store.toml`
- `scripts/atm-nudge-xml-1.py`

## Modified Surfaces

- modify remaining storage/runtime composition so no accepted production path
  depends on the retired Claude backend or on Claude inbox append, watcher
  import, rebuild, or context injection as a governing delivery/runtime path
- update ADR and architecture docs so backend interoperability remains
  mandatory after Claude backend retirement
- rewrite docs/diagrams/tests so Claude inbox append is historical only
- modify any surviving local nudge tooling so it no longer models Claude inbox
  append as part of delivery

## Obsolescence Instructions

- any temporary compile scaffolding left in runtime glue after Claude backend
  deletion must be marked
  `Phase AD obsolete: historical Claude mailbox compatibility only`
- obsolete compatibility helpers may remain only long enough to complete module
  deletion; they must not gain new production call sites or new documented
  behavior

## Deliverables

- `atm-storage-claude` is removed from the accepted line
- no accepted runtime path or doc still claims Claude inbox JSON append is a
  mailbox, nudge, delivery, or context-injection path
- the surviving local nudge path, if any, no longer depends on Claude inbox
  append semantics
- the shared `atm-storage` contract remains the governing backend seam after
  Claude backend retirement

## Required Work

- delete `atm-storage-claude` and its boundary records
- delete or rewrite obsolete nudge/context-injection logic
- rewrite state-machine/documentation text that still models Claude append as a
  mailbox or nudge path
- delete duplicate or stale nudge helpers when they exist only to preserve the
  retired inbox/context-injection model
- restate architecture so SQLite remains one backend implementation and future
  SQL backend support remains explicit
- restate docs so backend interoperability is preserved by the shared contract,
  not by requiring multiple live concrete backends after Claude retirement

## This Sprint Does Not Close

- local or graft emitter implementation
- roster drift repair
- smoke/readiness closeout

## Acceptance Criteria

- no accepted line still ships `atm-storage-claude` or its boundary records
- no accepted doc states that Claude inbox JSON append is an approved mailbox,
  delivery, nudge, or context-injection mechanism
- no accepted runtime code path still depends on Claude inbox append, watcher
  import, or rebuild behavior for message delivery, read semantics, or
  post-send emission
- the shared backend contract remains intact and documented as future-SQL-ready
- the accepted docs explicitly state that backend interoperability survives
  with one live concrete backend because the shared contract remains
  future-backend-ready
- every path listed under `Paths To Delete` is absent from the accepted line

## Required Validation

- doc/code grep gates for obsolete Claude nudge wording/logic
- targeted boundary-lint / boundary-grep gates for deleted Claude backend
  boundary TOMLs
- `test ! -e crates/atm-storage-claude/Cargo.toml`
- `test ! -e crates/atm-storage-claude/src/lib.rs`
- `test ! -e crates/atm-storage-claude/src/backend.rs`
- `test ! -e crates/atm-storage-claude/src/compat.rs`
- `test ! -e crates/atm-storage-claude/src/mailbox.rs`
- `test ! -e crates/atm-storage-claude/src/paths.rs`
- `test ! -e crates/atm-storage-claude/src/roster.rs`
- `test ! -e boundaries/atm-storage-claude/message-store.toml`
- `test ! -e boundaries/atm-storage-claude/roster-store.toml`
- `test ! -e scripts/atm-nudge-xml-1.py`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`
