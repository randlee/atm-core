---
id: AY.13
phase: AY
sprint: AY.13
title: Doctor team scope — default to the caller's team, `--all-teams` for oversight
branch: feature/ay13-doctor-team-scope
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay13-doctor-team-scope
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: complete
recommended_agent: cipher
recommended_model: implementation
execution_track: parallel
parallel_with: [AY.9, AY.10, AY.11]
dependency_relations:
  - prerequisite: AY.13
    dependent: none
    relation: parallel_safe
    rationale: additive `atm doctor` CLI/report change under `crates/atm-core/src/doctor/` and `crates/atm/src/commands/doctor.rs`; no Herdr transport, composition, or boundary edits. Added by fenix 2026-09-07 from Rand's doctor-scope requirement.
---

# AY.13 — Doctor team scope

Rand (2026-09-07, verbatim requirement):

- "if team-lead runs doctor, I would expect only his team to show up."
- "if we run outside ATM_TEAM environment w/ no --team specified, then we
  would see all issues."
- "team-lead or other team-members doing roster check should ONLY see their
  workspace scope"
- "There should be an option to see EVERYTHING. --all-teams or --verbose so
  that you or team lead or a hermes oversight agent can look at everything.
  Most teams should use the default and ONLY see their team scope."
- "i.e. sc-lint team-lead doesn't have the knowledge to manage every team
  roster on the computer."

## Behaviour today (integrate/phase-ay @ 57cf977db)

`resolved_doctor_team` (`crates/atm-core/src/doctor/mod.rs`) picks
`--team`, else `config::resolve_team(None, config)` (which reads `ATM_TEAM`
and config defaults). The roster, presence (AYP-R6-002 per-member `agent get`),
mixed-backend, and graft-receiver sections run for that one team only. When
no team resolves, those sections are skipped silently and the report shows
no roster issues at all.

## Required behaviour

| Invocation | Scope |
|---|---|
| `ATM_TEAM=<t>` set, no flag | only team `<t>` (unchanged) |
| `--team <t>` | only team `<t>` (unchanged; overrides `ATM_TEAM`) |
| `--all-teams` | every team in the canonical roster store, one team section each |
| no `ATM_TEAM`, no `--team`, no `--all-teams` | same as `--all-teams`, plus one info-level finding stating that no team was resolved so every team is shown |
| `--all-teams` together with `--team` | clap conflict error, exit 2 |

"Every team" means the teams the roster store returns (the same source
`atm teams` uses: `RosterStore::list_teams`). Nothing is inferred from the
filesystem or from `~/.claude/teams/`.

## Deliverables

- D1 `crates/atm/src/commands/doctor.rs`: `--all-teams` flag
  (`conflicts_with = "team"`), carried in `DoctorQuery` as a new
  `all_teams: bool` field (`#[serde(default)]` so older daemon peers and
  recorded JSON still deserialize).
- D2 `crates/atm-core/src/doctor/mod.rs`: a `DoctorTeamScope` enum
  (`Single(TeamName)` | `AllTeams { resolved_none: bool }`) computed from
  `team_override`, `all_teams`, and the config resolution. The
  roster/presence/mixed-backend/graft-receiver sections run once per team in
  scope. The single-team code path stays byte-for-byte in behaviour so
  existing tests keep passing without edits.
- D3 `crates/atm-core/src/doctor/report.rs`: `member_roster` stays as-is for
  the single-team case; add `team_rosters: Vec<MembersList>`
  (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`) populated only
  in all-teams scope, and a `team_scope` field naming the scope in the JSON.
  Each finding produced inside a team section must carry the team name in its
  message so all-teams output is attributable.
- D4 `crates/atm/src/output.rs`: text rendering prints one roster block per
  team in scope with the team name as the block heading; single-team output
  is unchanged.
- D4a Supporting scope plumbing: `crates/atm/src/commands/members.rs`,
  `crates/atm/src/commands/teams.rs`, and
  `crates/atm-runtime/src/doctor_projection.rs` carry the all-teams query and
  project each scoped roster without re-reading caller state.
- D5 Tests (`crates/atm-core/src/doctor/` unit tests against the existing
  stub roster store, never the live database): single team unchanged;
  `all_teams` with three teams yields three roster sections and presence
  findings attributed per team; no-team-resolved falls back to all-teams
  with the info finding; `--team` + `--all-teams` rejected at the clap layer
  (`crates/atm/src/commands/doctor.rs` unit test via `try_parse_from`).
- D6 Docs: the `atm doctor` entry in `docs/atm-error-codes.md` or wherever
  the doctor CLI options are documented gains the scope table above.

## Acceptance criteria

- AC1 `ATM_TEAM=x atm doctor` and `atm doctor --team x` produce the same
  roster sections as before this sprint (existing tests unchanged and green).
- AC2 `atm doctor --all-teams` reports every roster-store team; findings name
  their team.
- AC3 `atm doctor` with no team resolvable equals `--all-teams` plus the info
  finding; it never silently skips the roster sections.
- AC4 `atm doctor --team x --all-teams` exits 2 with a clap conflict message.
- AC5 `just validate` green; `cargo test -p atm-core -p atm` green; fmt and
  clippy clean before every push.
- AC6 Empty diff under `crates/atm-daemon/` (frozen legacy) and under
  `crates/atm-herdr/`, `boundaries/`, and `crates/atm-http-runtime/`.
- AC7 Presence probing (AYP-R6-002) stays bounded: one `agent get` per
  member per team, same deadline as today; no new Herdr connections outside
  the doctor run.

## Dispatch and PR topology

Not stacked. Branch from `integrate/phase-ay` head; plain `git push`; open
the PR (not draft) against `integrate/phase-ay` on the first push. Sized to
one context window; if D3/D4 output work will not fit, land D1/D2/D5 first,
push, checkpoint to fenix, then continue on the same branch.
