---
id: AD.35
title: Messaging Protocol And Regression Closeout
status: planned
branch: feature/pAD-s35-messaging-protocol-and-regression-closeout
worktree: ../atm-core-worktrees/feature/pAD-s35-messaging-protocol-and-regression-closeout
target: integrate/phase-AD
---

# Sprint AD.35 — Messaging Protocol And Regression Closeout

## Goal

- make the repaired ATM messaging model the only documented and regression
  tested operator path

## Hard Dependencies

- `AD.34` complete
- `docs/plans/phase-AD-followup/plan-atm-messaging-fixes.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- GitHub issues `#498`, `#499`, `#500`

## Exact Targets

- `docs/team-protocol.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/commands/read.md`
- `docs/atm/commands/list.md`
- `docs/atm/commands/send.md`
- `docs/atm/commands/ack.md`
- `docs/atm/commands/clear.md`
- `docs/atm/commands/help.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/adr/ADR-021-owner-only-message-mutation.md`
- `crates/atm/src/commands/help.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/readiness.md`
- `docs/plans/phase-AD/sprint-AD35.md`

## Interfaces To Add Or Modify

No additional core protocol type is introduced in this sprint. This sprint
closes the operator/documentation contract around the types and behaviors
landed in `AD.31` through `AD.34`.

The accepted operator contract after this sprint is:

- `atm peek` is the only documented non-mutating mailbox inspection path
- informational review uses `atm peek`, not `atm read --no-mark`
- only inspection surfaces may impersonate another member
- mutating surfaces fail closed when caller identity/team is unresolved
- only sender-owned durable `requires_ack` data may require acknowledgement

## Paths To Delete

- `atm read --no-mark` from all operator and command documentation
- any protocol text that tells agents to ack every displayed message without
  distinguishing requires-ack vs informational classes
- any help text that still advertises mutating impersonation

## Deliverables

- team protocol and command docs reflect the new `peek`/owner-only mutation
  model
- help text is aligned with the shipped command surface
- one authoritative end-to-end regression matrix covers `#498`, `#499`, and
  `#500`
- the Phase `AD` readiness/project-plan records show the new follow-up line as
  closed only through these regression gates

## This Sprint Does Not Close

- no additional runtime behavior beyond the AD.31-AD.34 contract

## Acceptance Criteria

- docs no longer mention `atm read --no-mark`
- docs no longer advertise mutating impersonation
- the end-to-end regression matrix covers:
  - `peek --as` inspection without mutation
  - owner-only `read` mutation
  - explicit `--requires-ack` send
  - plain informational send with no ack requirement
  - task send
  - self-addressed send rejection
  - historical self-ack poison termination
  - cross-agent ack reply not re-promoted into pending-ack
- `docs/plans/phase-AD/readiness.md` is updated only when the full AD.31-AD.35
  follow-up line is actually green

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- one authoritative end-to-end regression suite covering the matrix above
- `git diff --check`
