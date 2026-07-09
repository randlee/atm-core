---
id: AD.33
title: Self-Addressed Send Rejection
status: planned
branch: feature/pAD-s33-self-address-send-rejection
worktree: ../atm-core-worktrees/feature/pAD-s33-self-address-send-rejection
target: integrate/phase-AD
---

# Sprint AD.33 — Self-Addressed Send Rejection

## Goal

- reject self-addressed messages before persistence

## Hard Dependencies

- `AD.32` merged forward before implementation starts because both sprint
  lines touch shared send/read-facing surfaces and would otherwise create
  avoidable merge conflicts; this is a merge-order dependency, not a
  functional prerequisite for self-send rejection
- `docs/plans/phase-AD-followup/plan-atm-messaging-fixes.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- GitHub issue `#498`

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/send/tests.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-daemon/src/tests.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/commands/send.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD33.md`

## Interfaces To Add Or Modify

The accepted send validation contract after this sprint is:

```rust
pub fn validate_non_self_recipient(
    sender: &AgentName,
    sender_team: &TeamName,
    recipient: &ResolvedRecipient,
) -> Result<(), AtmError>;
```

The accepted behavior after this sprint is:

- ATM rejects `from == to` within the same team before persistence
- the rejection occurs for plain sends, threaded sends, task sends, and dry
  runs
- the shared `atm-core` send path owns the validation so CLI, loopback, and
  daemon-backed sends all fail the same way

## Paths To Delete

- any path that allows a message whose resolved sender and recipient are the
  same member in the same team
- any CLI/runtime test that still treats self-addressed sends as valid

## Deliverables

- self-addressed sends are rejected in the shared send path
- one stable typed error contract documents the failure
- all send entry paths surface the same rejection

## This Sprint Does Not Close

- historical self-addressed poison messages already persisted in a mailbox
- ack-loop termination for those historical messages
- operator protocol / wrapper closeout

## Acceptance Criteria

- no self-addressed message can be persisted
- `--dry-run` does not report success for a self-addressed send
- loopback, direct CLI, and daemon-backed send paths all surface the same
  validation failure
- docs state clearly that self-addressed messages are invalid ATM input

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted send tests covering:
  - plain send rejection
  - task send rejection
  - dry-run rejection
  - loopback transport rejection
  - daemon-backed send rejection
- `git diff --check`
