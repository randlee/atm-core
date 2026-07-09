---
id: AD.31
title: Mailbox Peek Surface And Owner-Only Mutation Reset
status: planned
branch: feature/pAD-s31-mailbox-peek-and-owner-only-mutation
worktree: ../atm-core-worktrees/feature/pAD-s31-mailbox-peek-and-owner-only-mutation
target: integrate/phase-AD
---

# Sprint AD.31 — Mailbox Peek Surface And Owner-Only Mutation Reset

## Goal

- split mailbox inspection from mailbox mutation by adding `atm peek`
- remove impersonation from every mutating mailbox/message command

## Hard Dependencies

- accepted `AD.30` baseline merged into `integrate/phase-AD`
- `docs/plans/phase-AD-followup/plan-atm-messaging-fixes.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- GitHub issues `#498`, `#499`, `#500`

## Exact Targets

- `crates/atm/src/commands/peek.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/commands/ack.rs`
- `crates/atm/src/commands/clear.rs`
- `crates/atm/src/commands/help.rs`
- `crates/atm/src/main.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-core/src/identity/mod.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/commands/peek.md`
- `docs/atm/commands/read.md`
- `docs/atm/commands/list.md`
- `docs/atm/commands/send.md`
- `docs/atm/commands/ack.md`
- `docs/atm/commands/clear.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/adr/ADR-021-owner-only-message-mutation.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD31.md`

## Interfaces To Add Or Modify

The accepted CLI/runtime ownership contract after this sprint is:

```rust
pub struct PeekQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub target_address: Option<AgentAddress>,
    pub team_override: Option<TeamName>,
    pub selection_mode: ReadSelection,
    pub seen_state_filter: bool,
    pub message_id_filter: Option<AtmMessageId>,
    pub sender_filter: Option<AgentName>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub task_filter: Option<TaskId>,
    pub contains_filter: Option<String>,
    pub timeout_secs: Option<u64>,
}

pub struct ReadQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor: AgentName,
    pub target_address: Option<AgentAddress>,
    pub team: TeamName,
    pub selection_mode: ReadSelection,
    pub seen_state_filter: bool,
    pub seen_state_update: bool,
    pub message_id_filter: Option<AtmMessageId>,
    pub sender_filter: Option<AgentName>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub task_filter: Option<TaskId>,
    pub contains_filter: Option<String>,
    pub timeout_secs: Option<u64>,
}

pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor: AgentName,
    pub team: TeamName,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

pub struct ClearQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor: AgentName,
    pub team: TeamName,
    pub selection_mode: ClearSelection,
    pub dry_run: bool,
}

pub struct SendRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub sender: AgentName,
    pub team: TeamName,
    pub to: AgentAddress,
    // remaining fields omitted
}
```

The accepted operator contract after this sprint is:

- `atm read` is owner-only and mutates mailbox state
- `atm peek` is non-mutating and may inspect another member with `--as`
- `atm send`, `atm read`, `atm ack`, and `atm clear` must not expose sender or
  actor impersonation flags
- mutating commands fail closed when caller identity or team is unresolved
- ATM does not implement a special exception for "maybe the actual agent" when
  `ATM_IDENTITY` is unset
- `atm list` may continue to allow `--as` because it is inspection-only

## Paths To Delete

- `--no-mark` from the public `atm read` CLI surface
- `--as` from `atm read`, `atm ack`, and any mutating mailbox command help
  surface
- `--from` / sender override from the public `atm send` CLI surface
- `actor_override` / `sender_override` fields from mutating request types
- any product or command doc that still describes mutating impersonation as an
  accepted operation

## Deliverables

- `atm peek` exists as the explicit non-mutating mailbox inspection command
- `atm read` remains the owner-only mutating command
- mutating commands no longer accept impersonation
- the shared identity helpers are split so inspection-only surfaces may still
  resolve `--as`, while mutating surfaces resolve only the actual caller
- requirements, architecture, command docs, and a new ADR define the
  owner-only mutation rule unambiguously

## This Sprint Does Not Close

- durable `requires_ack` persistence
- read-time ack creation removal
- self-addressed send rejection
- self-ack poison termination

## Acceptance Criteria

- `atm peek` accepts the selection/filter surface that `atm read --no-mark`
  used to own, but performs no seen-state or ack-state mutation
- `atm read` no longer exposes `--no-mark`
- `atm send`, `atm read`, `atm ack`, and `atm clear` no longer expose sender
  or actor impersonation flags
- mutating commands fail closed when caller identity/team cannot be resolved
- docs and help text state clearly that only inspection-only commands may
  impersonate another member

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted CLI tests covering:
  - `atm peek --as` inspection success without mutation
  - `atm read` owner-only mutation
  - `atm send`, `atm read`, `atm ack`, and `atm clear` impersonation flag
    rejection
- targeted runtime tests proving `peek` leaves per-message `read`,
  `pending_ack_at`, and `acknowledged_at` unchanged, and does not advance the
  per-agent seen-state watermark stored by
  `crates/atm-core/src/read/seen_state.rs`
- `git diff --check`
