---
id: AD.19
title: Read Mutation Output Consistency Repair
status: planned
branch: feature/pAD-s19-read-mutation-output-consistency-repair
worktree: ../atm-core-worktrees/feature/pAD-s19-read-mutation-output-consistency-repair
target: integrate/phase-AD
---

# Sprint AD.19 — Read Mutation Output Consistency Repair

## Goal

- make `atm read` report the selected message and reader-lane snapshot it
  actually observed, instead of mixing the selected id with a later unread
  payload or claiming durable post-handoff state in the response

## Hard Dependencies

- `AD.1` complete
- `AD.11` complete
- `AD.13` complete
- `AD.18` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/read/metadata_selection.rs`
- `crates/atm-core/src/read/state.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Interfaces To Add Or Modify

The accepted read-mutation contract after this sprint is:

```rust
pub struct ReadOutcome {
    pub mutation_applied: bool,
    pub message: Option<ClassifiedMessage>,
    pub selected_message_id: Option<AtmMessageId>,
    pub bucket_counts: BucketCounts,
}
```

with these invariants:

- if `mutation_applied == true` and `message.is_some()`, `message` describes
  the same durable message identified by `selected_message_id`
- `bucket_counts` describe the reader-lane snapshot and MAY predate durable
  handoff application
- `mutation_applied == true` means the read/seen transition was accepted into
  the supervised non-blocking handoff, not that it is already durable
- a read-side transition may later mark a message read while the response still
  returns that same selected message; the command must not silently swap the
  payload to the next unread message just because the selection mode was
  `--unread`

## Paths To Delete

- logic in `crates/atm-core/src/read/mod.rs` that re-runs unread selection and
  substitutes the next unread message into the response payload after the
  original selection
- any response contract that claims a post-handoff durable bucket snapshot
- any smoke/test expectation that treats mismatched `selected_message_id` and
  `message.message_id`/payload as acceptable read behavior

## Deliverables

- `atm read --unread --json` returns a payload consistent with the selected
  message whose transition it offered to the handoff
- `ReadOutcome.selected_message_id` and `ReadOutcome.message` refer to the
  same selected durable message
- `ReadOutcome.bucket_counts` reflect the reader-lane snapshot
- regression coverage proves the read-side mutation path and ack-side mutation
  path remain distinct, and that ack already clears `pending_ack_at` /
  populates `acknowledged_at` correctly

## This Sprint Does Not Close

- caller-context ownership
- raw CLI runtime-root unification beyond consuming the `AD.18` contract
- graft boundary reset
- metadata-path `--contains` / full-body search correctness; that closes in
  `AD.20`

## Acceptance Criteria

- a targeted read-mutation test proves:
- first `atm read --unread --json` accepts the selected unread message's
  read/seen transition into the supervised handoff
- returned `selected_message_id` identifies that selected message
- returned `message`, when present, matches that same selected message rather
  than a later unread message
- returned `bucket_counts` are the reader-lane snapshot and need not show the
  handoff result; a bounded later `atm list --json` poll observes durability
- repeated reads do not claim read-your-writes; callers use the bounded list
  poll when they require durable unread-count changes
- ack regression coverage proves ack-side state mutation already persists
  `read=true`, clears `pending_ack_at`, and sets `acknowledged_at`
- `docs/requirements.md`, `docs/architecture.md`,
  `docs/atm-core/requirements.md`, and `docs/atm-core/architecture.md`
  describe `atm read` as accepted non-blocking read-state handoff with a
  self-consistent reader-lane output snapshot

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted `atm read` mutation regression coverage
- targeted ack-state regression coverage
- `just smoke normal`
- `git diff --check`
