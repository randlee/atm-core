---
id: Z.10
title: Team Backup Restore Automation And Config Projection
status: complete
branch: feature/pZ-s10-team-backup-restore-automation-and-config-projection
worktree: ../atm-core-worktrees/feature/pZ-s10-team-backup-restore-automation-and-config-projection
target: integrate/phase-Z
---

# Sprint Z.10 — Team Backup Restore Automation And Config Projection

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.10
worktree: ../atm-core-worktrees/feature/pZ-s10-team-backup-restore-automation-and-config-projection
branch: feature/pZ-s10-team-backup-restore-automation-and-config-projection
status: complete
estimated_scope: medium
```

## Goal

Automate backup/restore around ATM roster truth so backup remains an audit
snapshot, while restore rebuilds the recreated Claude team config from
canonical ATM state.

## Scope Summary

This sprint owns the final team recovery rewrite before `Z.3` canary begins.

## Governing Requirements

- `REQ-CORE-TEAM-001`
- `REQ-CORE-CLAUDE-ROSTER-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `ConfigIngress`

## Prerequisites

- `Z.9` complete

## Hard Dependencies

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- current manual restore procedure received from `team-lead`

## Exact Targets

- `crates/atm-core/src/team_admin/restore.rs`
- `crates/atm-core/src/team_admin.rs`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete backup `config.json` roster-truth replay from
  `crates/atm-core/src/team_admin/restore.rs`
- delete generic `load_team_config(...)` use from
  `crates/atm-core/src/team_admin/restore.rs`
- if recreated-shell preservation is still needed for current `team-lead` /
  `leadSessionId`, keep it behind a dedicated narrow preservation helper that
  does not expose generic roster-truth loading

## Non-Goals

- no canary/dogfood execution yet
- no release sign-off work

## Sub-Tasks

1. Keep backup as audit snapshot, not roster truth.
   Development work:
   - preserve backup capture of raw Claude files, inboxes, tasks, and ATM-owned
     durable state
   - write an ATM roster audit snapshot for the backed-up team
   - explicitly stop treating backup `config.json` as restore authority
   Required tests:
   - prove backup still captures the required artifacts
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

2. Rewrite restore around ATM projection.
   Development work:
   - delete backup-config-driven roster replay from
     `crates/atm-core/src/team_admin/restore.rs`
   - require Claude `TeamCreate` shell recreation first
   - overwrite recreated `config.json` from canonical ATM roster truth
   Required tests:
   - prove restore no longer reads backup `config.json` as roster truth
   - prove recreated `config.json` is rebuilt from ATM state
   Required docs:
   - update `docs/phase-Z/config-json-violation-inventory.md`

3. Preserve recreated lead shell identity and metadata.
   Development work:
   - preserve current recreated `team-lead`
   - preserve current recreated `leadSessionId`
   - preserve canonical member metadata such as `tmux_pane_id`
   - keep deterministic task/inbox recovery behavior
   - if recreated-shell preservation still requires reading the freshly
     recreated Claude team shell, perform that through a dedicated helper that
     returns only the approved lead-shell fields, not through
     `load_team_config(...)`
   Required tests:
   - prove recreated lead shell values survive restore
   - prove non-lead membership/inboxes/tasks restore without manual file edits
   Required docs:
   - update `docs/atm-core/requirements.md`

4. Update the planning/closure records.
   Development work:
   - stamp `Z.10` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/phase-Z/readiness.md`

## Split Recommendation

If the work starts re-opening retained runtime command reads or watcher-ingest
ownership, stop and push that scope back into `Z.5` through `Z.9`.

## Acceptance Criteria

- backup preserves raw Claude files and ATM-owned durable state for audit use
- backup writes `atm-roster.json` as an audit-only canonical ATM roster snapshot
- restore does not read backup `config.json` as roster truth
- after Claude `TeamCreate`, `atm teams restore` overwrites `config.json` from
  canonical ATM roster truth
- restore preserves recreated `team-lead`, recreated `leadSessionId`, and
  canonical member metadata such as `tmux_pane_id`
- the automated restore path replaces manual file-edit steps before `Z.3`
  begins
- `docs/phase-Z/readiness.md` is stamped with the `Z.10` accepted head and
  closure verdict

## Non-Closure

- `Z.10` does not begin canary/dogfood execution
- `Z.10` does not perform release sign-off work

## Production-Ready Expectation

Every listed `Z.10` deliverable is expected to land at a production-ready
level for the backup/restore scope this sprint claims: restore must rebuild the
Claude team shell from ATM roster truth deterministically, without falling back
to backup `config.json` authority or manual file surgery.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/team_admin/restore.rs`
  - expected: no production matches; if any surviving read is still narrowly
    justified, the exact call site must be named explicitly in the sprint-close
    evidence
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/atm-core/requirements.md`
- `docs/project-plan.md`

## Risks And Watchouts

- restore must preserve the recreated lead shell instead of replaying stale
  lead identity from backup
- raw backup preservation still matters for audit/emergency inspection, so do
  not delete that capture value while removing backup-config authority
