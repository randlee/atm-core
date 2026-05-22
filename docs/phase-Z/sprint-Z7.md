---
id: Z.7
title: Team Admin Roster Authority And Member Metadata
status: planned
branch: feature/pZ-s7-team-admin-roster-authority-and-member-metadata
worktree: ../atm-core-worktrees/feature/pZ-s7-team-admin-roster-authority-and-member-metadata
target: integrate/phase-Z
---

# Sprint Z.7 — Team Admin Roster Authority And Member Metadata

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.7
worktree: ../atm-core-worktrees/feature/pZ-s7-team-admin-roster-authority-and-member-metadata
branch: feature/pZ-s7-team-admin-roster-authority-and-member-metadata
status: planned
estimated_scope: medium
```

## Goal

Make team-admin surfaces operate on ATM roster truth and move retained Claude
member metadata such as `tmux_pane_id` into canonical ATM roster ownership.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- accepted `Z.6` closeout

## Prerequisites

- `Z.6` complete

## Exact Targets

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/schema/agent_member.rs`
- `crates/atm-core/src/boundary/store.rs`

## Deliverables

- `atm members` and `atm teams` report ATM roster truth
- `atm team member add` mutates ATM roster truth first and then projects to
  `config.json`
- `tmux_pane_id` is canonical ATM roster-member metadata rather than a durable
  `.atm.toml` authority
- `docs/phase-Z/readiness.md` updated with accepted `Z.7` verdict and head

## Required Work

- replace team-admin file-first roster views with ATM roster-truth views
- preserve any explicit raw Claude file comparison behavior only as a
  diagnostic surface outside normal members / teams command output
- ensure ATM-owned member-add writes the canonical roster and then projects the
  approved member set into `config.json`
- store justified Claude member-routing metadata such as `tmux_pane_id` in
  canonical ATM roster state and project it back into `config.json`
- stop treating `.atm.toml` as the durable source of per-member pane mapping

## Acceptance Criteria

- `atm members` and `atm teams` report ATM roster truth rather than raw
  `config.json` state
- `atm team member add` persists member additions through canonical ATM roster
  ownership and projects the resulting config
- `tmux_pane_id` survives ATM member add, watcher ingest, and later restore
  through canonical ATM roster ownership
- `.atm.toml` is no longer the durable source of per-member tmux pane metadata

## Non-Closure

- `Z.7` does not own team restore automation
- `Z.7` does not reopen the watcher-ingress ownership decision from `Z.6`

## Production-Ready Expectation

Every listed `Z.7` deliverable is expected to land at a production-ready level
for team-admin roster authority: user-facing team views and member mutation
must reflect the same canonical roster truth the runtime already uses.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "tmux_pane_id" crates/atm-core`
- `docs/phase-Z/readiness.md`
