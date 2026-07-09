---
id: AD.34
title: Self-Ack Loop Termination And Historical Poison Cleanup
status: planned
branch: feature/pAD-s34-self-ack-loop-termination
worktree: ../atm-core-worktrees/feature/pAD-s34-self-ack-loop-termination
target: integrate/phase-AD
---

# Sprint AD.34 — Self-Ack Loop Termination And Historical Poison Cleanup

## Goal

- make `atm ack` converge when a historical self-addressed poison message
  already exists

## Hard Dependencies

- `AD.33` complete
- `docs/plans/phase-AD-followup/plan-atm-messaging-fixes.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- GitHub issue `#499`

## Exact Targets

- `crates/atm-core/src/ack/mod.rs`
- `crates/atm/src/commands/ack.rs`
- `crates/atm/src/output.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/commands/ack.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD34.md`

## Interfaces To Add Or Modify

The accepted ack reply-disposition contract after this sprint is:

```rust
pub enum AckReplyDisposition {
    SuppressedSelfAck,
    Sent {
        reply_message_id: AtmMessageId,
        reply_target: ReplyTarget,
    },
}

pub struct AckOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
    pub task_id: Option<TaskId>,
    pub reply_disposition: AckReplyDisposition,
    pub reply_text: String,
    pub warnings: Vec<WarningEntry>,
}
```

The accepted behavior after this sprint is:

- if the ack target is self-addressed, ATM marks it acknowledged and emits no
  reply message
- if the ack target is not self-addressed, normal reply behavior continues
- the output contract explicitly distinguishes "ack succeeded with no reply
  because self-ack was suppressed" from normal reply emission

## Paths To Delete

- any ack path that emits a new reply back to the same actor for a
  self-addressed message
- any output contract that assumes every successful ack produced a reply
  message

## Deliverables

- historical self-addressed poison messages can be acknowledged to completion
- self-ack no longer creates a replacement pending-ack message
- ack output and JSON shape describe suppressed-self-ack success explicitly

## This Sprint Does Not Close

- operator protocol / wrapper closeout
- the full cross-agent regression matrix

## Acceptance Criteria

- acking a historical self-addressed message marks it acknowledged
- acking a historical self-addressed message emits no new reply message
- non-self ack behavior remains unchanged
- CLI and JSON output distinguish suppressed self-ack from normal reply-send
  success

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted ack tests covering:
  - historical self-addressed poison message termination
  - normal non-self ack reply behavior unchanged
  - JSON/human output for suppressed self-ack
- `git diff --check`
