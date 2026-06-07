---
id: Z.8
title: Watcher-Owned Claude Config Ingest
status: complete
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
status: complete
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

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `docs/plans/phase-Z/config-json-violation-inventory.md`
- `docs/plans/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-core/src/config/mod.rs`
- `docs/plans/phase-Z/config-json-violation-inventory.md`
- `docs/plans/phase-Z/claude-roster-sync-and-restore.md`
- `docs/plans/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete any remaining non-watcher external `config.json` ingest path,
  including
  `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` in
  `crates/atm-core/src/boundary_support.rs` and its forwarding in
  `crates/atm-core/src/direct_boundaries.rs`
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
   - delete
     `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` from
     `crates/atm-core/src/boundary_support.rs` and delete its forwarding from
     `crates/atm-core/src/direct_boundaries.rs` once watcher/reconcile owns the
     first-team ingest path
   Required tests:
   - prove new-team ingest hydrates ATM roster truth before later runtime
     consumers depend on it
   - prove the startup-only bootstrap helper no longer exists on the production
     call surface after `Z.8` closes
   Required docs:
   - update `docs/plans/phase-Z/config-json-violation-inventory.md`

2. Add daemon-owned write suppression.
   Development work:
   - ensure ATM-owned config projection does not re-trigger watcher import
     loops
   - implement suppression as a daemon-owned in-memory projection-write journal
     keyed by canonical `config.json` path plus the write's content digest or
     projection epoch; the watcher consumes one matching journal entry and
     suppresses only that matching event
   - the suppression journal is intentionally process-local and does not
     survive daemon restart
   - if the daemon crashes mid-write, no durable suppression state is kept; a
     later watcher event after restart is treated as external input and flows
     through the same idempotent watcher / reconcile ingest path
   Required tests:
   - prove daemon-authored projection writes do not self-replay as external
     ingest
   - prove the projection-write journal suppresses the matching write event
     once and only once
   - prove restart/crash behavior is correct:
     restart clears suppression state, and a post-crash event is handled as an
     ordinary external ingest candidate
   Required docs:
   - update `docs/plans/phase-Z/claude-roster-sync-and-restore.md`

3. Close the ingest ownership records.
   Development work:
   - stamp `Z.8` accepted head and verdict in `docs/plans/phase-Z/readiness.md`
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
- `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` and its
  forwarding are deleted once watcher/reconcile owns first-team ingest
- daemon-owned projection writes do not re-trigger watcher import loops
- daemon-write suppression is explicit, process-local, and restart-safe:
  a matching projection event is suppressed once, restart drops suppression
  state, and crash recovery falls back to idempotent external ingest

## Non-Closure

- `Z.8` does not move `atm members`, `atm teams`, `add-member`, or restore
  ownership
- `Z.8` does not begin canary or release-signoff execution

## Production-Ready Expectation

Every listed `Z.8` deliverable is expected to land at a production-ready level
for the watcher-ingest scope this sprint claims: watcher/reconcile must become
the only external `config.json` reader, and daemon-write suppression must be
explicit, bounded, and restart-safe.

## Required Validation

- `cargo test --workspace`
- `cargo test --workspace z8_projection_write_suppression_is_process_local -- --nocapture`
  - expected: matching projection write event is suppressed once; restart or
    crash leaves no durable suppression residue
- `cargo test --workspace z8_deletes_startup_only_config_bootstrap_helper -- --nocapture`
  - expected: watcher/reconcile first-team ingest path passes without any
    surviving call surface for
    `hydrate_roster_from_team_config_once_at_startup_if_empty(...)`
- `git diff --check`
- `rg -n "load_claude_team_config_document\\(" crates/atm-core/src crates/atm-daemon/src`
  - expected: surviving production matches are restricted to
    `crates/atm-daemon/src/reconcile_runtime.rs`,
    `crates/atm-daemon/src/projection_write_journal.rs`,
    `crates/atm-core/src/service_runtime.rs`,
    `crates/atm-core/src/team_admin.rs`,
    and `crates/atm-core/src/team_admin/restore.rs`; any surviving match for
    `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` in
    `boundary_support.rs` or `direct_boundaries.rs` is a failure
- `docs/plans/phase-Z/readiness.md`

## Required Document Updates

- `docs/plans/phase-Z/claude-roster-sync-and-restore.md`
- `docs/plans/phase-Z/config-json-violation-inventory.md`
- `docs/plans/phase-Z/readiness.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-core/requirements.md`

## Risks And Watchouts

- watcher ingest must not become a second mutation path with different roster
  rules from `atm team member add`
- daemon-write suppression has to be explicit; otherwise the import loop will
  reappear under a different name
