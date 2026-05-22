---
id: Z.5
title: Runtime Roster Truth Cutover
status: planned
branch: feature/pZ-s5-runtime-roster-truth-cutover
worktree: ../atm-core-worktrees/feature/pZ-s5-runtime-roster-truth-cutover
target: integrate/phase-Z
---

# Sprint Z.5 — Runtime Roster Truth Cutover

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.5
worktree: ../atm-core-worktrees/feature/pZ-s5-runtime-roster-truth-cutover
branch: feature/pZ-s5-runtime-roster-truth-cutover
status: planned
estimated_scope: medium
```

## Goal

Make canonical ATM roster state the only runtime membership source for retained
commands and introduce the immutable public `ClaudeCodeTeamRoster` view.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- accepted `Z.2` closeout

## Prerequisites

- `Z.2` complete
- `Z1-F001` and `Z1-F002` are closed on the accepted `integrate/phase-Z` line

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
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/doctor/mod.rs`

## Deliverables

- immutable public `ClaudeCodeTeamRoster` view backed by ATM roster truth
- `list`, `read`, `clear`, and `ack` membership validation cut over to ATM
  roster truth only
- `send` no longer uses `config.json` as a pre-write membership gate
- `doctor` remains the explicit config-vs-ATM comparison surface
- `docs/phase-Z/readiness.md` updated with accepted `Z.5` verdict and head

## Required Work

- remove normal runtime roster-truth reads of `config.json` from retained
  command flows
- route retained runtime membership decisions through ATM roster truth and the
  immutable `ClaudeCodeTeamRoster` projection only
- keep `doctor` as a comparison surface that may warn when `config.json` is
  missing ATM roster members or otherwise drifted
- document any remaining justified `config.json` read surface explicitly in the
  updated docs

## Acceptance Criteria

- no normal retained runtime command path reads `config.json` for roster-truth
  membership validation
- `list`, `read`, `clear`, and `ack` use ATM roster truth only
- `send` no longer blocks before durable write based on `config.json`
- `doctor` can still compare `config.json` against ATM roster truth and report
  drift
- `ClaudeCodeTeamRoster` exists as the only approved immutable public runtime
  roster surface

## Non-Closure

- `Z.5` does not implement watcher-owned `config.json` ingest
- `Z.5` does not automate team-admin or restore workflows
- `Z.5` does not move `tmux_pane_id` ownership yet

## Production-Ready Expectation

Every listed `Z.5` deliverable is expected to land at a production-ready level
for runtime roster-truth ownership: retained commands must be able to run on
ATM roster truth alone without hidden `config.json` runtime dependencies.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/send/mod.rs crates/atm-core/src/list.rs crates/atm-core/src/read/mod.rs crates/atm-core/src/clear/mod.rs crates/atm-core/src/ack/mod.rs`
  - expected: no retained-command production matches
- `docs/phase-Z/readiness.md`
