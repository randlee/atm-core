---
id: TEAMS-RM.02
title: Add `atm teams remove-member` CLI Subcommand
status: planned
branch: TBD - implementation happens on a new feature worktree, not this planning worktree
worktree: TBD - execution worktree not assigned (see Execution Note)
target: integrate/phase-<current-active-phase>, per project-plan.md branch flow (not `develop` directly)
---

# Sprint 02 — Add `atm teams remove-member` CLI Subcommand

```yaml
plan_type: sprint_plan
phase: teams-remove-member
sprint: TEAMS-RM.02
worktree: planning/localhost-fix-remove-member (planning only)
branch: feature/atm-teams-remove-member (implementation)
status: planned
estimated_scope: one CLI subcommand, roster mutation, requirements, generated surface docs, and focused tests
```

## Execution Note

This document is a planning artifact only, produced from the
`planning/localhost-fix-remove-member` worktree. It contains no Rust code
changes. Implementation must happen on a **separate feature worktree**
branched from `develop` via `sc-git-worktree` (e.g.
`feature/teams-remove-member`), per `CLAUDE.md`'s branch-management rules.
This planning worktree must not be used to implement or commit code.

## Goal

Add `atm teams remove-member <team> <member>` as a peer/sibling command to
`atm teams add-member`, using the same argument shape, storage-layer
symmetry, error handling, and output conventions, so the retained `atm
teams` roster-repair surface supports both inserting and deleting a roster
entry through the same reviewed pattern.

## Scope Summary

Add the retained roster-repair command and its documentation without changing
the `RosterStore` boundary or deleting any inbox data. The implementation
branch is separate from this planning worktree.

## Governing Requirements

