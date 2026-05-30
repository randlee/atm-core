# AA.3 Direct Doctor Path And Runtime Health Simplification

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.3
worktree: ../atm-core-worktrees/feature/pAA-s3-direct-doctor-and-runtime-health-split
branch: feature/pAA-s3-direct-doctor-and-runtime-health-split
status: planned
estimated_scope: medium
```

## Goal

Move store diagnostics into the store subsystem, restore a direct CLI doctor
path for local diagnostics, and strip SQLite-specific health logic out of the
daemon.

## Scope Summary

This sprint applies the doctor-trait model: `atm-rusqlite` diagnoses itself,
the CLI can query it directly, and daemon health becomes aggregation of
daemon-owned runtime state plus injected subsystem reports, including
cross-subsystem drift comparison where that comparison is not backend-specific.

The concrete simplification decisions are frozen now:
- remove `sqlite_boundary: Option<SqliteBoundaryAssembly>` from
  `crates/atm-daemon/src/runtime_health.rs`
- replace direct roster access there with injected `RosterStore`
- remove daemon-owned WAL checkpoint calls from `runtime_health.rs`
- remove `sqlite_ready` / `sqlite_detail` from
  `crates/atm-daemon/src/runtime_status_cache.rs` and from
  `crates/atm-core/src/protocol.rs::RuntimeStatusSnapshot`
- report store health through subsystem doctor results, not daemon cache fields
- keep daemon runtime health free to aggregate subsystem doctor reports, but do
  not let it perform backend-specific investigation logic itself

## Governing Requirements

- `REQ-P-DOCTOR-001`
- `REQ-CORE-DOCTOR-001`
- `REQ-DAEMON-HEALTH-001`
- `REQ-RUSQLITE-STORE-001`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`
- `docs/adr/ADR-014-runtime-health-projection-and-liveness-signal-ownership.md`

## Governing Boundaries

- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`

## Prerequisites

- `AA.1`
- `AA.2`

## Hard Dependencies

- shared doctor traits from `AA.1`

## Non-Goals

- boundary relock
- full replay leak cleanup

## Sub-Tasks

- Implement `atm-rusqlite` store-doctor logic.
  Development work: move SQLite readiness/openability/migration health into
  the store subsystem and expose it only through the doctor trait. This
  includes the current direct SQLite readiness concerns presently projected
  through daemon runtime status.
  Required tests: in-process store doctor tests.
  Required doc or boundary updates: `atm-rusqlite` docs.

- Restore a direct CLI doctor path for local diagnostics.
  Development work: make CLI doctor query local config/store diagnostics
  directly when daemon-owned runtime state is not required. The concrete core
  target is `crates/atm-core/src/doctor/mod.rs`, which should become an
  aggregator/orchestrator over `ConfigDoctor`, `MailStoreDoctor`, and
  `RosterStoreDoctor` instead of burying backend-specific logic inline.
  Required tests: doctor CLI regression coverage with and without a live
  daemon.
  Required doc or boundary updates: product requirements and architecture.

- Simplify daemon runtime health to daemon-owned projection only.
  Development work: remove direct SQLite roster/store health probing from
  `runtime_health.rs`, convert `reload_runtime_view(...)` and
  `record_heartbeat(...)` to injected `RosterStore`, and delete
  `mark_sqlite_unavailable*`-style daemon cache semantics. The precise cache
  decision is:
  - `RuntimeStatusSnapshot` keeps daemon runtime liveness/readiness/member
    counts
  - `RuntimeStatusSnapshot` stops carrying `sqlite_ready` and `sqlite_detail`
  - any store-specific detail appears only in subsystem doctor reports
  Required tests: runtime-health projection tests.
  Required doc or boundary updates: daemon requirements/architecture.

## Split Recommendation

Keep doctor behavior and runtime-health simplification together; they are the
same seam from opposite sides.

## Acceptance Criteria

- store diagnostics are owned by `atm-rusqlite`
- CLI doctor can answer direct local store/config checks without daemon routing
- daemon runtime health no longer owns SQLite-specific probing or SQLite-named
  runtime cache fields
- daemon doctor aggregation may include injected store/config subsystem reports
  but does not deeply analyze SQLite internals and only performs
  cross-subsystem comparison at the report level

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-rusqlite/requirements.md`

## Risks And Watchouts

- do not let “optional daemon fast path” turn back into “daemon owns the only
  doctor implementation”
