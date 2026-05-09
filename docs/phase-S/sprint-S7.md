# Phase S.7 — Bounded Queue Query Implementation

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.7"
status: planned
estimated_scope: L
```

## Goal

Implement the queue-query split defined in ADR-009 so `atm list` becomes the
bounded metadata-search surface and `atm read` becomes the single-message
detail surface.

## Governing Requirements

- `REQ-P-LIST-001`
- `REQ-P-READ-001`
- `REQ-CORE-LIST-001`
- `REQ-CORE-WORKFLOW-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`

## Governing ADRs

- `docs/adr/ADR-009-bounded-queue-query-surface.md`

## Hard Dependencies

- S.5 planning hardening is merged
- the durable mailbox source of truth remains SQLite-backed
- successor/update chain semantics remain the current source of truth for
  logical message selection

## Required Work

1. Add the `atm list` CLI surface.
1.1 Create `crates/atm/src/commands/list.rs`.
1.2 Wire the command into:
   - `crates/atm/src/commands/mod.rs`
   - `crates/atm/src/main.rs`
   - `crates/atm/src/output.rs`

2. Convert `atm read` to single-message logical-current selection.
2.1 Update `crates/atm/src/commands/read.rs` so selector-driven reads return
    one most-recent logical current message.
2.2 Emit `selected_message_id`, `match_count`, and
    `additional_match_count` in JSON output and human-readable follow-up text.

3. Implement the shared list/read filter contract.
3.1 Keep `atm list` and `atm read` aligned on:
   - `--team`
   - `--from`
   - `--since`
   - `--task`
   - `--contains`
   - `--unread`
   - `--pending-ack`
   - `--all`
3.2 Implement the legacy `atm read` alias migration:
   - `--unread-only`
   - `--pending-ack-only`
   - `--history`
   - `--since-last-seen`

4. Split metadata query from full-message fetch in `atm-core`.
4.1 Implement the bounded list/query service path in a dedicated core module.
4.2 Keep successor/update-chain terminal-node collapse shared between list and
    selector-driven read paths.
4.3 Ensure bare `atm read` still prioritizes pending-ack messages ahead of
    non-ack unread messages.

5. Implement the bounded durable query path.
5.1 Extend the concrete SQLite path in `crates/atm-rusqlite/src/shared_db.rs`
    so list/count queries are bounded and do not materialize full mailbox
    history for operator-facing responses.
5.2 Keep selection semantics owned by `atm-core`, not by `atm-rusqlite`.

## Required Code Targets

- `crates/atm/src/commands/list.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/commands/mod.rs`
- `crates/atm/src/main.rs`
- `crates/atm/src/output.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/read/filters.rs`
- `crates/atm-core/src/read/wait.rs`
- `crates/atm-core/src/workflow.rs`
- `crates/atm-rusqlite/src/shared_db.rs`

## Required Document Updates

- `docs/atm/commands/list.md`
- `docs/atm/commands/read.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-core/modules/list.md`
- `docs/atm-core/modules/read.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/architecture.md`

## Acceptance Criteria

- `atm list` exists as a top-level CLI verb
- default queue inspection is bounded by query behavior rather than render
  truncation
- `atm read` returns exactly one logical current message
- selector-driven reads report extra matches in metadata instead of returning
  multiple full bodies
- successor/task-thread selection operates on terminal-node logical current
  messages rather than superseded predecessors
- the legacy read aliases remain supported only as deprecated compatibility
  spellings

## Required Validation

- `just lint`
- targeted CLI tests for `atm list` and `atm read`
- queue selection tests in `atm-core`
- SQLite-backed bounded-query tests in `atm-rusqlite`