`docs/requirements.md` section 12.2 ("Retained Surface") currently states
the retained `teams` surface is `add-member`, `update-member`, `backup`,
`restore`, and **explicitly excludes** `remove-member` as an out-of-scope
historical orchestration command (docs/requirements.md:2040, "The retained
surface explicitly does not include ... `remove-member`").

This sprint changes that decision. Any implementation worktree for this
sprint must update `docs/requirements.md` §12 in the same PR to:
- move `remove-member` from the excluded list into the retained surface
  list (`REQ-P-TEAMS-001`)
- add a `atm teams remove-member` required-behavior subsection mirroring
  the existing `add-member`/`update-member` subsections (see Explicit
  Required Behavior below)

Do not implement the CLI/storage change without this requirements update —
that would leave the requirements doc and the shipped surface out of sync,
which is a structural (Blocking) finding under this repo's QA conventions.

## Governing ADRs

No ADR change is required. This sprint consumes the existing retained CLI and
SQLite roster ownership decisions; it must not introduce a second roster
mutation path.

## Governing Boundaries

- `RosterStore` remains the sole roster persistence boundary.
- CLI parsing and presentation remain in `crates/atm`; roster authorization
  and mutation remain in `atm-core/team_admin`.
- The command must use `load_roster` and `replace_roster`; no direct SQLite,
  filesystem, or inbox deletion path is permitted.

## Prerequisites

- The implementation worktree is created from the documented integration flow.
- Existing `add-member`, `update-member`, and CLI-surface baseline tests are
  available as the behavioral and generated-output references.

## Hard Dependencies

- `docs/requirements.md` §12 (`atm teams`) — must be updated as part of this
  sprint, not deferred.
- `crates/atm-core/src/team_admin/member_mutation.rs` — existing
  `add_member_with_roster_store` / `update_member_with_roster_store` pattern
  this sprint must mirror.
- `crates/atm-core/src/boundary.rs` `RosterStore` trait — no new trait
  method is required; `remove-member` reuses the existing
  `replace_roster`/`load_roster` primitives, the same way `add-member` does.
- `crates/atm/tests/cli_surface_baseline.json` — diff-gate baseline that
  must be regenerated in the same commit that adds the new subcommand
  surface (see Required Validation).

## Non-Goals

- Deleting the former member's inbox or data.
- A confirmation or `--force` workflow for self-removal or last-member removal.
- Restoring historical orchestration commands or changing `RosterStore`.

## Sub-Tasks

1. Update `REQ-P-TEAMS-001` and its retained-surface/required-behavior text.
   Add focused requirement tests/checks where the existing documentation gate
   expects them.
2. Add the core request, authorization, deterministic roster removal, outcome,
   and tests using the existing `RosterStore` mutation path.
3. Add CLI parsing, dispatch, human/JSON output, generated CLI documentation,
   and the additive CLI-surface baseline update.

## Split Recommendation

Keep the requirements, core mutation, CLI wiring, generated CLI artifacts, and
tests in one implementation PR: they form one externally visible command and
its only safe closure proof.

## Exact Targets

- `docs/requirements.md` (§12, `REQ-P-TEAMS-001` retained-surface list and
  required-behavior subsections)
- `crates/atm-core/src/team_admin/member_mutation.rs` (new
  `RemoveMemberRequest`, `RemoveMemberOutcome`,
  `remove_member_with_roster_store`, `ensure_member_present`)
- `crates/atm-core/src/team_admin.rs` (re-export the new request/outcome
  types alongside `AddMemberRequest`/`UpdateMemberRequest`)
- `crates/atm/src/commands/teams.rs` (new `RemoveMemberCommand`,
  `TeamsSubcommand::RemoveMember` variant, dispatch arm, unit tests)
- `crates/atm/src/output.rs` (new `print_remove_member_result`)
- `crates/atm/tests/cli_surface_baseline.json` (regenerated, additive-only)
- `docs/atm/cli-reference-<version>.md` (regenerated via the existing
  `gen_cli_docs` example, per the `cli_surface` test's documented flow)
- `CHANGELOG.md` (new `Unreleased` entry)

## Design: Symmetry With `update-member`, Not `add-member`, On Caller Authorization

`remove-member` is destructive and irreversible in the same way
`update-member` is not (a mis-targeted `update-member` call is merely a
metadata edit; a mis-targeted `remove-member` call permanently drops a
roster entry). It must **not** mirror `add-member`'s caller-less shape.
Instead, on the caller-authorization axis specifically, `remove-member`
mirrors `update-member`: it takes a `caller_context: CallerContext` and
enforces that the caller belongs to the target team before the removal is
allowed, via a `validate_remove_member_caller` guard that mirrors
`update-member`'s `validate_update_member_caller`
(`crates/atm-core/src/team_admin/member_mutation.rs:205-225`). On every
other axis (CLI args, storage read, roster mutation shape, output
conventions), `remove-member` remains a direct peer of `add-member`:

| Aspect | `add-member` | `update-member` | `remove-member` (this sprint) |
|---|---|---|---|
| CLI args | `team`, `member` positional + `--json` | `team`, `member` positional + `--json` | `team`, `member` positional + `--json` |
| Storage read | `projection::load_team_roster(roster_store, &team)` | same | same |
| Caller authorization | none — no `caller_context` param, no team-membership check | `run(self, _atm_home_dir, caller_context: CallerContext)`; `validate_update_member_caller` requires the caller belong to the target team (`member_mutation.rs:205-225`) | **follows `update-member`, not `add-member`**: `run(self, caller_context: CallerContext)`; `validate_remove_member_caller` requires the caller belong to the target team, mirroring `validate_update_member_caller` verbatim |
| Storage guard (existence) | `ensure_member_absent` → `AtmError::member_already_exists` | `ensure_member_present` equivalent → `AtmError::member_not_found` | `ensure_member_present` → `AtmError::member_not_found` (already exists in `crate::error`, reused verbatim, same as `update-member`'s not-found path) |
| Roster mutation | push a new `RosterEntry`, then `roster_store.replace_roster(&team, &existing_roster)` | mutate matching `RosterEntry` fields, then `roster_store.replace_roster(&team, &existing_roster)` | filter out the matching `RosterEntry` by `agent_name`, then `roster_store.replace_roster(&team, &existing_roster)` — the literal inverse mutation over the same primitive, no new `RosterStore` trait method |
| Outcome type | `AddMemberOutcome { action: "add-member", team, member, created_inbox }` | `UpdateMemberOutcome { action: "update-member", ... }` | `RemoveMemberOutcome { action: "remove-member", team, member }` (mirrors `UpdateMemberOutcome`'s shape since there is no inbox side effect — see Non-Closure) |
| CLI wiring | `TeamsSubcommand::AddMember(AddMemberCommand)` → `command.run(home_dir)` | `TeamsSubcommand::UpdateMember(UpdateMemberCommand)` → `command.run(atm_home_dir, caller_context)` (`teams.rs:295`) | `TeamsSubcommand::RemoveMember(RemoveMemberCommand)` → `command.run(caller_context)`, same call shape as `update-member`, **not** `add-member` |
| Output | `output::print_add_member_result(&outcome, json)` | `output::print_update_member_result(&outcome, json)` | `output::print_remove_member_result(&outcome, json)`, same `if json { pretty json } else { one-line summary }` shape |
| Error propagation | `.map_err(Into::into)` from `AtmError` → `anyhow::Result` | same | same |

## Explicit Code Samples

```rust
// crates/atm-core/src/team_admin/member_mutation.rs

/// Parameters for removing one member from a team roster.
///
/// Carries `caller_identity`/`caller_team` (same shape as
/// `UpdateMemberRequest`, not `AddMemberRequest`) because `remove-member`
/// enforces caller authorization — see `validate_remove_member_caller`.
#[derive(Debug, Clone)]
pub struct RemoveMemberRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub team: TeamName,
    pub member: AgentName,
}

impl RemoveMemberRequest {
    pub fn new(
        caller_identity: AgentName,
        caller_team: TeamName,
        team: &str,
        member: &str,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            caller_identity,
            caller_team,
            team: team.parse()?,
            member: member.parse()?,
        })
    }
}

/// Result of removing one member from a team roster.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RemoveMemberOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub member: AgentName,
}

/// Remove one member record from a team roster.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member does not
/// exist, or roster persistence fails.
pub fn remove_member_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: RemoveMemberRequest,
) -> Result<RemoveMemberOutcome, AtmError> {
    validate_remove_member_caller(roster_store, &request)?;
    let mut existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    ensure_member_present(&existing_roster, &request.team, &request.member)?;
    existing_roster.retain(|entry| entry.agent_name != request.member);
    roster_store.replace_roster(&request.team, &existing_roster)?;

    Ok(RemoveMemberOutcome {
        action: "remove-member",
        team: request.team,
        member: request.member,
    })
}

fn ensure_member_present(
    existing_roster: &[RosterEntry],
    team: &TeamName,
    member: &AgentName,
) -> Result<(), AtmError> {
    if !existing_roster
        .iter()
        .any(|existing_member| existing_member.agent_name == *member)
    {
        return Err(AtmError::member_not_found(member.as_str(), team.as_str()));
    }
    Ok(())
}

/// Require that the calling identity is itself a member of the target team
/// before a removal is allowed. This is the same authorization requirement
/// `update-member` enforces via `validate_update_member_caller`
/// (`crates/atm-core/src/team_admin/member_mutation.rs:205-225`) — mirrored
/// here verbatim because `remove-member` is destructive/irreversible like
/// `update-member`, not additive like `add-member` (which enforces no such
/// check).
fn validate_remove_member_caller(
    roster_store: &dyn RosterStore,
    request: &RemoveMemberRequest,
) -> Result<(), AtmError> {
    if request.caller_team != request.team {
        return Err(AtmError::validation(format!(
            "caller team '{}' does not match remove-member target team '{}'",
            request.caller_team, request.team
        )));
    }

    let caller_entry = roster_store.query_membership(&request.team, &request.caller_identity)?;
    if caller_entry.is_none() {
        return Err(AtmError::member_not_found(
            request.caller_identity.as_str(),
            request.team.as_str(),
        ));
    }

    Ok(())
}
```

```rust
// crates/atm/src/commands/teams.rs

#[derive(Debug, Args)]
struct RemoveMemberCommand {
    team: String,
    member: String,

    #[arg(long)]
    json: bool,
}

impl RemoveMemberCommand {
    // Signature mirrors `UpdateMemberCommand::run` (`teams.rs:295`), not
    // `AddMemberCommand::run` — `caller_context` is required so
    // `validate_remove_member_caller` can enforce that the caller belongs
    // to the target team before the removal proceeds.
    fn run(self, caller_context: CallerContext) -> Result<()> {
        let json = self.json;
        let request = self.build_request(caller_context)?;
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::remove_member_with_roster_store(roster_store, request)
        })?;
        output::print_remove_member_result(&outcome, json)
    }

    fn build_request(self, caller_context: CallerContext) -> Result<RemoveMemberRequest> {
        RemoveMemberRequest::new(
            caller_context.caller_identity,
            caller_context.caller_team,
            &self.team,
            &self.member,
        )
        .map_err(Into::into)
    }
}

// enum TeamsSubcommand adds: RemoveMember(RemoveMemberCommand),
// TeamsCommand::run dispatch adds:
//   Some(TeamsSubcommand::RemoveMember(command)) => command.run(caller_context),
```

```rust
// crates/atm/src/output.rs

/// Print one remove-member result in human-readable or JSON form.
pub fn print_remove_member_result(outcome: &RemoveMemberOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!("Removed member {} from {}", outcome.member, outcome.team);
    }
    Ok(())
}
```

## Explicit Required Behavior (mirrors `docs/requirements.md` §12.3 shape)

`atm teams remove-member` must:
- validate that the calling identity belongs to the target team before any
  other check runs, via `validate_remove_member_caller`, the same
  requirement `update-member` enforces via `validate_update_member_caller`
  (`crates/atm-core/src/team_admin/member_mutation.rs:205-225`); reject with
  a caller/target-team mismatch error otherwise. `add-member` does **not**
  enforce this and is not the pattern followed here.
- validate that the target team exists (via `RosterStore::load_roster`,
  same as `add-member`/`update-member`)
- validate that the target member currently exists in that team's roster;
  reject with `AtmError::member_not_found` otherwise (same error type
  `update-member` already uses for its missing-member case)
- persist the roster with the member entry removed, deterministically, via
  `RosterStore::replace_roster` — the same single mutation primitive
  `add-member` uses, applied as the inverse operation
- support `--json` output using the same pretty-printed-JSON /
  one-line-human-summary convention as every other `teams` subcommand
- exit non-zero with a readable error message on team-not-found or
  member-not-found, consistent with `add-member`/`update-member`

## Edge Cases (must be covered by acceptance criteria + tests)

1. **Removing the last member of a team**: allowed. Neither
   `add-member` nor `update-member` enforces a minimum-member invariant
   today, and this sprint does not introduce one. `atm teams remove-member`
   may leave a team with zero roster entries; the team directory itself is
   untouched.
2. **Removing a non-existent member**: rejected with
   `AtmError::member_not_found`, non-zero exit, same message shape as
   `update-member`'s equivalent failure.
3. **Removing the currently-authenticated caller identity**: allowed, no
   special-cased rejection. `add-member` performs no caller-identity check
   at all; `remove-member` follows the same precedent rather than
   introducing new caller-context validation this sprint does not otherwise
   need. This is a deliberate, explicit design choice — call it out in the
   PR description — not an oversight; a future sprint may add a
   confirmation/`--force` gate for self-removal if operators report
   accidental self-lockout, but that is out of scope here (see Non-Closure).
4. **Removing from a non-existent team**: rejected the same way
   `add-member`/`update-member` reject it today (via
   `projection::load_team_roster` surfacing the team-not-found error).
5. **Caller is not a member of the target team (cross-team caller)**:
   rejected before any roster mutation is attempted. `validate_remove_member_caller`
   requires `request.caller_team == request.team` and that
   `roster_store.query_membership(&team, &caller_identity)` resolves to a
   present entry — the identical requirement `update-member` enforces via
   `validate_update_member_caller`
   (`crates/atm-core/src/team_admin/member_mutation.rs:205-225`). This is
   new relative to `add-member`, which has no caller-context parameter and
   performs no such check; `remove-member` does not follow `add-member`'s
   precedent here because removal is destructive/irreversible.

## Non-Closure

This sprint does **not**:
- delete or touch the removed member's inbox directory/files. `add-member`
  creates an inbox as a side effect; `remove-member` intentionally does
  **not** delete one, to avoid silent, irreversible mailbox data loss. Inbox
  cleanup (if ever wanted) is a separate, explicitly-scoped future sprint.
- add a `--force`/confirmation gate for self-removal or last-member removal.
- restore the broader historical `teams` orchestration surface (`spawn`,
  `join`, `resume`, `cleanup`) that `docs/requirements.md` §12.2 still
  excludes — only `remove-member` moves from excluded to retained.
- change `RosterStore` trait shape; no new trait method is added.

## Deliverables

1. `docs/requirements.md` §12 updated: `remove-member` moved into the
   retained surface list, with a required-behavior subsection.
2. `RemoveMemberRequest` (with `caller_identity`/`caller_team` fields),
   `RemoveMemberOutcome`, `remove_member_with_roster_store`,
   `ensure_member_present`, `validate_remove_member_caller` added to
   `crates/atm-core/src/team_admin/member_mutation.rs` and re-exported from
   `crates/atm-core/src/team_admin.rs`.
3. `RemoveMemberCommand` + `TeamsSubcommand::RemoveMember` wired into
   `crates/atm/src/commands/teams.rs`, `run(self, caller_context: CallerContext)`
   dispatched from `TeamsCommand::run` with the caller context (same call
   shape as `UpdateMemberCommand::run`, `teams.rs:295`).
4. `output::print_remove_member_result` added to `crates/atm/src/output.rs`.
5. Unit tests for `remove_member_with_roster_store` (success, member-not-found,
   team-not-found, cross-team-caller-rejected) in `member_mutation.rs`'s
   existing test module pattern.
6. CLI-level tests for `RemoveMemberCommand` in `teams.rs`'s existing test
   module, following the `update_member_command`/`Fixture` pattern, including
   a cross-team-caller-rejected case, using `TEST_TEAM = "test-team"` (never
   the literal `"atm-dev"`, per RULE-008).
7. `crates/atm/tests/cli_surface_baseline.json` regenerated
   (`ATM_CLI_SURFACE_BLESS=1 cargo test -p agent-team-mail --test
   cli_surface`, or the `gen_cli_docs` example) so the diff-gate reflects
   the new subcommand as an accepted addition, not a drift.
8. `docs/atm/cli-reference-<version>.md` regenerated in the same commit as
   the baseline (per `cli_surface.rs`'s documented flow).
9. `CHANGELOG.md` `Unreleased` entry describing the new subcommand.

## Acceptance Criteria

- `atm teams remove-member <team> <member>` exists, is documented in
  `docs/requirements.md` §12, and behaves per "Explicit Required Behavior"
  above.
- Removing an existing member deterministically drops exactly that one
  `RosterEntry` from the team's roster and leaves all other entries
  byte-for-byte unchanged.
- Removing a non-existent member fails with `AtmError::member_not_found`
  and a non-zero exit, without mutating the roster.
- Removing from a non-existent team fails the same way `add-member` does
  today, without creating any new team state.
- Removing the last member is allowed, leaves an empty roster, and does not
  delete the team directory or any inbox data; a focused test proves it.
- A caller may remove its own roster entry when it is a member of the target
  team; this succeeds through the ordinary authorization and mutation path,
  and a focused test proves it. No confirmation/force policy is added here.
- A caller whose `caller_team` does not match the target team, or who is
  not present in the target team's roster, is rejected by
  `validate_remove_member_caller` before any roster mutation occurs — the
  same requirement `update-member` enforces, verified by a dedicated
  cross-team-caller-rejected test case.
- `--json` output round-trips through `serde_json` and matches the
  `RemoveMemberOutcome` shape (`action`, `team`, `member`).
- No `RosterStore` trait signature changes; `remove-member` is implemented
  entirely through `load_roster`/`replace_roster`, the same primitives
  `add-member` uses.
- No test in the new coverage uses the literal `"atm-dev"` team name or
  spawns the daemon (RULE-008).
- `cli_surface` diff-gate test passes with the new subcommand present in
  the committed baseline as an additive-only change (no removed/renamed
  subcommand or argument anywhere else in the tree).
- `docs/requirements.md` §12.2's excluded-command list no longer lists
  `remove-member`.

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo test -p agent-team-mail --test cli_surface`
- `rg -n 'remove-member' docs/requirements.md`
- `rg -n 'RemoveMember|remove_member_with_roster_store|validate_remove_member_caller' crates/atm-core/src/team_admin/member_mutation.rs crates/atm/src/commands/teams.rs crates/atm/src/output.rs`
- `! rg -n '"atm-dev"' crates/atm/src/commands/teams.rs`
- `git diff --check`

## Required Document Updates

- `docs/requirements.md` §12 retained surface and required behavior.
- `docs/atm/cli-reference-<version>.md` and the CLI-surface baseline generated
  by the repository's existing toolchain.
- `CHANGELOG.md` under `Unreleased`.

## Risks And Watchouts

- Self-removal and last-member removal are intentionally allowed only because
  this sprint explicitly preserves the current no-minimum-member invariant;
  do not silently add a new policy while implementing the command.
- The requirements change, generated CLI artifacts, and implementation must
  land together. Omitting any one leaves the shipped CLI surface inconsistent.

## References

- `crates/atm-core/src/team_admin/member_mutation.rs` (existing
  `add_member_with_roster_store` / `update_member_with_roster_store`
  pattern this sprint mirrors)
- `crates/atm-core/src/team_admin/member_mutation.rs:205-225`
  (`validate_update_member_caller` — the exact pattern
  `validate_remove_member_caller` mirrors, since `remove-member` is
  destructive/irreversible like `update-member`, not additive like
  `add-member`)
- `crates/atm/src/commands/teams.rs` (existing `AddMemberCommand` /
  `UpdateMemberCommand` CLI wiring and test module)
- `crates/atm/src/commands/teams.rs:295` (`UpdateMemberCommand::run(self,
  _atm_home_dir, caller_context)` — the call shape `RemoveMemberCommand::run`
  follows, not `add-member`'s caller-less `run(self, atm_home_dir)`)
- `crates/atm/src/output.rs:542-562` (`print_add_member_result` /
  `print_update_member_result` output conventions)
- `crates/atm-core/src/boundary/store.rs:174-186` (`RosterStore` trait —
  reused unchanged)
- `docs/requirements.md:2000-2040` (§12 `atm teams`, current
  `remove-member` exclusion this sprint reverses)
- `crates/atm/tests/cli_surface.rs` (additive-only CLI surface diff gate)
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md` (sprint-doc
  shape and production-ready expectation this doc follows)
