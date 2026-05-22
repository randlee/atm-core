---
id: Z.6
title: Watcher-Owned Claude Config Ingress
status: planned
branch: feature/pZ-s6-watcher-owned-claude-config-ingress
worktree: ../atm-core-worktrees/feature/pZ-s6-watcher-owned-claude-config-ingress
target: integrate/phase-Z
---

# Sprint Z.6 — Watcher-Owned Claude Config Ingress

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.6
worktree: ../atm-core-worktrees/feature/pZ-s6-watcher-owned-claude-config-ingress
branch: feature/pZ-s6-watcher-owned-claude-config-ingress
status: planned
estimated_scope: medium
```

## Goal

Make the watcher / reconcile lane the only roster-truth reader of
`config.json` and implement the accepted Claude send warning semantics.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- accepted `Z.5` closeout

## Prerequisites

- `Z.5` complete

## Exact Targets

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`
- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-core/src/send/mod.rs`

## Deliverables

- watcher-owned `config.json` ingress path for canonical ATM roster import
- daemon-authored write suppression so ATM-owned config projection does not
  re-trigger import loops
- new-team and external-config-change import path into ATM roster truth
- accepted Claude send mismatch warning after durable write / inbox write path
- `docs/phase-Z/readiness.md` updated with accepted `Z.6` verdict and head

## Required Work

- narrow `ConfigIngress` so it no longer behaves like a generic runtime roster
  lookup helper
- ensure watcher / reconcile is the only roster-truth reader of external
  `config.json` changes
- when a `config.json` change is observed and was not caused by an ATM-owned
  write, route the result through the equivalent of ATM member-add / roster
  import logic
- on the Claude send path, preserve the accepted behavior:
  - durable ATM write happens first
  - if the target inbox exists, inbox write is still attempted
  - after that path is selected, ATM may compare the member against
    `config.json`
  - missing config membership becomes a warning, not a veto

## Acceptance Criteria

- watcher / reconcile is the only production roster-truth reader of
  `config.json`
- generic runtime `load_team_config(...)` roster lookup behavior is removed or
  narrowed to watcher-owned import only
- external `config.json` changes import canonical ATM roster updates through
  the watcher / reconcile lane
- Claude send mismatch against `config.json` is returned as a warning after the
  durable write path and does not veto inbox write when the inbox exists

## Non-Closure

- `Z.6` does not convert team-admin surfaces to ATM-roster authority
- `Z.6` does not complete backup / restore automation

## Production-Ready Expectation

Every listed `Z.6` deliverable is expected to land at a production-ready level
for config-ingress ownership: one reader, one watcher-owned import path, and
one accepted send-warning contract.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/boundary_support.rs crates/atm-core/src/direct_boundaries.rs crates/atm-daemon/src/boundary_adapters.rs crates/atm-daemon/src/direct_boundaries.rs`
  - expected: surviving matches are watcher-owned import seams only
- `docs/phase-Z/readiness.md`
