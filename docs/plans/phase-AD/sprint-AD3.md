---
id: AD.3
title: Post-Send Nudge Contract Simplification
status: planned
branch: feature/pAD-s3-post-send-nudge-contract-simplification
worktree: ../atm-core-worktrees/feature/pAD-s3-post-send-nudge-contract-simplification
target: integrate/phase-AD
---

# Sprint AD.3 — Post-Send Nudge Contract Simplification

## Goal

- simplify post-send nudge ownership back to one direct post-commit seam

## Hard Dependencies

- `AD.1` complete
- `AD.2` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `#440`

## Exact Targets

- `crates/atm-core/src/send/`
- `crates/atm-core/src/ack/`
- `crates/atm-core/src/send/hook.rs`
- `crates/atm-core/src/config/mod.rs`
- `crates/atm-core/src/delivery_plan.rs`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/boundary/mod.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Interfaces To Add Or Modify

- define the accepted post-send event contract explicitly:

```rust
pub struct PostSendHookEvent {
    pub sender: AgentName,
    pub sender_team: TeamName,
    pub recipient: AgentName,
    pub recipient_team: TeamName,
    pub message_id: AtmMessageId,
    pub requires_ack: bool,
    pub is_ack: bool,
    pub task_id: Option<TaskId>,
    pub recipient_pane_id: Option<PaneId>,
}

pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

- modify `send` and `ack` finalization so they call the direct emitter seam
  after persistence and own sender-visible warning construction directly
- modify post-send capability lookup so it no longer depends on caller working
  directory or unrelated repo-local `.atm.toml` discovery

## Deliverables

- one accepted post-send contract:
  - persist message
  - emit nudge only when recipient exposes post-send capability
  - log and warn on emission failure
- post-send ownership no longer hidden behind generic delivery-plan behavior
- live post-send capability resolution no longer changes based on the caller's
  current working directory

## Required Work

- document the simplified post-send runtime contract directly in the code/docs
- narrow post-send responsibility away from generic plan construction where it
  obscures the simple send model
- remove caller-CWD-dependent config lookup from live post-send capability
  decisions
- preserve durable delivery behavior while shrinking post-send ownership to one
  direct seam

## Explicit Code Samples

```rust
pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

```rust
// Required send shape:
persist_message(...)?;
if recipient_has_post_send_hook {
    if let Err(error) = post_send_hook_emitter.emit(&event) {
        log_post_send_failure(&error);
        warnings.push(render_post_send_warning(&error));
    }
}
```

## Obsolescence Instructions

- `DeliveryPlan`, `ReplyDeliveryPlan`, `execute_delivery_plan(...)`,
  `execute_reply_delivery_plan(...)`, and `NotificationSink`-based post-send
  orchestration become obsolete for normal send/ack post-send behavior in this
  sprint
- if any of those helpers cannot be deleted immediately, mark them
  `Phase AD obsolete: not the governing post-send seam`, remove all new
  send/ack callers, and carry them only until the relevant AD.5 / AD.8
  deletion work has landed when those retained paths still exist

## This Sprint Does Not Close

- local tmux-backed emitter implementation
- graft-backed emitter implementation
- Claude inbox nudge deletion

## Acceptance Criteria

- the accepted design for post-send nudge execution is stated directly in docs
  and code-facing seams
- post-send behavior is explicitly modeled as post-commit emission, not a
  generic planned side-effect bundle
- sender warning ownership on emission failure is explicit and testable
- no validated reproduction remains where running `atm send` from an unrelated
  repo or working directory changes whether post-send emission is attempted

## Required Validation

- targeted tests or compile gates for the narrowed contract seam
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`
