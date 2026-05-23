---
id: Z.5
title: Runtime Roster Truth Cutover
status: complete
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
status: complete
estimated_scope: medium
```

## Goal

Delete the retained runtime `config.json` roster-truth reads in `list`, `read`,
`clear`, and `ack`, while preserving `doctor` as the explicit config-vs-ATM
comparison surface.

## Scope Summary

This sprint owns the retained command cutover only. It does not yet change
Claude send semantics, boundary helper shape, watcher ingest, team-admin, or
restore.

## Governing Requirements

- `REQ-CORE-CLAUDE-ROSTER-001`
- `REQ-CORE-CLAUDE-ROSTER-002`
- `REQ-CORE-QA-RUNTIME-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `ConfigIngress`

## Prerequisites

- `Z.2` complete

## Hard Dependencies

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete `runtime.load_team_config(...)` membership validation from:
  - `crates/atm-core/src/list.rs`
  - `crates/atm-core/src/read/mod.rs`
  - `crates/atm-core/src/clear/mod.rs`
  - `crates/atm-core/src/ack/mod.rs`
- keep `crates/atm-core/src/doctor/mod.rs` only as a compare-only reader

## Non-Goals

- no Claude send warning-semantics work
- no `ConfigIngress` trait narrowing
- no watcher/reconcile ingest work
- no team-admin or restore work

## Sub-Tasks

1. Cut over retained query/mutation commands to ATM roster truth only.
   Development work:
   - delete `runtime.load_team_config(...)` membership validation from:
     - `crates/atm-core/src/list.rs`
     - `crates/atm-core/src/read/mod.rs`
     - `crates/atm-core/src/clear/mod.rs`
     - `crates/atm-core/src/ack/mod.rs`
   - route those membership decisions through ATM roster truth only
   Required tests:
   - command-path tests that prove valid ATM roster members still pass
   - command-path tests that prove missing ATM roster members fail without any
     `config.json` dependency
   Required docs:
   - update `docs/phase-Z/config-json-violation-inventory.md`

2. Narrow `doctor` to an explicit comparison-only role.
   Development work:
   - keep `doctor` file reads only for config-vs-ATM drift reporting
   - make sure `doctor` does not reopen a generic runtime roster lookup seam
   Required tests:
   - drift-report coverage when Claude roster differs from ATM roster truth
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

3. Update the planning/closure records.
   Development work:
   - stamp `Z.5` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md` only if execution sequencing changes

## Split Recommendation

If the work requires Claude send post-write warning behavior, `ConfigIngress`
trait changes, or watcher/reconcile import logic, stop and move that scope into
`Z.6`, `Z.7`, or `Z.8` instead of widening `Z.5`.

## Acceptance Criteria

- `list`, `read`, `clear`, and `ack` no longer read `config.json` for roster
  truth
- `doctor` is the only retained runtime surface in this sprint still allowed to
  compare against `config.json`
- the path inventory row for each deleted runtime read is marked closed in
  `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md` is stamped with the `Z.5` accepted head and
  closure verdict

## Non-Closure

- `Z.5` does not change Claude send warning semantics
- `Z.5` does not narrow `ConfigIngress` or introduce watcher/reconcile ingest
- `Z.5` does not change team-admin or restore ownership

## Production-Ready Expectation

Every listed `Z.5` deliverable is expected to land at a production-ready level
for the retained-command cutover scope this sprint claims: retained runtime
membership decisions must depend on ATM roster truth only, while `doctor`
remains a deliberate comparison surface rather than a generic lookup seam.

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/list.rs crates/atm-core/src/read/mod.rs crates/atm-core/src/clear/mod.rs crates/atm-core/src/ack/mod.rs`
  - expected: no production matches
- `rg -n "load_team_config\\(" crates/atm-core/src/doctor/mod.rs`
  - expected: any surviving match is comparison-only drift reporting; zero
    production membership-lookup matches are allowed
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Risks And Watchouts

- do not silently leave `ack` split between ATM roster truth and file-backed
  reply-team validation
- `doctor` is allowed to survive, but only as a comparison surface
- if command tests still need `load_team_config(...)`, that is a sign the
  runtime helper cleanup in `Z.6` needs to happen immediately after this sprint
