---
id: AD.1
title: Caller Context Ownership Restore
status: planned
branch: feature/pAD-s1-caller-context-ownership-restore
worktree: ../atm-core-worktrees/feature/pAD-s1-caller-context-ownership-restore
target: integrate/phase-AD
---

# Sprint AD.1 — Caller Context Ownership Restore

## Goal

- restore correct caller identity and caller team ownership on the ATM command
  surface

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
- `crates/atm/src/observability.rs`
- `crates/atm-core/src/identity/`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-core/src/send/`
- `crates/atm-core/src/read/`
- `crates/atm-core/src/ack/`
- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/clear/`
- `crates/atm-core/src/doctor/`
- `crates/atm-core/src/team_admin.rs`
- `docs/plans/phase-AD/`

## Interfaces To Add Or Modify

- add one CLI-owned caller-context resolver used by retained ATM commands
- add one shared override-normalization path so retained ATM commands consume
  caller identity and caller team through the same code rather than parsing
  `ATM_IDENTITY`, `ATM_TEAM`, or repo config independently
- add required `caller_identity: AgentName` and
  `caller_team: TeamName` fields to caller-owned request DTOs that cross the
  CLI/daemon boundary:
  - `SendRequest`
  - `AckRequest`
  - `ReadQuery`
  - `ListQuery`
  - `ClearQuery`
- update daemon dispatch/request decode so caller-owned requests fail closed if
  the required caller-context fields are absent or invalid
- update command-entry helpers so command-line overrides win over
  invoking-shell `ATM_IDENTITY` / `ATM_TEAM`, but no other fallback is
  allowed
- update retained non-daemon and local-only command entry points (`log`,
  `teams`, `members`, and their subcommands) so they use the same
  caller-context rule even when they do not dispatch through the daemon
- preserve `doctor` as a diagnostic command that does not require caller
  identity; optional team scoping remains separate from caller-context
  enforcement

## Exact Implementation Shape

- add `crates/atm/src/commands/caller_context.rs`
- move retained command caller-context ownership into that module; do not
  duplicate env parsing in `send.rs`, `read.rs`, `ack.rs`, `list.rs`,
  `clear.rs`, `log.rs`, `doctor.rs`, `members.rs`, or `teams.rs`
- define these exact core helper types in the new module:

```rust
pub(crate) struct CallerContext {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
}

pub(crate) struct CallerContextOverrides<'a> {
    pub identity_override: Option<&'a str>,
    pub team_override: Option<&'a str>,
}
```

- define one exported retained-command entry helper in that module:

```rust
pub(crate) fn resolve_cli_caller_context(
    overrides: CallerContextOverrides<'_>,
) -> Result<CallerContext, AtmError>
```

- `resolve_cli_caller_context(...)` must implement this exact precedence:
  - caller identity: explicit command override if present, else invoking-shell
    `ATM_IDENTITY`, else `CallerIdentityUnresolved`
  - caller team: explicit command override if present, else invoking-shell
    `ATM_TEAM`, else `CallerTeamUnresolved`
- `resolve_cli_caller_context(...)` must parse both values into
  `AgentName` / `TeamName` before returning
- `resolve_cli_caller_context(...)` must not:
  - read `.atm.toml`
  - read repo-local default-team config
  - inspect daemon state
  - inspect roster state
  - consult `ATM_IDENTITY` / `ATM_TEAM` from any process other than the
    invoking CLI process
- retained ATM commands must not call `std::env::var("ATM_IDENTITY")`,
  `std::env::var("ATM_TEAM")`, or equivalent env helpers outside
  `caller_context.rs`

## Per-Command Override Mapping

- `atm send`
  - caller identity override source: `SendCommand.from`
  - caller team override source: `SendCommand.team`
- `atm read`
  - caller identity override source: `ReadCommand.actor` (`--as`)
  - caller team override source: `ReadCommand.team`
- `atm ack`
  - caller identity override source: `AckCommand.actor` (`--as`)
  - caller team override source: `AckCommand.team`
- `atm list`
  - caller identity override source: `ListCommand.actor` (`--as`)
  - caller team override source: `ListCommand.team`
- `atm clear`
  - caller identity override source: `ClearCommand.actor_override` (`--as`)
  - caller team override source: `ClearCommand.team`
- `atm doctor`
  - caller identity override source: none; caller identity is not required
  - caller team override source: `DoctorCommand.team` when present; otherwise
    diagnostic scope remains unset
