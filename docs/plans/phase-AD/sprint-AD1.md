---
id: AD.1
title: Caller Identity Ownership Restore
status: planned
branch: feature/pAD-s1-caller-identity-ownership-restore
worktree: ../atm-core-worktrees/feature/pAD-s1-caller-identity-ownership-restore
target: integrate/phase-AD
---

# Sprint AD.1 — Caller Identity Ownership Restore

## Goal

- restore correct caller identity ownership on daemon-backed ATM commands

## Hard Dependencies

- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm-core/requirements.md`
- `#421`

## Exact Targets

- `crates/atm/src/commands/`
- `crates/atm/src/composition.rs`
- `crates/atm-core/src/identity/`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-core/src/send/`
- `crates/atm-core/src/read/`
- `crates/atm-core/src/ack/`
- `docs/plans/phase-AD/`

## Interfaces To Add Or Modify

- add one CLI-owned caller-identity resolver used by caller-owned commands
- add required `caller_identity: AgentName` fields to caller-owned request DTOs
  that cross the CLI/daemon boundary:
  - `SendRequest`
  - `AckRequest`
  - `ReadQuery`
  - `ListQuery`
  - `ClearQuery`
- update daemon dispatch/request decode so caller-owned requests fail closed if
  the required caller-identity field is absent or invalid
- update command-entry helpers so `--from` / `--as` override
  invoking-shell `ATM_IDENTITY`, but no other fallback is allowed

## Deliverables

- bare ATM commands in an `arch-ctm` shell no longer resolve as `team-lead`
- daemon-backed request envelopes carry required caller identity fields
- explicit overrides still win over invoking-shell `ATM_IDENTITY`
- unresolved caller identity fails at the CLI boundary before daemon dispatch

## Required Work

- define the one accepted identity ownership rule for daemon-backed commands
- make caller identity mandatory for every caller-owned daemon request shape;
  downstream identity must never be optional
- make bare `send`, `read`, `ack`, and any other actor-scoped commands honor
  invoking-shell `ATM_IDENTITY` without requiring `--as` or `--from`
- make CLI command entry points reject caller-owned commands when neither an
  explicit override nor invoking-shell `ATM_IDENTITY` is available
- remove any remaining reliance on daemon-process ambient identity for caller
  resolution

## Explicit Code Samples

```rust
pub struct SendRequest {
    pub caller_identity: AgentName,
    // existing send fields...
}

pub struct ReadQuery {
    pub caller_identity: AgentName,
    // existing read fields...
}
```

```rust
fn resolve_cli_caller_identity(...) -> Result<AgentName, AtmError> {
    // explicit override when supported, otherwise invoking-shell ATM_IDENTITY
}
```

## Error Contract

- `CallerIdentityUnresolved` / `ATM_IDENTITY_UNAVAILABLE`
  - cause: a caller-owned command reached the CLI boundary with neither an
    explicit caller override nor invoking-shell `ATM_IDENTITY`
  - emitted by: `resolve_cli_caller_identity(...)`
  - sender surface: command failure before daemon dispatch
  - recovery: set `ATM_IDENTITY` in the invoking shell or pass the explicit
    `--as` / `--from` override the command supports
  - daemon contact: forbidden

## Obsolescence Instructions

- any caller-owned identity helper that falls back to hook files or daemon
  ambient environment becomes obsolete in this sprint
- if a legacy helper cannot be deleted immediately, mark it as
  `Phase AD obsolete: caller-owned identity fallback forbidden`, remove all
  production call sites, and forbid new call sites while AD remains open

## This Sprint Does Not Close

- obsolete config identity removal
- post-send nudge simplification
- roster drift repair

## Acceptance Criteria

- `ATM_IDENTITY=arch-ctm atm read --team atm-dev` reads `arch-ctm` state
- `ATM_IDENTITY=arch-ctm atm send --team atm-dev ...` sends as `arch-ctm`
- explicit `--as` / `--from` continues to override invoking-shell
  `ATM_IDENTITY`
- if neither explicit override nor invoking-shell `ATM_IDENTITY` is present,
  caller-owned commands fail locally and do not dispatch to the daemon
- no validated reproduction remains where a bare `arch-ctm` command resolves as
  `team-lead`

## Required Validation

- targeted command tests for bare and explicit identity paths
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `git diff --check`
