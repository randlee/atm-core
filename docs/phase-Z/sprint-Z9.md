---
id: Z.9
title: Team Admin Roster Authority And Canonical Member Metadata
status: planned
branch: feature/pZ-s9-team-admin-roster-authority-and-member-metadata
worktree: ../atm-core-worktrees/feature/pZ-s9-team-admin-roster-authority-and-member-metadata
target: integrate/phase-Z
---

# Sprint Z.9 — Team Admin Roster Authority And Canonical Member Metadata

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.9
worktree: ../atm-core-worktrees/feature/pZ-s9-team-admin-roster-authority-and-member-metadata
branch: feature/pZ-s9-team-admin-roster-authority-and-member-metadata
status: planned
estimated_scope: medium
```

## Goal

Make `atm members`, `atm teams`, and `atm team member add` operate on canonical
ATM roster truth, and move Claude compatibility metadata such as
`tmux_pane_id` into canonical ATM roster ownership.

## Scope Summary

This sprint owns team-admin views and member mutation, not restore automation.

## Governing Requirements

- `REQ-CORE-TEAM-001`
- `REQ-CORE-CLAUDE-ROSTER-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `ConfigIngress`

## Prerequisites

- `Z.8` complete

## Hard Dependencies

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/schema/agent_member.rs`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete raw `config.json` truth reads from `list_teams` / `list_members`
- delete file-first authority from `atm team member add`
- delete durable `.atm.toml` authority for `tmux_pane_id`

## Non-Goals

- no restore automation
- no release/canary work

## Sub-Tasks

1. Cut over `atm members` / `atm teams`.
   Development work:
   - delete file-truth views in `crates/atm-core/src/team_admin.rs` for:
     - `list_teams`
     - `list_members`
   - make those commands report canonical ATM roster truth
   Required tests:
   - prove members/teams output reflects ATM roster state without requiring
     `config.json` membership reads
   Required docs:
   - update `docs/phase-Z/config-json-violation-inventory.md`

2. Cut over `atm team member add`.
   Development work:
   - delete file-first roster mutation behavior in `team_admin.rs`
   - mutate canonical ATM roster truth first
   - project the approved member set back into `config.json`
   Required tests:
   - prove duplicate detection and team existence validation still work
   - prove ATM roster truth and projected config remain aligned after add
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

3. Canonicalize retained Claude member metadata.
   Development work:
   - move `tmux_pane_id` and any justified surviving Claude routing metadata
     into canonical ATM roster-member state
   - stop treating `.atm.toml` as durable pane-routing authority
   Approved Rust / schema shape:
   ```rust
   #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
   #[serde(rename_all = "camelCase")]
   pub struct AgentMember {
       pub name: AgentName,
       #[serde(default)]
       pub agent_id: String,
       #[serde(default)]
       pub agent_type: AgentType,
       #[serde(default)]
       pub model: String,
       #[serde(default)]
       pub joined_at: Option<u64>,
       #[serde(default)]
       pub tmux_pane_id: Option<String>,
       #[serde(default)]
       pub cwd: String,
       #[serde(flatten)]
       pub extra: Map<String, Value>,
   }
   ```
   Migration contract:
   - this is the existing `AgentMember` record with its current field set
     preserved; `Z.9` does not replace it with a parallel type or rename
     `name`
   - the only schema change this sprint owns is `tmux_pane_id` moving from the
     current default-empty `String` shape to an optional/nullable field with
     backward-compatible serde defaults
   - SQLite adds a nullable `tmux_pane_id` column to the canonical ATM roster
     member table.
   - preexisting rows default to `NULL` until ATM add, watcher ingest, or
     restore supplies a value.
   - `config.json` projection writes the field back out only when
     `tmux_pane_id` is `Some(...)`.
   Required tests:
   - prove `tmux_pane_id` survives ATM add, watcher ingest, and later restore
   - prove a legacy `config.json` `AgentMember` shape still deserializes and
     round-trips when `tmux_pane_id` is omitted or provided as the current
     Claude field
   Required docs:
   - update `docs/atm-core/requirements.md`

4. Update the planning/closure records.
   Development work:
   - stamp `Z.9` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/phase-Z/readiness.md`

## Split Recommendation

If the work starts rebuilding teams from backup material or preserving recreated
`team-lead` / `leadSessionId`, stop and move that scope into `Z.10`.

## Acceptance Criteria

- `atm members` and `atm teams` report ATM roster truth rather than raw Claude
  file state
- `atm team member add` mutates ATM roster truth first and then projects
  `config.json`
- `tmux_pane_id` is canonical ATM roster-member metadata rather than durable
  `.atm.toml` state
- legacy `AgentMember` payloads remain backward-compatible under serde when
  `tmux_pane_id` is omitted or serialized in the current Claude-compatible form

## Non-Closure

- `Z.9` does not rewrite backup/restore flow
- `Z.9` does not begin canary or release-signoff execution

## Production-Ready Expectation

Every listed `Z.9` deliverable is expected to land at a production-ready level
for the team-admin/member-metadata scope this sprint claims: team-admin
commands must use ATM roster truth consistently, and `tmux_pane_id` migration
must preserve backward-compatible serde behavior.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/team_admin.rs`
  - expected: no production matches
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Risks And Watchouts

- do not leave `members`/`teams` half on ATM roster and half on raw file state
- removing `.atm.toml` pane authority must not silently drop `tmux_pane_id`