- `atm log`
  - caller identity override source: none; invoking-shell `ATM_IDENTITY` is
    mandatory
  - caller team override source: none; invoking-shell `ATM_TEAM` is mandatory
- `atm members`
  - caller identity override source: none; invoking-shell `ATM_IDENTITY` is
    mandatory
  - caller team override source: `MembersCommand.team` when present, else
    invoking-shell `ATM_TEAM`
- `atm teams`
  - caller identity override source: none; invoking-shell `ATM_IDENTITY` is
    mandatory
  - caller team override source: invoking-shell `ATM_TEAM`
- `atm teams add-member`
  - caller identity override source: none; invoking-shell `ATM_IDENTITY` is
    mandatory
  - caller team override source: invoking-shell `ATM_TEAM`
  - note: positional `team` is the target roster team, not caller team, and
    must not be reused as caller context
- `atm teams backup`
  - caller identity override source: none; invoking-shell `ATM_IDENTITY` is
    mandatory
  - caller team override source: invoking-shell `ATM_TEAM`
  - note: positional `team` is the backup target, not caller team
- `atm teams restore`
  - caller identity override source: none; invoking-shell `ATM_IDENTITY` is
    mandatory
  - caller team override source: invoking-shell `ATM_TEAM`
  - note: positional `team` is the restore target, not caller team

## Wiring Rules

- every retained ATM command `run(...)` method must resolve caller context once
  at command entry before composing any request/query or touching the daemon
- daemon-backed commands must thread the resolved `CallerContext` into the
  request/query builder and then into the `atm-core` request DTO
- local-only commands must still call `resolve_cli_caller_context(...)` even if
  the downstream retained operation does not currently need caller identity for
  business logic
- `members` / `teams` / `log` must fail closed at CLI entry when caller
  identity or caller team is missing; they must not remain "special cases"
- `doctor` must remain outside the shared caller-context failure path:
  - no caller-identity requirement on the direct local path
  - no caller-identity requirement on the daemon-routed path
  - optional `--team` continues to scope diagnostics when supplied
- no command may silently substitute a target team, repo-local default team, or
  roster-derived team as caller team

## DTO And Entry-Point Changes

- add `caller_identity: AgentName` and `caller_team: TeamName` as required
  fields on these daemon-crossing shapes:
  - `SendRequest`
  - `AckRequest`
  - `ReadQuery`
  - `ListQuery`
  - `ClearQuery`
- request decode/dispatch must reject missing caller-context fields before
  command execution; downstream `Option<AgentName>` / `Option<TeamName>` caller
  fields are not allowed on retained daemon-backed commands after this sprint
- local-only retained commands do not need to invent daemon DTOs, but they do
  need to receive a resolved `CallerContext` at CLI entry and fail closed when
  resolution fails

## Deliverables

- bare ATM commands in an `arch-ctm` shell no longer resolve as
  `team-lead@atm-dev`
- daemon-backed request envelopes carry required caller identity and caller
  team fields
- explicit overrides still win over invoking-shell `ATM_IDENTITY` /
  `ATM_TEAM`
- unresolved caller identity or caller team fails at the CLI boundary before
  daemon dispatch or retained command execution
- every retained ATM command that already exists before AD.9 uses the shared
  caller-context resolver rather than per-command fallback logic, except
  `doctor`, which remains identity-free by design

## Required Work

- add `caller_context.rs` as the only retained-command env-resolution module
- wire `send`, `read`, `ack`, `list`, `clear`, `log`, `members`, `teams`,
  `teams add-member`, `teams backup`, and `teams restore` through
  `resolve_cli_caller_context(...)`
- thread resolved caller identity/team into daemon-backed request DTOs and
  fail closed before daemon dispatch when resolution fails
- fail closed at CLI entry for local-only retained commands instead of letting
  repo config, roster state, or daemon ambient env supply caller context
- delete or obsolete every production caller-context fallback outside
  `caller_context.rs`
- keep `doctor` out of the caller-context resolver and document/test that it
  runs without caller identity while still honoring optional `--team`

## Explicit Code Samples

```rust
pub struct SendRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    // existing send fields...
}

pub struct ReadQuery {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    // existing read fields...
}
```

```rust
fn resolve_cli_caller_context(
    overrides: CallerContextOverrides<'_>,
) -> Result<CallerContext, AtmError> {
    // one shared retained-command entry path; all ATM commands call this
}
```

## Error Contract

