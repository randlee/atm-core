---
id: AD.22
title: Nudge Routing State Ownership And Dogfood Transition Cleanup
status: planned
branch: feature/pAD-s22-built-in-nudge-hardening-part2
worktree: ../atm-core-worktrees/feature/pAD-s22-built-in-nudge-hardening-part2
target: integrate/phase-AD
---

# Sprint AD.22 — Nudge Routing State Ownership And Dogfood Transition Cleanup

## Goal

- remove stale-by-design tmux pane routing from committed repo config, make the
  SQLite roster the sole live pane-routing authority, and migrate dogfood
  config onto the shipped built-in nudge path or explicit local overrides

## Hard Dependencies

- `AD.21` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `.atm.toml`
- `scripts/atm-nudge.py`
- `scripts/atm-nudge.sh`
- `scripts/test_atm_nudge.py`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-graft/requirements.md`
- `docs/atm-graft/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD22.md`

## Interfaces To Add Or Modify

The accepted pane-routing ownership rule after this sprint is carried by the
existing canonical roster row plus the retained `atm members --json`
projection; this sprint does not introduce a new Rust coordinator struct just
to restate that invariant.

Accepted seam:

- canonical roster/store truth keeps `recipient_pane_id` on `RosterEntry`
- retained local team projection in `crates/atm-core/src/team_admin.rs`
  exposes that same value through `MemberSummary.tmux_pane_id`
- `crates/atm/src/commands/teams.rs` continues to surface the same roster
  projection to CLI JSON consumers such as `scripts/atm-nudge.py`
- compatibility helpers may consume only that canonical roster projection or
  an explicit `--pane`; they may not rediscover live pane ids from committed
  `.atm.toml`

with these invariants:

- live tmux routing uses the roster-backed `recipient_pane_id` only
- committed `.atm.toml` does not carry `[[rmux.windows.panes]].tmux_pane_id`
  as a live-routing source of truth
- `atm teams add-member ... --pane-id` and
  `atm teams update-member ... --pane-id` remain the accepted repair and
  update path for pane routing
- repo-local scripts may exist only as explicit compatibility tools or explicit
  local overrides; they are not the default shipped nudge implementation
- compatibility helpers that survive must resolve pane routing from canonical
  roster state or explicit `--pane` only; they must not rediscover live pane
  ids from committed repo config

## Paths To Delete

- committed `tmux_pane_id` entries in repo-tracked `.atm.toml`
- any doc or dogfood default that still describes `.atm.toml` rmux panes as
  the accepted live nudge-routing authority
- any repo default that still depends on `scripts/atm-nudge.py`,
  `scripts/atm-nudge.sh`, or `scripts/atm-nudge-xml-1.py` for the normal
  installed post-send path

## Deliverables

- repo-tracked `.atm.toml` no longer carries stale live pane ids
- docs state clearly that pane routing is local roster state, not git-tracked
  config
- dogfood config uses the built-in installed nudge path by default or a clearly
  marked explicit local override path
- repo-local nudge scripts are either deleted or marked compatibility-only with
  no ambiguity that they are not the default shipped path
- retained compatibility helpers render the same six accepted XML forms as the
  built-in path when they are used explicitly
- if any residual graft-facing nudge docs remain after `AD.21`, this sprint
  closes them without reopening sink-private receiver behavior

## This Sprint Does Not Close

- new message families beyond the six built-in template kinds from `AD.21`
- any new receiver implementation beyond the existing local tmux and graft
  emitters already accepted in Phase `AD`

## Acceptance Criteria

- repo `.atm.toml` no longer contains `tmux_pane_id` fields
- targeted regression or smoke coverage proves a roster-backed pane id is
  sufficient for local tmux nudge with no `.atm.toml` pane lookup
- docs and dogfood defaults no longer describe or depend on committed pane ids
  for live routing
- any retained repo-local script clearly states compatibility-only or
  override-only status and no accepted doc treats it as the default installed
  path
- targeted regression coverage proves `scripts/atm-nudge.py` resolves panes
  from canonical roster state only unless the operator passes `--pane`
- `atm teams add-member` / `update-member` docs remain the explicit pane repair
  and update workflow

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted roster-backed pane-routing regression coverage
- targeted dogfood/default-config migration verification
- `git diff --check`
