# AA.2 `atm-runtime` Skeleton And Concrete Composition Transfer

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.2
worktree: ../atm-core-worktrees/feature/pAA-s2-atm-runtime-composition-transfer
branch: feature/pAA-s2-atm-runtime-composition-transfer
status: planned
estimated_scope: large
```

## Goal

Introduce `atm-runtime` and move all concrete SQLite/runtime assembly out of
`atm-daemon`.

## Scope Summary

This sprint creates the dedicated composition crate that replaces the daemon as
the legal home for `SqliteBoundaryAssembly`, SQLite observability injection,
and concrete production runtime/store wiring behind the `atm-core`
capability-trait family.

The concrete transfer decisions are frozen now:
- move `SqliteBoundaryAssembly::new*` calls out of
  `crates/atm-daemon/src/composition.rs`
- move `SqliteRemoteReplayStore` out of
  `crates/atm-daemon/src/lib.rs`
- move `RemoteReplayStateRecord` out of `atm-rusqlite` and into a
  storage-neutral `atm-core` boundary surface
- move `RemoteReplayStore` out of `crates/atm-daemon/src/peer_transport.rs`
  and onto a storage-neutral boundary surface that `atm-runtime` can
  implement without depending on `atm-daemon`

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-RUSQLITE-STORE-001`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`
- supersedes the current daemon composition assumption in
  `docs/atm-daemon/architecture.md`

## Governing Boundaries

- `boundaries/atm-core/runtime-factory.toml`
- `boundaries/atm-rusqlite/sqlite-boundary-assembly.toml`
- `boundaries/atm-rusqlite/shared-db.toml`

## Prerequisites

- `AA.1`

## Hard Dependencies

- accepted `atm-runtime` crate ownership model

## Out Of Scope

- final boundary relock
- final doctor split

## Deliverables

- `crates/atm-runtime` exists and is the only legal Phase AA home for
  concrete production runtime/store assembly. The expected minimum file set is:
  - `crates/atm-runtime/src/lib.rs`
  - `crates/atm-runtime/src/composition.rs`
  - `crates/atm-runtime/src/replay_store.rs`

- The minimum runtime bundle contract is frozen in this sprint doc so daemon
  startup does not invent its own seam:

  ```rust
  pub struct RuntimeBundle {
      pub mail_store: Arc<dyn MailStore>,
      pub task_store: Arc<dyn TaskStore>,
      pub roster_store: Arc<dyn RosterStore>,
      pub mail_store_doctor: Arc<dyn MailStoreDoctor>,
      pub task_store_doctor: Arc<dyn TaskStoreDoctor>,
      pub roster_store_doctor: Arc<dyn RosterStoreDoctor>,
      pub config_doctor: Arc<dyn ConfigDoctor>,
      pub remote_replay_store: Arc<dyn RemoteReplayStore>,
  }
  ```

- Concrete SQLite boundary assembly moves into `atm-runtime`. The frozen move
  list is:
  - move `SqliteBoundaryAssembly::new*` calls out of
    `crates/atm-daemon/src/composition.rs`
  - move SQLite observability injection out of daemon composition
  - replace the direct daemon `sqlite_boundary` construction path with an
    injected `RuntimeBundle`

- Replay ownership is moved out of daemon-private and SQLite-private seams.
  The frozen shape is:

  ```rust
  pub struct RemoteReplayStateRecord {
      pub request_id: String,
      pub created_at: DateTime<Utc>,
      pub payload: Vec<u8>,
  }

  pub trait RemoteReplayStore: Send + Sync {
      fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError>;
      fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError>;
      fn delete(&self, request_id: &str) -> Result<(), AtmError>;
      fn purge_expired(&self) -> Result<(), AtmError>;
  }
  ```

- `atm-daemon` startup consumes only injected storage-neutral runtime inputs
  expressed through `MailStore`, `TaskStore`, `RosterStore`, the doctor
  traits from `AA.1`, and `RemoteReplayStore`.

## Split Recommendation

Do not combine doctor behavior changes into this sprint. This sprint is only
about composition ownership transfer.

## Acceptance Criteria

- `atm-runtime` exists and owns concrete production assembly
- `atm-daemon` no longer constructs `SqliteBoundaryAssembly`
- `RemoteReplayStateRecord` and `RemoteReplayStore` no longer originate in
  daemon-private or SQLite-private modules
- the sprint doc contains the minimum `RuntimeBundle` and replay contract
  shapes, so implementation does not have to invent them during the move
- daemon composition consumes storage-neutral injected inputs
- daemon composition depends on `MailStore` / `TaskStore` / `RosterStore` plus
  doctor traits rather than backend identity
- direct daemon-to-SQLite assembly imports are removed from production
  composition code

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Required Document Updates

- `Cargo.toml`
- `docs/project-plan.md`
- `docs/plan-phase-AA.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-runtime/architecture.md`
- `docs/atm-runtime/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-rusqlite/requirements.md`

## Risks And Watchouts

- the crate move must not just relocate daemon-shaped assumptions into
  `atm-runtime`; the new crate is composition-only