- `CallerIdentityUnresolved` / `ATM_IDENTITY_UNAVAILABLE`
  - cause: a caller-context-owned command reached the CLI boundary with neither an
    explicit caller override nor invoking-shell `ATM_IDENTITY`
  - emitted by: `resolve_cli_caller_context(...)`
  - sender surface: command failure before daemon dispatch
  - recovery: set `ATM_IDENTITY` in the invoking shell or pass the explicit
    `--as` / `--from` override the command supports
  - daemon contact: forbidden
- `CallerTeamUnresolved` / `ATM_TEAM_UNAVAILABLE`
  - cause: a retained ATM command reached command entry with neither an
    explicit team override nor invoking-shell `ATM_TEAM`
  - emitted by: `resolve_cli_caller_context(...)`
  - caller surface: command failure before retained execution or daemon
    dispatch
  - recovery: set `ATM_TEAM` in the invoking shell or pass the explicit
    `--team` override the command supports
  - daemon contact: forbidden

## Obsolescence Instructions

- any caller-context helper that falls back to hook files, repo config, or
  daemon ambient environment becomes obsolete in this sprint
- if a legacy helper cannot be deleted immediately, mark it as
  `Phase AD obsolete: caller-owned context fallback forbidden`, remove all
  production call sites, and forbid new call sites while AD remains open

## This Sprint Does Not Close

- caller-context coverage for `atm teams update-member`; that command is added
  in `AD.9` and closes there using the same shared resolver
- obsolete config identity removal
- post-send nudge simplification
- roster drift repair

## Acceptance Criteria

- `ATM_IDENTITY=arch-ctm ATM_TEAM=atm-dev atm read ...` reads
  `arch-ctm@atm-dev` state
- `ATM_IDENTITY=arch-ctm ATM_TEAM=atm-dev atm send ...` sends as
  `arch-ctm@atm-dev`
- `ATM_IDENTITY=arch-ctm ATM_TEAM=atm-dev atm ack ...` replies as
  `arch-ctm@atm-dev`
- `ATM_IDENTITY=arch-ctm ATM_TEAM=atm-dev atm list ...`,
  `atm clear ...`, `atm log ...`, `atm members ...`,
  `atm teams ...`, `atm teams add-member ...`, `atm teams backup ...`, and
  `atm teams restore ...` all execute against `arch-ctm@atm-dev` caller
  context rather than guessed fallback context
- `atm doctor ...` runs without requiring `ATM_IDENTITY`
- `atm doctor --team atm-dev ...` scopes diagnostics to the supplied team, but
  bare `atm doctor ...` still works when `ATM_TEAM` is unset
- explicit `--as` / `--from` / `--team` continues to override invoking-shell
  `ATM_IDENTITY` / `ATM_TEAM`
- if neither explicit override nor invoking-shell `ATM_IDENTITY` /
  `ATM_TEAM` is present, caller-context-owned retained ATM commands fail
  locally and do not dispatch to the daemon
- no validated reproduction remains where a bare `arch-ctm` command resolves
  as `team-lead@atm-dev`

## Required Validation

- targeted command tests for env-only success across:
  - `send`
  - `read`
  - `ack`
  - `list`
  - `clear`
  - `log`
  - `members`
  - `teams`
  - `teams add-member`
  - `teams backup`
  - `teams restore`
- targeted command tests for CLI-only caller-context success across commands
  that expose both caller-identity and caller-team override surfaces:
  - `send`
  - `read`
  - `ack`
  - `list`
  - `clear`
- targeted precedence tests proving explicit CLI caller-context overrides win
  over env across:
  - `send`
  - `read`
  - `ack`
  - `list`
  - `clear`
- targeted missing-context failure tests across caller-context-owned commands:
  - missing identity failure
  - missing team failure
- targeted doctor tests proving:
  - `atm doctor` succeeds without `ATM_IDENTITY`
  - `atm doctor` succeeds without `ATM_TEAM`
  - `atm doctor --team <team>` still scopes diagnostics when supplied
- targeted unit coverage for `resolve_cli_caller_context(...)` itself:
  - explicit identity/team override wins over env
  - env identity/team works when override is absent
  - missing identity fails with `CallerIdentityUnresolved`
  - missing team fails with `CallerTeamUnresolved`
  - invalid explicit identity/team fails during parsing
  - invalid env identity/team fails during parsing
- targeted command-entry tests proving no retained command reads caller context
  from repo-local `.atm.toml`, roster state, or daemon ambient state
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `git diff --check`
