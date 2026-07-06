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

- make `atm read` report the message it actually mutated and the post-mutation
  bucket counts it actually produced, instead of mixing pre-mutation ids/counts
  with post-mutation unread selection

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
- `bucket_counts` always describe the post-mutation mailbox state returned by
  that command execution
- a read-side mutation may mark a message read and still return that same
  message in the output payload; the command must not silently swap the payload
  to the next unread message just because the selection mode was `--unread`

## Paths To Delete

- post-mutation reload logic in `crates/atm-core/src/read/mod.rs` that
  re-runs unread selection and substitutes the next unread message into the
  response payload after the original message has been marked read
- returning pre-mutation `selection.bucket_counts` after a successful
  mutation path
- any smoke/test expectation that treats mismatched `selected_message_id` and
  `message.message_id`/payload as acceptable read behavior

## Deliverables

- `atm read --unread --json` returns a payload consistent with the message it
  actually marked read
- `ReadOutcome.selected_message_id` and `ReadOutcome.message` refer to the same
  durable message after mutation
- `ReadOutcome.bucket_counts` reflect post-mutation mailbox state
- regression coverage proves the read-side mutation path and ack-side mutation
  path remain distinct, and that ack already clears `pending_ack_at` /
  populates `acknowledged_at` correctly

## This Sprint Does Not Close

- caller-context ownership
- raw CLI runtime-root unification beyond consuming the `AD.18` contract
- graft boundary reset

## Acceptance Criteria

- a targeted read-mutation test proves:
  - first `atm read --unread --json` marks the selected unread message read
  - returned `selected_message_id` identifies that mutated message
  - returned `message`, when present, matches that same mutated message rather
    than a later unread message
  - returned `bucket_counts.unread` is the post-mutation unread total
- repeated `atm read --unread --json` calls monotonically reduce unread counts
  until the unread surface is exhausted, with no off-by-one stale count in the
  returned payload
- ack regression coverage proves ack-side state mutation already persists
  `read=true`, clears `pending_ack_at`, and sets `acknowledged_at`
- `docs/requirements.md`, `docs/architecture.md`,
  `docs/atm-core/requirements.md`, and `docs/atm-core/architecture.md`
  describe `atm read` as a durable read-state mutation with self-consistent
  post-mutation output

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted `atm read` mutation regression coverage
- targeted ack-state regression coverage
- `just smoke normal`
- `git diff --check`
