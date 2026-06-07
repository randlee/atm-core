---
id: Z.24
title: sc-observability v1.1.0 Retained Log Maintenance Adoption
status: complete
branch: feature/pZ-obs-v1.1.0-log-maintenance
worktree: ../atm-core-worktrees/feature/pZ-obs-v1.1.0-log-maintenance
target: integrate/phase-Z
---

# Sprint Z.24 — sc-observability v1.1.0 Retained Log Maintenance Adoption

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.24
worktree: ../atm-core-worktrees/feature/pZ-obs-v1.1.0-log-maintenance
branch: feature/pZ-obs-v1.1.0-log-maintenance
status: complete
estimated_scope: medium
```

## Goal

- update ATM to `sc-observability` / `sc-observability-types` `1.1.0`
- delete daemon-local retained-log maintenance ownership
- adopt logger-owned `RetainedLogPolicy` and maintenance runtime
- expose retained-log maintenance health through `atm doctor`

## Scope Summary

This sprint moves retained-log rotation, pruning, and bounded maintenance
shutdown out of ATM-local code and onto the shared `sc-observability` runtime.
ATM keeps policy selection, health projection, and doctor/report presentation.

## Governing Requirements

- `REQ-P-DOCTOR-001`
- `REQ-CORE-DOCTOR-001`
- `REQ-CORE-DAEMON-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `ObservabilityPort`
- `DaemonRuntimeObservability`

## Prerequisites

- `Z.23` complete

## Hard Dependencies

- `docs/plans/phase-Z/plan-phase-Z.md`
- `docs/project-plan.md`
- `docs/plans/phase-Z/sprint-Z24.md`

## Exact Targets

- `Cargo.toml`
- `Cargo.lock`
- `crates/atm/Cargo.toml`
- `crates/atm-core/src/observability.rs`
- `crates/atm-daemon/bin_support/daemon_observability.rs`
- `crates/atm/src/main.rs`
- `crates/atm/src/output.rs`
- `docs/plans/phase-Z/plan-phase-Z.md`
- `docs/project-plan.md`

## Deliverables

- `sc-observability` and `sc-observability-types` upgraded to `1.1.0`
- daemon retained-log maintenance owned by `LoggerConfig.retained_log_policy`
- no ATM-local prune worker / rotation / retention implementation remains
- `atm doctor` projects maintenance health with rotated/pruned counts and last
  pass timing

## Required Work

- replace ATM-local retained-log worker ownership with logger-owned maintenance
  runtime
- keep ATM policy values explicit and bounded
- route daemon shutdown through the logger typestate shutdown path
- project maintenance health into ATM observability snapshots and doctor output
- delete the daemon-local retained-log maintenance implementation
- revalidate the updated observability stack with the full smoke lane

## This Sprint Does Not Close

- daemon-side retained query/follow ownership
- broader observability architecture changes unrelated to retained maintenance

## Acceptance Criteria

- no local rotation/prune/maintenance-worker reimplementation remains in
  daemon observability code
- retained-log maintenance is configured through `RetainedLogPolicy`
- maintenance shutdown remains bounded through the logger-owned join-timeout
  contract
- `atm doctor` reports retained-log maintenance counts and last-pass timing
- `cargo test --workspace` passes
- `python3 .just/run_lint.py all` passes
- `just smoke thorough` passes

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `just smoke thorough`
- `git diff --check`

## Split Recommendation

If daemon-side retained query/follow ownership widens beyond health projection
and maintenance adoption, split that into a later follow-on sprint.

## Production-Ready Expectation

The accepted daemon line must no longer own retained-log maintenance logic that
already exists in `sc-observability`; ATM should only own policy values,
health/report projection, and shutdown sequencing.
