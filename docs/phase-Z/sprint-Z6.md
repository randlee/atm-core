---
id: Z.6
title: Claude Send Semantics And Immutable Runtime Roster View
status: planned
branch: feature/pZ-s6-claude-send-semantics-and-runtime-roster-view
worktree: ../atm-core-worktrees/feature/pZ-s6-claude-send-semantics-and-runtime-roster-view
target: integrate/phase-Z
---

# Sprint Z.6 — Claude Send Semantics And Immutable Runtime Roster View

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.6
worktree: ../atm-core-worktrees/feature/pZ-s6-claude-send-semantics-and-runtime-roster-view
branch: feature/pZ-s6-claude-send-semantics-and-runtime-roster-view
status: planned
estimated_scope: medium
```

## Goal

Delete the Claude send pre-write `config.json` gate, introduce immutable
`ClaudeCodeTeamRoster`, and remove the generic runtime helper seam that made
send/file-based validation easy to keep around.

## Scope Summary

This sprint owns send semantics and the public immutable runtime roster view.
It does not yet narrow `ConfigIngress` or implement watcher/reconcile ingest.

## Governing Requirements

- `REQ-CORE-CLAUDE-ROSTER-001`
- `REQ-CORE-CLAUDE-ROSTER-002`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `ConfigIngress`

## Prerequisites

- `Z.5` complete

## Hard Dependencies

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/boundary/store.rs`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete pre-write `config.json` membership gating from
  `crates/atm-core/src/send/mod.rs`
- delete or narrow generic runtime `load_team_config(...)` helper use from
  `crates/atm-core/src/service_runtime.rs`
- add immutable `ClaudeCodeTeamRoster` as the approved runtime roster view

## Non-Goals

- no watcher-owned external ingest yet
- no team-admin cutover
- no restore automation

## Sub-Tasks

1. Replace the Claude send pre-write config gate.
   Development work:
   - delete `runtime.load_team_config(...)` roster validation from
     `crates/atm-core/src/send/mod.rs`
   - preserve the accepted sequence:
     - durable ATM write first
     - attempt Claude inbox write when the inbox exists
     - only then compare against the immutable `ClaudeCodeTeamRoster`
       projection populated from approved Claude config-ingress state
     - missing Claude roster membership becomes a warning, not a veto
   Required tests:
   - prove send no longer blocks before the SQLite write
   - prove inbox write is still attempted when the inbox exists
   - prove the warning text is returned after the write path when the Claude
     member is absent from the immutable Claude roster projection, while the
     warning text still names the underlying `config.json` roster mismatch
   Required docs:
   - update `docs/phase-Z/config-json-violation-inventory.md`

2. Introduce immutable runtime roster projection.
   Development work:
   - add the immutable public `ClaudeCodeTeamRoster` surface
   - use it as the approved runtime roster projection instead of generic
     `config.json` reads
   Approved Rust shape:
   ```rust
   #[derive(Clone, Debug, Eq, PartialEq)]
   pub struct ClaudeCodeTeamRoster {
       team_name: TeamName,
       members: Arc<[ClaudeCodeRosterMember]>,
   }

   #[derive(Clone, Debug, Eq, PartialEq)]
   pub struct ClaudeCodeRosterMember {
       member_name: AgentName,
       harness: DeliveryHarness,
       inbox_path: Option<PathBuf>,
       tmux_pane_id: Option<String>,
   }

   impl ClaudeCodeTeamRoster {
       pub fn from_roster_snapshot(
           team_name: TeamName,
           records: &[RosterMemberRecord],
       ) -> Self
   }
   ```
   Contract notes:
   - `ClaudeCodeTeamRoster` is `pub` and immutable by construction.
   - `Arc<[ClaudeCodeRosterMember]>` keeps member order frozen for the lifetime
     of the snapshot and preserves `Send + Sync` semantics by construction.
   - `RosterMemberRecord.recipient_pane_id` maps one-to-one into
     `ClaudeCodeRosterMember.tmux_pane_id`; the field rename is intentional and
     the approved public Claude-facing name in this sprint is `tmux_pane_id`.
   - `from_roster_snapshot(...)` is the only approved builder in this sprint;
     runtime consumers do not build ad hoc file-backed projections.
   Required tests:
   - cover runtime consumers that need immutable roster inspection
   - prove the post-write Claude warning path consumes
     `ClaudeCodeTeamRoster`, not a direct send-path file read
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

3. Remove the generic runtime helper seam.
   Development work:
   - delete or narrow `load_team_config(...)` from
     `crates/atm-core/src/service_runtime.rs`
   Required tests:
   - prove retained command/runtime paths no longer rely on the generic helper
   Required docs:
   - update `docs/atm-core/architecture.md`

## Split Recommendation

If the work requires `ConfigIngress` trait redesign, boundary adapter changes,
or watcher/reconcile import ownership, stop and move that scope into `Z.7` or
`Z.8`.

## Acceptance Criteria

- `send` no longer uses `config.json` as a pre-write membership gate
- `ClaudeCodeTeamRoster` exists as the approved immutable runtime roster view
- generic runtime `load_team_config(...)` helper use is removed from
  `send`-driven command/runtime behavior

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/send/mod.rs crates/atm-core/src/service_runtime.rs`
  - expected: no production matches
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`

## Risks And Watchouts

- do not let send warning behavior regress into a hidden pre-write veto
- `ClaudeCodeTeamRoster` must be immutable public surface, not a second mutable
  source of truth
- deleting the helper from `service_runtime.rs` is intentional; leaving it in
  place invites future boundary regressions
