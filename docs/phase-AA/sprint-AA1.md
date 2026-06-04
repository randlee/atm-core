# AA.1 Subsystem Doctor Traits And Shared Diagnostic Contracts

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.1
worktree: ../atm-core-worktrees/feature/pAA-s1-subsystem-doctor-traits
branch: feature/pAA-s1-subsystem-doctor-traits
status: complete
estimated_scope: medium
```

## Goal

Define the storage-facing capability traits, subsystem doctor traits, and
shared diagnostic DTOs so every subsystem can report its own health without
the daemon or CLI reimplementing subsystem logic.

## Scope Summary

This sprint introduces the architectural contract that later code-removal
sprints use: `atm-core` keeps `MailStore` / `TaskStore` / `RosterStore` as the
primary storage-neutral capability boundaries, adds doctor traits beside them,
allows backend implementations such as SQLite and Claude JSON to satisfy the
same trait family, and makes `atm-daemon` consume injected traits rather than
peeking into SQLite internals.

## Governing Requirements

- `REQ-P-DOCTOR-001`
- `REQ-CORE-DOCTOR-001`
- `REQ-CORE-BOUNDARY-001`
- `REQ-RUSQLITE-STORE-001`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`

## Governing Boundaries

- `docs/atm-core/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Prerequisites

- `AA.0`

## Hard Dependencies

- accepted shared diagnostic DTO shape

## Out Of Scope

- concrete runtime composition transfer
- daemon code deletion
- boundary relock

## Deliverables

- The Phase AA trait-family reuse decision is frozen in the docs:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
  remain the primary storage-neutral capability traits.
  This sprint must not introduce a second parallel
  `MessageReader` / `MessageWriter` / `RosterReader` / `RosterWriter`
  hierarchy.

- The doctor traits are defined beside the existing capability boundaries in
  `atm-core`. The intended shape is frozen now:

  ```rust
  pub trait MailStoreDoctor: Send + Sync {
      fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError>;
  }

  pub trait TaskStoreDoctor: Send + Sync {
      fn inspect_task_store(&self) -> Result<TaskStoreDoctorReport, AtmError>;
  }

  pub trait RosterStoreDoctor: Send + Sync {
      fn inspect_roster_store(&self) -> Result<RosterStoreDoctorReport, AtmError>;
  }

  pub trait ConfigDoctor: Send + Sync {
      fn inspect_config(&self) -> Result<ConfigDoctorReport, AtmError>;
  }
  ```

- The shared report DTO direction is frozen now: subsystem reports are
  normalized enough for aggregation but still allow backend-specific findings.
  The minimum DTO shape is:

  ```rust
  pub struct DoctorFinding {
      pub code: &'static str,
      pub severity: DoctorSeverity,
      pub summary: String,
      pub detail: Option<String>,
  }

  pub struct MailStoreDoctorReport {
      pub findings: Vec<DoctorFinding>,
  }

  pub struct TaskStoreDoctorReport {
      pub findings: Vec<DoctorFinding>,
  }

  pub struct RosterStoreDoctorReport {
      pub findings: Vec<DoctorFinding>,
  }

  pub struct ConfigDoctorReport {
      pub findings: Vec<DoctorFinding>,
  }
  ```

- `atm-rusqlite` owns store health through the doctor traits. The minimum
  store-doctor responsibilities are frozen:
  - path resolution
  - openability
  - schema/bootstrap/migration readiness
  - bounded store findings
  - bounded task-store findings when `TaskStore` is backed by the same store

- Daemon doctor aggregation is documented as aggregate-only. The minimum
  ownership rule is frozen:
  - daemon doctor may aggregate `ConfigDoctor`, `MailStoreDoctor`,
    `TaskStoreDoctor`, and `RosterStoreDoctor` reports plus daemon-owned
    runtime state
  - daemon doctor may compare subsystem reports for drift
  - daemon doctor must not inspect SQLite internals directly

- The backend-agnostic rule is frozen: SQLite-backed and Claude-JSON-backed
  implementations may satisfy the same behavior-named trait family later, but
  this sprint does not require the Claude JSON implementation to land.

## Split Recommendation

Keep the trait and DTO line separate from the composition transfer. This sprint
must settle the contracts before any crate starts moving code across them.

## Acceptance Criteria

- `MailStore`, `TaskStore`, and `RosterStore` are explicitly retained as the
  Phase AA storage-neutral capability surfaces
- `MailStoreDoctor`, `TaskStoreDoctor`, `RosterStoreDoctor`, and
  `ConfigDoctor` are defined in `atm-core`
- the sprint docs include explicit trait signatures and a minimum report DTO
  shape, including the minimum subsystem report structs, so implementation
  does not have to invent the contract later
- `atm-rusqlite` has a documented store doctor contract
- the docs explicitly allow both SQLite-backed and Claude-JSON-backed
  implementations behind the same behavior-named traits
- daemon doctor aggregation is explicitly aggregate-only
- no new daemon-local SQLite helper is introduced while landing the traits

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`

## Risks And Watchouts

- if AA creates a second parallel read/write trait hierarchy on top of
  `MailStore` / `TaskStore` / `RosterStore`, the phase will widen churn instead
  of making the later deletion sprints mechanical
