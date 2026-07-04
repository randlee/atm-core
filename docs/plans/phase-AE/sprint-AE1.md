---
id: AE.1
title: atm teams remove-member Subcommand And Send Pipeline Fix
status: planned
branch: feature/pAE-s1-remove-member-and-send-pipeline
worktree: ../atm-core-worktrees/feature/pAE-s1-remove-member-and-send-pipeline
target: integrate/phase-AE
---

# Sprint AE.1 — `atm teams remove-member` Subcommand And Send Pipeline Fix

## Goal

- add `atm teams remove-member <team> <name>` subcommand
- fix `atm send` to resolve CLI-local inputs (--stdin, --file) before IPC dispatch

## Hard Dependencies

- `docs/plans/phase-AE/plan-phase-AE.md`
- `docs/requirements.md`
- `docs/atm/requirements.md`
- `docs/atm-core/requirements.md`
- `#423`
- `#448`

## Exact Targets

- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/send/`
- `crates/atm-core/src/roster/`
- `crates/atm-core/src/protocol.rs`

## Interfaces To Add Or Modify

### remove-member subcommand

- add `TeamsCommand::RemoveMember { team: TeamName, name: AgentName }` variant
- add `atm teams remove-member <team> <name>` CLI parsing
- caller context: invoking-shell `ATM_IDENTITY` mandatory, `ATM_TEAM` mandatory
  (positional `team` is the target roster team, not caller team)
- daemon-backed: send `RemoveMemberRequest { caller_identity, caller_team, target_team, agent_name }`
- daemon removes member from SQLite roster, updates inbox directory if present
- on success: emit success message with removed member identity
- on failure: `MemberNotFound`, `TeamNotFound`, or standard caller-context errors

### send pipeline fix

- `atm send --stdin` reads payload from stdin at CLI boundary before IPC dispatch
- `atm send --file <path>` reads payload from file at CLI boundary before IPC dispatch
- `atm send <message>` positional payload remains the default
- `--stdin` and `--file` are mutually exclusive; error on both
- payload resolution happens in `crates/atm/src/commands/send.rs` before
  constructing `SendRequest`
- `SendRequest` gains `payload_source: PayloadSource` enum:
  ```rust
  pub enum PayloadSource {
      Positional(String),
      Stdin(Vec<u8>),
      File(PathBuf),
  }
  ```
- daemon receives resolved payload bytes, not path or stdin reference
- large payloads (>1MB) warn but do not reject

## Deliverables

- `atm teams remove-member atm-dev test-agent` removes `test-agent` from `atm-dev` roster
- `atm send --stdin team-lead <<< "message"` sends stdin content
- `atm send --file .prompts/review.md team-lead` sends file content
- both `--stdin` and `--file` fail with clear error
- caller-context enforcement matches AD.1 rules for both surfaces

## Required Validation

- `atm teams remove-member` succeeds with valid team and member
- `atm teams remove-member` fails with `MemberNotFound` for unknown member
- `atm teams remove-member` fails with `CallerIdentityUnresolved` without `ATM_IDENTITY`
- `atm send --stdin` pipes payload correctly to daemon
- `atm send --file <path>` pipes file content correctly
- `atm send --stdin --file <path>` fails with mutual-exclusion error
- payload >1MB warns but completes
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
