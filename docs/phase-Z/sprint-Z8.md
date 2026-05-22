---
id: Z.8
title: Watcher-Owned Claude Config Ingest
status: planned
branch: feature/pZ-s8-watcher-owned-claude-config-ingest
worktree: ../atm-core-worktrees/feature/pZ-s8-watcher-owned-claude-config-ingest
target: integrate/phase-Z
---

# Sprint Z.8 — Watcher-Owned Claude Config Ingest

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.8
worktree: ../atm-core-worktrees/feature/pZ-s8-watcher-owned-claude-config-ingest
branch: feature/pZ-s8-watcher-owned-claude-config-ingest
status: planned
estimated_scope: medium
```

## Goal

Make watcher / reconcile the only production reader of external Claude
`config.json` roster changes and import those changes into canonical ATM roster
truth.

## Scope Summary

This sprint owns new-team ingest, external edit ingest, and daemon-write
suppression. It assumes the `ConfigIngress` contract is already narrowed by
`Z.7`.

## Governing Requirements

- `REQ-CORE-CLAUDE-ROSTER-001`
- `REQ-CORE-CLAUDE-ROSTER-002`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `ConfigIngress`
- `WatchEventSource`
- `ReconcileCoordinator`

## Prerequisites

- `Z.7` complete

## Hard Dependencies

- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-core/src/config/mod.rs`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete any remaining non-watcher external `config.json` ingest path
- narrow parser use in `crates/atm-core/src/config/mod.rs` to approved callers
- add daemon-write suppression so projection writes do not re-trigger import

## Non-Goals

- no team-admin cutover
- no restore automation

## Sub-Tasks

1. Implement watcher-owned external roster ingest.
   Development work:
   - make watcher/reconcile the only production reader of external Claude
     roster changes
   - handle first-team ingest and later external config edits through canonical
     ATM roster update logic
   Required tests:
   - prove new-team ingest hydrates ATM roster truth before later runtime
     consumers depend on it
   Required docs:
   - update `docs/phase-Z/config-json-violation-inventory.md`

2. Add daemon-owned write suppression.
   Development work:
   - ensure ATM-owned config projection does not re-trigger watcher import
     loops
   Required tests:
   - prove daemon-authored projection writes do not self-replay as external
     ingest
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

3. Close the ingest ownership records.
   Development work:
   - stamp `Z.8` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/atm-daemon/boundaries.md`

## Split Recommendation

If the work begins moving `atm members`, `atm teams`, `add-member`, or restore
ownership, stop and move that scope into `Z.9` or `Z.10`.

## Acceptance Criteria

- watcher / reconcile is the only production reader of external Claude
  `config.json` roster changes
- new-team ingest and external config changes update canonical ATM roster truth
- daemon-owned projection writes do not re-trigger watcher import loops

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src crates/atm-daemon/src`
  - expected: surviving production matches are restricted to
    `crates/atm-daemon/src/watch_runtime.rs`,
    `crates/atm-daemon/src/reconcile_runtime.rs`,
    `crates/atm-core/src/doctor/mod.rs`, and one explicitly named
    projection-only helper if this sprint still requires it; any other match is
    a failure
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-core/requirements.md`

## Risks And Watchouts

- watcher ingest must not become a second mutation path with different roster
  rules from `atm team member add`
- daemon-write suppression has to be explicit; otherwise the import loop will
  reappear under a different name
