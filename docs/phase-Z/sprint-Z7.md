---
id: Z.7
title: Config Ingress Boundary Narrowing And Static Gates
status: planned
branch: feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
worktree: ../atm-core-worktrees/feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
target: integrate/phase-Z
---

# Sprint Z.7 — Config Ingress Boundary Narrowing And Static Gates

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.7
worktree: ../atm-core-worktrees/feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
branch: feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
status: planned
estimated_scope: medium
```

## Goal

Narrow `ConfigIngress` so it cannot remain a generic runtime roster lookup
boundary, and define the repository-local lint / `sc-lint`-candidate gates that
mechanically guard the approved caller set.

## Scope Summary

This sprint owns the boundary/helper contract cleanup and static gate
definition. It does not yet implement watcher/reconcile ingest behavior.

## Governing Requirements

- `REQ-CORE-CLAUDE-ROSTER-001`
- `REQ-CORE-CLAUDE-ROSTER-002`
- `REQ-P-LINT-POSTMORTEM-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `ConfigIngress`
- `DaemonConfigIngressAdapter`

## Prerequisites

- `Z.6` complete

## Hard Dependencies

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete generic retained-command/runtime `load_team_config(...)` boundary
  behavior from:
  - `crates/atm-core/src/boundary_support.rs`
  - `crates/atm-core/src/direct_boundaries.rs`
  - `crates/atm-daemon/src/boundary_adapters.rs`
  - `crates/atm-daemon/src/direct_boundaries.rs`
- narrow `ConfigIngress` in `crates/atm-core/src/boundary/store.rs` to the
  approved caller set
- define `SCB-CONFIG-001`, `SCB-CONFIG-002`, and `SCB-CONFIG-003`

## Non-Goals

- no watcher/reconcile roster import logic yet
- no team-admin cutover
- no restore automation

## Sub-Tasks

1. Narrow the `ConfigIngress` contract.
   Development work:
   - remove generic roster-lookup semantics from:
     - `crates/atm-core/src/boundary/store.rs`
     - `crates/atm-core/src/boundary_support.rs`
     - `crates/atm-core/src/direct_boundaries.rs`
     - `crates/atm-daemon/src/boundary_adapters.rs`
     - `crates/atm-daemon/src/direct_boundaries.rs`
   - leave only the approved ingress/comparison/preservation behavior
   Required tests:
   - prove generic retained command/runtime lookup no longer compiles or no
     longer exists on the boundary surface
   Required docs:
   - update `docs/atm-core/boundaries.md`
   - update `docs/atm-daemon/boundaries.md`

2. Define static gates for future regressions.
   Development work:
   - define repository-local lint / `sc-lint`-candidate rules:
     - `SCB-CONFIG-001`
     - `SCB-CONFIG-002`
     - `SCB-CONFIG-003`
   - implement the rule family in `.just/lint_boundaries.py` as deterministic
     repository-local token / regex scans rather than prose-only review notes
   - define the explicit allowlist in a checked-in machine-runnable file,
     `.just/allowlists/scb_config_allowlist.toml`, with one entry per approved
     survivor and fields at minimum:
     - `rule`
     - `path`
     - `symbol`
     - `why`
     - `sunset_sprint`
   - add a checked-in known-bad fixture, for example
     `.just/fixtures/scb_config_known_bad.rs`, that intentionally violates all
     three `SCB-CONFIG-*` rules
   Required tests:
   - add a machine-runnable boundary-lint fixture that contains one known-bad
     direct `config.json` roster lookup and proves the rule family rejects it
   - wire that fixture into `just lint boundaries` through
     `.just/lint_boundaries.py` so the lint performs a fixture self-test:
     the known-bad fixture must be rejected internally with rule ids and
     `path:line` output, or the top-level lint fails
   - define the reject signal explicitly:
     - real repo violation: `just lint boundaries` exits non-zero and prints
       `SCB-CONFIG-00X <path>:<line> <summary>`
     - fixture self-test failure: `just lint boundaries` exits non-zero and
       prints that the known-bad fixture was not rejected as expected
   Required docs:
   - update `docs/requirements.md`
   - update `docs/architecture.md`

3. Sync the planning records.
   Development work:
   - stamp `Z.7` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

## Split Recommendation

If the work starts implementing watcher/reconcile import behavior, daemon-write
suppression, or external roster change ingestion, stop and move that scope into
`Z.8`.

## Acceptance Criteria

- `ConfigIngress` is no longer documented or shaped as a generic runtime roster
  lookup boundary
- the helper/adapter chain no longer exposes generic retained command/runtime
  `load_team_config(...)` behavior
- repo-local lint / `sc-lint`-candidate rule definitions exist for the
  `config.json` boundary violation family and produce a verifiable reject
  signal on a known-bad fixture
- the machine-runnable allowlist file and known-bad fixture are checked in and
  are the authoritative inputs to `just lint boundaries`

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `just lint boundaries`
  - expected on a clean repo: exit `0` after the fixture self-test proves the
    known-bad file is rejected internally
  - expected on a real boundary violation or fixture false-negative: exit
    non-zero and print `SCB-CONFIG-00X <path>:<line> <summary>`
- `rg -n "load_team_config\\(" crates/atm-core/src/boundary_support.rs crates/atm-core/src/direct_boundaries.rs crates/atm-daemon/src/boundary_adapters.rs crates/atm-daemon/src/direct_boundaries.rs`
  - expected: surviving matches are explicitly ingress/comparison/preservation
    only
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Risks And Watchouts

- this sprint is not allowed to hide the boundary leak behind new naming while
  keeping the same generic behavior
- the allowlist must stay explicit and narrow
- if the static gates cannot be stated crisply here, later implementation will
  drift back toward ad hoc exceptions
