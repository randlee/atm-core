---
id: AD.9
title: Update-Member CLI And Roster Repair Path
status: planned
branch: feature/pAD-s9-update-member-cli-and-roster-repair-path
worktree: ../atm-core-worktrees/feature/pAD-s9-update-member-cli-and-roster-repair-path
target: integrate/phase-AD
---

# Sprint AD.9 — Update-Member CLI And Roster Repair Path

## Goal

- create the accepted CLI repair path for existing member metadata on the
  canonical SQL-backed roster

## Hard Dependencies

- `AD.1` complete
- `AD.2` complete
- `AD.7` complete
- `AD.8` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/schema/agent_member.rs`
- `crates/atm/src/commands/caller_context.rs`
- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/members.rs`
- team startup / rmux / pane metadata guidance touched by pane repair

## Interfaces To Add Or Modify

```rust
pub struct UpdateMemberRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub team: TeamName,
    pub member: AgentName,
    pub home_dir: Option<PathBuf>,
    pub harness: Option<RosterHarness>,
    pub agent_type: Option<AgentType>,
    pub model: Option<ModelName>,
    pub tmux_pane_id: Option<PaneId>,
}
```

```rust
pub struct UpdateMemberCommand {
    team: String,
    member: String,
    #[arg(long)]
    home_dir: Option<PathBuf>,
    #[arg(long)]
    harness: Option<String>,
    #[arg(long)]
    agent_type: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long = "pane-id")]
    pane_id: Option<String>,
}
```

- add `atm teams update-member` as the accepted CLI mutation path for existing
  roster metadata
- keep `atm teams add-member` as create-only behavior; it must not become the
  repair/update path for existing members
- make `atm teams update-member` consume the same shared
  `resolve_cli_caller_context(...)` path introduced in `AD.1`
- modify the accepted CLI repair path so existing roster metadata can be set
  and repaired through ATM-owned commands against SQLite roster truth:
  - `home_dir`
  - `recipient_pane_id`
  - `model`
  - `harness`
  - `agent_type`
- modify doctor roster projection so pane drift is surfaced directly from the
  authoritative roster/store boundary
- modify startup/operator guidance so pane repair steps point to the accepted
  CLI path rather than to repo-local config edits

## Obsolescence Instructions

- any helper, doc, or script that treats `.atm.toml` as live pane-routing truth
  becomes obsolete in this sprint
- any workflow that tells operators to re-run `add-member` in order to repair
  existing member metadata becomes obsolete in this sprint
- if such helpers cannot be deleted immediately, mark them
  `Phase AD obsolete: pane truth lives in SQLite roster state`, block new
  production callers, and keep them out of the accepted repair flow

## Deliverables

- existing member metadata is updateable from the CLI through one accepted
  mutation path
- `atm teams update-member` enforces caller identity and caller team through
  the same shared CLI-owned resolver used by the rest of the retained ATM
  command surface
- durable member `home_dir` is stored on the canonical SQL-backed roster row
- authoritative pane metadata is restored for active team members in the
  existing SQLite roster rows
- active pane ids are settable and repairable from the CLI
- operator home-dir and pane repair guidance points to the accepted CLI path
  instead of repo-local config edits

## Required Work

- tighten roster drift detection around active member pane/registration truth
- add or finalize `atm teams update-member` for the existing SQLite-owned
  member metadata:
  - `home_dir`
  - `recipient_pane_id` / `tmux_pane_id`
  - `model`
  - `harness`
  - `agent_type`
- wire `atm teams update-member` through `resolve_cli_caller_context(...)`
  instead of parsing `ATM_IDENTITY` / `ATM_TEAM` separately or reusing target
  team as caller team
- require invoking-shell caller identity and caller team at CLI entry for
  `atm teams update-member`; the positional `team` argument remains the target
  roster team only
- remove lingering `.atm.toml` assumptions around active pane-id authority
- update operator guidance for restoring pane truth when drift occurs

## CLI Error Contract

- `MemberAlreadyExists` / `ATM_MEMBER_ALREADY_EXISTS`
  - cause: `atm teams add-member` targets a member row that already exists
  - caller surface: command failure with no roster mutation
  - recovery: use `atm teams update-member` for metadata repair on existing
    members instead of retrying `add-member`

## This Sprint Does Not Close

- caller identity ownership
- post-send emitter contract
- directory metadata terminology cleanup
- final readiness

## Acceptance Criteria

- doctor output accurately reflects repaired or still-drifting roster state
- active team pane/registration truth is restored for the accepted baseline
- the validated-on-entry blank `tmux_pane_id` drift for `team-lead` and
  `arch-ctm` is repaired on the accepted baseline
- operators can update existing member metadata through `atm teams
  update-member`
- `atm teams update-member` fails locally when caller identity or caller team
  is unavailable instead of guessing from repo config, roster state, or daemon
  ambient environment
- `atm teams update-member` uses invoking-shell `ATM_TEAM` as caller team and
  does not reinterpret the positional target `team` argument as caller context
- `atm teams add-member` remains create-only and rejects attempts to use it as
  an update path for existing members
- `atm teams update-member` accepts durable `home_dir` repair for existing
  members
- operators can set or repair active pane ids through the accepted CLI path
- the accepted baseline no longer depends on `.atm.toml` as the pane-id source
  of truth

## Required Validation

- targeted doctor/roster/pane-cli tests
- targeted `teams update-member` caller-context tests:
  - success with invoking-shell `ATM_IDENTITY` plus `ATM_TEAM`
  - missing-identity local failure
  - missing-team local failure
  - proof that positional target `team` does not satisfy caller-team
    resolution
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`
