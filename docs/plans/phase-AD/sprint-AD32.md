---
id: AD.32
title: Durable Ack Intent And Read Semantics Reset
status: planned
branch: feature/pAD-s32-durable-ack-intent-reset
worktree: ../atm-core-worktrees/feature/pAD-s32-durable-ack-intent-reset
target: integrate/phase-AD
---

# Sprint AD.32 — Durable Ack Intent And Read Semantics Reset

## Goal

- make ack requirement a durable sender-owned message property
- delete read-time ack creation

## Hard Dependencies

- `AD.31` complete
- `docs/plans/phase-AD-followup/plan-atm-messaging-fixes.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- GitHub issue `#500`

## Exact Targets

- `crates/atm-core/src/schema/inbox_message.rs`
- `crates/atm-core/src/types.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/read/state.rs`
- `crates/atm-core/src/read/metadata_selection.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/send/persistence.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/threading.rs`
- `crates/atm-core/src/workflow.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/composition.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/adr/ADR-021-owner-only-message-mutation.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD32.md`

## Interfaces To Add Or Modify

The durable message contract after this sprint is:

```rust
pub struct InboxMessage {
    pub schema_version: u32,
    pub message_id: AtmMessageId,
    pub from: AgentName,
    pub text: String,
    pub timestamp: IsoTimestamp,
    pub requires_ack: bool,
    pub pending_ack_at: Option<IsoTimestamp>,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub acknowledges_message_id: Option<AtmMessageId>,
    pub task_id: Option<TaskId>,
    // remaining fields omitted
}

pub enum AckRequirementState {
    NotRequired,
    RequiredPending,
    RequiredAcknowledged,
}

pub fn derive_ack_requirement(message: &InboxMessage) -> AckRequirementState;
```

The accepted behavior after this sprint is:

- `requires_ack` is persisted on every message
- only send/ack creation paths may set `requires_ack`
- read and peek must never create ack-required state
- `AckActivationMode::PromoteDisplayedUnread` is deleted
- ack replies persist `requires_ack = false`
- the compatibility rule for legacy rows with no explicit `requires_ack`
  field is deterministic:
  - `requires_ack = true` only when the legacy row has
    `pending_ack_at.is_some()` and `acknowledges_message_id.is_none()`
  - `requires_ack = false` otherwise

That compatibility rule is intentional because it preserves legitimate
historical sender-required messages while preventing legacy ack replies from
re-qualifying as ack-required just because they were displayed.

## Paths To Delete

- `AckActivationMode::PromoteDisplayedUnread`
- any read-path code that sets `pending_ack_at` because a message was displayed
- any requirement or doc wording that says read creates ack-required state
- any test that still treats `atm read` display as the source of ack
  obligation

## Deliverables

- `InboxMessage` persists a durable `requires_ack` field
- read/peek semantics no longer create pending-ack state
- ack state is derived from sender-owned durable intent plus acknowledgement
  completion, not display-time mutation
- the accepted compatibility rule for legacy rows is implemented and tested

## This Sprint Does Not Close

- self-addressed send rejection
- self-ack poison termination
- operator protocol / wrapper closeout

## Acceptance Criteria

- a plain informational send persists `requires_ack = false`
- an explicit `--requires-ack` send persists `requires_ack = true`
- task sends persist `requires_ack = true`
- ack replies persist `requires_ack = false`
- `atm read` and `atm peek` never create `pending_ack_at`
- legacy ack replies with `acknowledges_message_id` do not become ack-required
  during compatibility load

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted schema/runtime tests covering:
  - plain send
  - `--requires-ack` send
  - task send
  - ack reply
  - legacy row compatibility load with and without
    `acknowledges_message_id`
- targeted read/peek tests proving no display-time ack promotion remains
- `git diff --check`
