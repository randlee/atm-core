---
id: Z.8
title: Team Backup Restore Automation And Config Projection
status: planned
branch: feature/pZ-s8-team-backup-restore-automation-and-config-projection
worktree: ../atm-core-worktrees/feature/pZ-s8-team-backup-restore-automation-and-config-projection
target: integrate/phase-Z
---

# Sprint Z.8 — Team Backup Restore Automation And Config Projection

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.8
worktree: ../atm-core-worktrees/feature/pZ-s8-team-backup-restore-automation-and-config-projection
branch: feature/pZ-s8-team-backup-restore-automation-and-config-projection
status: planned
estimated_scope: large
```

## Goal

Automate team backup / restore around ATM roster truth so backup preserves raw
files but restore rebuilds the recreated Claude team config from canonical ATM
state.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- accepted `Z.7` closeout

## Prerequisites

- `Z.7` complete
- restored-team operator flow is still the current `team-lead` manual
  procedure anchored on backup, `TeamDelete`, `TeamCreate`, and then
  `atm teams restore`

## Exact Targets

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/team_admin/restore.rs`

## Deliverables

- backup remains a raw snapshot / audit surface for Claude files, inboxes,
  tasks, and ATM durable state
- restore uses canonical ATM roster truth to overwrite the recreated team's
  `config.json`
- restore preserves the current recreated `team-lead` entry and current
  `leadSessionId`
- restore preserves canonical member metadata such as `tmux_pane_id`
- `docs/phase-Z/readiness.md` updated with accepted `Z.8` verdict and head

## Required Work

- keep raw backup capture behavior for audit / inspection value
- explicitly stop treating backup `config.json` as restore authority
- preserve the operator precondition that Claude `TeamCreate` recreates the new
  team shell before `atm teams restore` projects ATM state back into
  `config.json`
- restore non-lead members, approved durable team state, inboxes, and tasks in
  one synchronized ATM-owned flow
- preserve current recreated `team-lead` and current `leadSessionId` instead
  of replaying stale lead state from backup
- recompute task high-water state and preserve deterministic restore behavior

## Acceptance Criteria

- backup still preserves raw Claude files and ATM-owned durable state for audit
  use
- restore does not use backup `config.json` as roster truth
- after `TeamCreate`, `atm teams restore` overwrites `config.json` from
  canonical ATM roster truth
- restore preserves current recreated `team-lead`, current `leadSessionId`,
  and canonical member metadata such as `tmux_pane_id`
- the automated restore path replaces the current manual file-edit steps as the
  authoritative ATM recovery flow
- `Z.3` canary and dogfood remains blocked until `Z.8` closes

## Non-Closure

- `Z.8` does not run canary or release sign-off work
- `Z.8` does not reopen `Z.1` / `Z.2` smoke findings outside restore-specific
  fallout discovered during implementation

## Production-Ready Expectation

Every listed `Z.8` deliverable is expected to land at a production-ready level
for team recovery: the product-owned restore flow must replace manual config
replay with deterministic ATM-owned projection.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `docs/phase-Z/readiness.md`
