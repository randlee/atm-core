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
- the concrete `ConfigDoctor` implementation lives in `atm-runtime`
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
- `boundaries/atm-runtime/runtime-composition.toml`

## Prerequisites

- `AA.1`
- `AA.2`

## Hard Dependencies

- shared doctor traits from `AA.1`

## Out Of Scope

- boundary relock
- full replay leak cleanup

## Deliverables

- `atm-rusqlite` owns store diagnostics through the doctor traits. The frozen
  doctor responsibility list is:
  - SQLite openability
  - schema/bootstrap/migration readiness
  - bounded store findings
  - any SQLite-specific detail that does not belong in daemon runtime state

- `atm-runtime` owns the concrete `ConfigDoctor` implementation and the direct
  local doctor-path assembly used by the CLI. The frozen ownership rule is:
  - `ConfigDoctor` trait stays in `atm-core`
  - concrete `ConfigDoctor` implementation lives in `atm-runtime`
  - backend-specific `config.json` investigation does not live in
    `atm-daemon`

- `atm-core/src/doctor/mod.rs` becomes an explicit aggregator/orchestrator over
  subsystem doctors rather than a backend-specific implementation surface.
  The minimum aggregate shape is frozen:

  ```rust
  pub struct DoctorReport {
      pub config: ConfigDoctorReport,
      pub mail_store: MailStoreDoctorReport,
      pub task_store: TaskStoreDoctorReport,
      pub roster_store: RosterStoreDoctorReport,
      pub daemon_runtime: Option<DaemonRuntimeDoctorReport>,
      pub drift_findings: Vec<DoctorFinding>,
  }

  pub struct DaemonRuntimeDoctorReport {
      pub findings: Vec<DoctorFinding>,
  }
  ```

- The direct CLI doctor path is restored for local diagnostics that do not
  require daemon-owned runtime state. The frozen dependency-edge decision is:
  - the `atm` crate depends on `atm-runtime` for direct local doctor-path
    assembly
  - `atm` must not depend directly on `atm-rusqlite`
  - direct local doctor wiring must remain outside `atm-daemon`

- `RuntimeStatusSnapshot` is simplified to daemon-owned runtime state only.
  The frozen delete list is:
  - remove `sqlite_boundary: Option<SqliteBoundaryAssembly>` from
    `crates/atm-daemon/src/runtime_health.rs`
  - remove direct roster-store hydration through SQLite from
    `runtime_health.rs`
  - remove daemon-owned WAL checkpoint calls from `runtime_health.rs`
  - remove `sqlite_ready`
  - remove `sqlite_detail`
  - remove `mark_sqlite_unavailable(...)`
  - remove `mark_sqlite_unavailable_with_detail(...)`

- The minimum post-AA runtime snapshot direction is frozen:

  ```rust
  pub struct RuntimeStatusSnapshot {
      pub daemon_ready: bool,
      pub active_connections: u64,
      pub active_advisory_sessions: u64,
      pub live_member_count: u64,
      // no sqlite_ready
      // no sqlite_detail
  }
  ```

## Split Recommendation

Keep doctor behavior and runtime-health simplification together; they are the
same seam from opposite sides.

## Acceptance Criteria

- store diagnostics are owned by `atm-rusqlite`
- the owning crate for the concrete `ConfigDoctor` implementation is named
  explicitly as `atm-runtime`
- CLI doctor can answer direct local store/config checks without daemon routing
- the `atm -> atm-runtime` dependency edge is frozen explicitly as the direct
  local doctor-path seam, and no direct `atm -> atm-rusqlite` edge is allowed
- daemon runtime health no longer owns SQLite-specific probing or SQLite-named
  runtime cache fields
- the sprint doc freezes the aggregate doctor DTO and the reduced runtime
  snapshot shape so later code work is mechanical
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
- `docs/atm-runtime/architecture.md`
- `docs/atm-runtime/requirements.md`
- `docs/atm-runtime/boundaries.md`
- `docs/atm-rusqlite/requirements.md`

## Risks And Watchouts

- do not let “optional daemon fast path” turn back into “daemon owns the only
  doctor implementation”
- `AA.3` must update `docs/atm-daemon/architecture.md` and
  `docs/architecture.md` so their runtime-health contract no longer claims
  daemon-owned `sqlite_ready` / `sqlite_detail` fields once this sprint lands
