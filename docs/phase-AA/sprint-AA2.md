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

## Non-Goals

- final boundary relock
- final doctor split

## Sub-Tasks

- Add `crates/atm-runtime`.
  Development work: introduce the crate, manifests, and composition entrypoints
  that can build production runtime inputs from concrete adapters. The expected
  new files are:
  - `crates/atm-runtime/src/lib.rs`
  - `crates/atm-runtime/src/composition.rs`
  - `crates/atm-runtime/src/replay_store.rs`
  Required tests: compile/build coverage and narrow integration coverage.
  Required doc or boundary updates: project plan, crate docs, boundaries.

- Move SQLite boundary assembly into `atm-runtime`.
  Development work: relocate `SqliteBoundaryAssembly` construction and
  observability injection out of daemon composition. The concrete daemon edits
  are:
  - delete `SqliteBoundaryAssembly` construction from
    `crates/atm-daemon/src/composition.rs`
  - replace the direct `sqlite_boundary` construction path with an injected
    runtime bundle from `atm-runtime`
  Required tests: runtime composition tests proving `atm-daemon` consumes
  injected ports only.
  Required doc or boundary updates: daemon/runtime/rusqlite architecture docs.

- Change `atm-daemon` to consume storage-neutral runtime inputs.
  Development work: delete direct construction from daemon composition and make
  daemon startup take injected ports/factories only, expressed through
  `MailStore`, `TaskStore`, `RosterStore`, and the new doctor traits from
  `AA.1`.
  Required tests: daemon startup and request-dispatch regression coverage.
  Required doc or boundary updates: daemon boundaries and architecture docs.

## Split Recommendation

Do not combine doctor behavior changes into this sprint. This sprint is only
about composition ownership transfer.

## Acceptance Criteria

- `atm-runtime` exists and owns concrete production assembly
- `atm-daemon` no longer constructs `SqliteBoundaryAssembly`
- `RemoteReplayStateRecord` and `RemoteReplayStore` no longer originate in
  daemon-private or SQLite-private modules
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
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-rusqlite/requirements.md`

## Risks And Watchouts

- the crate move must not just relocate daemon-shaped assumptions into
  `atm-runtime`; the new crate is composition-only
