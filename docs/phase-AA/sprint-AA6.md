# AA.6 `sc-observability` 1.2.0 Upgrade

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.6
worktree: ../atm-core-worktrees/feature/pAA-s6-obs-upgrade
branch: feature/pAA-s6-obs-upgrade
status: planned
estimated_scope: medium
```

## Goal

Upgrade ATM from `sc-observability` / `sc-observability-types` `1.1.0` to
`1.2.0` and absorb the public logging/runtime API changes without regressing
CLI or daemon observability behavior.

## Scope Summary

This sprint is an explicit dependency-upgrade line after the daemon/SQLite
boundary repair. It is not a general observability redesign. The work is to
migrate ATM to the `1.2.0` public surface that now uses queue-backed logger
admission, stopped-logger typestate, and the renamed retained-log shutdown
policy.

The `1.2.0` migration surface is frozen now:
- `Logger::emit()` is deprecated; ATM must migrate to `Logger::log()` or
  `Logger::try_log()` at the concrete `sc-observability` adapter layer rather
  than carrying the deprecated compatibility path forward indefinitely
- queue admission is no longer sink durability; ATM must make
  `flush()` / `shutdown()` the explicit durability/lifecycle barriers where
  it currently relies on immediate retained-log visibility
- the queue-backed APIs use `LogError` / `TryLogError` rather than the older
  synchronous `emit()` error surface; ATM must update its `AtmError`
  observability mapping accordingly
- logger shutdown returns `Logger<Stopped>`; daemon retained-log shutdown
  wrappers must preserve stopped-logger health inspection across the new
  typestate API
- `LoggingHealthReport` now exposes queue/writer/maintenance state; ATM doctor
  and observability presentation must project the additional health data
- `RetainedLogPolicy` now uses typed policy fields and renames the shutdown
  timeout field to `writer_shutdown_timeout`; ATM code still carrying
  `maintenance_join_timeout` must be migrated

## Governing Requirements

- `REQ-P-OBS-001`
- `REQ-P-OBS-004`
- `REQ-ATM-OBS-001`
- `REQ-CORE-OBS-001`
- `REQ-CORE-DOCTOR-001`
- `REQ-DAEMON-OBS-002`
- `REQ-DAEMON-OBS-003`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`

## Governing Boundaries

- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-runtime/boundaries.md`

## Prerequisites

- `AA.5`

## Hard Dependencies

- the `AA.5` boundary relock is complete, so the upgrade does not have to
  preserve daemon-local SQLite seams that the earlier sprints are deleting
- `docs/atm-runtime/boundaries.md` is created by `AA.2` and must exist before
  this sprint begins

## Out Of Scope

- no new observability product features
- no new daemon/storage boundary decisions
- no retry/replay redesign unrelated to the `sc-observability` API change

## Deliverables

- Workspace dependency versions are bumped to `1.2.0` where ATM currently pins
  `sc-observability` and `sc-observability-types`.

- The concrete `sc-observability` adapter call sites are migrated off the
  deprecated compatibility path. The minimum migration set is frozen now:
  - `crates/atm/src/main.rs`
  - `crates/atm-daemon/bin_support/daemon_observability.rs`
  - if `AA.4` moves the concrete `sc-observability` composition into
    `atm-runtime`, the migration target shifts from daemon bin-support code to
    the `atm-runtime` adapter/composition path that owns logger assembly after
    that move
  - any surviving direct adapter-layer logger call site after `AA.4`

- Queue-admission vs durability semantics are made explicit. The migration rule
  is frozen now:
  - `log()` or `try_log()` is used for queue admission
  - `flush()` or `shutdown()` is the only durability barrier
  - ATM lifecycle code must not assume queue admission implies immediate
    retained-log persistence

- The retained-log policy/config migration is frozen now. The minimum required
  code updates are:
  - replace `maintenance_join_timeout` with `writer_shutdown_timeout`
  - replace old raw policy-field shapes with the `1.2.0` typed wrappers:
    - `ByteCount`
    - `FileCount`
    - `RetentionMaxAge`
    - `MaintenanceCadence`
    - `WriterShutdownTimeout`
  The known affected ATM source is:
  - `crates/atm-daemon/bin_support/daemon_observability.rs`
  - any retained-log policy helper that still uses the old field name or shape

- ATM health/report projection is updated for the `1.2.0` logger runtime.
  The minimum presentation-review surface is frozen now:
  - `crates/atm/src/main.rs`
  - `crates/atm/src/output.rs`
  - `crates/atm-core/src/doctor/mod.rs`
  - `crates/atm-daemon/bin_support/daemon_observability.rs`
  The goal is not to expose every new raw field, but ATM must compile cleanly
  and intentionally decide which queue/writer/maintenance details are surfaced
  in doctor and retained-log health reports.

- The `1.2.0` breaking/migration items are documented in the sprint itself so
  implementation is mechanical:
  1. deprecated `Logger::emit()` compatibility path
  2. queue-backed admission via `log()` / `try_log()`
  3. durability boundary moves to `flush()` / `shutdown()`
  4. `LogError` / `TryLogError` mapping updates
  5. `Logger<Stopped>` shutdown typestate handling
  6. `LoggingHealthReport` queue/writer/maintenance projection review
  7. `RetainedLogPolicy.writer_shutdown_timeout` and typed policy wrappers

## Split Recommendation

Keep this sprint strictly scoped to the `1.2.0` upgrade. Do not mix in
unrelated observability refactors or feature work once the migration compiles.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA6.md` exists with `status: planned`,
  `branch: feature/pAA-s6-obs-upgrade`, and a populated `worktree:`
- the sprint doc enumerates every `sc-observability` `1.2.0` migration item
  currently required by ATM:
  - deprecated `emit()` path
  - `log()` / `try_log()` queue-admission APIs
  - explicit durability barriers
  - `LogError` / `TryLogError`
  - `Logger<Stopped>` shutdown typestate
  - `LoggingHealthReport` queue/writer/maintenance projection review
  - `writer_shutdown_timeout` and typed retained-log policy field migration
- `docs/plan-phase-AA.md` contains an `AA.6` section after `AA.5`
- `docs/project-plan.md` includes the `AA.6` sprint entry before closeout
- `docs/architecture.md` is either updated by this sprint or removed from
  Required Document Updates if no `AA.6` migration detail lands there
- `docs/requirements.md` is either updated by this sprint or removed from
  Required Document Updates if no `AA.6` requirement text lands there
- `docs/atm-core/design/sc-observability-integration.md` is either updated by
  this sprint or removed from Required Document Updates if no migration detail
  lands there

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/sprint-AA6.md`
- `docs/phase-AA/readiness.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/atm-core/design/sc-observability-integration.md`

## Risks And Watchouts

- if ATM upgrades the version pins without replacing the old retained-log
  policy field names, the build will fail on the logger config seam
- if ATM keeps using the deprecated `emit()` path indefinitely, the repo will
  carry forward the old synchronous mental model even though the logger runtime
  is queue-backed
- if doctor/health output ignores the new queue/writer state entirely, ATM may
  compile but lose meaningful operator signal during retained-log degradation
