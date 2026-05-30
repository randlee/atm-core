# AA.1 Subsystem Doctor Traits And Shared Diagnostic Contracts

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.1
worktree: ../atm-core-worktrees/feature/pAA-s1-subsystem-doctor-traits
branch: feature/pAA-s1-subsystem-doctor-traits
status: planned
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

## Non-Goals

- no concrete runtime composition transfer yet
- no boundary relock yet

## Sub-Tasks

- Freeze the capability-trait reuse decision.
  Development work: keep `MailStore`, `TaskStore`, and `RosterStore` as the
  storage-neutral read/write capability surfaces for Phase AA; do not add a
  parallel `MessageReader` / `MessageWriter` / `RosterReader` / `RosterWriter`
  hierarchy in this phase.
  Required tests: compile/build validation for unchanged callers.
  Required doc or boundary updates: `atm-core` requirements and architecture.

- Add doctor traits beside the existing capability boundaries.
  Development work: define `MailStoreDoctor` beside
  `crates/atm-core/src/boundary/mail.rs`, define `RosterStoreDoctor` beside
  `crates/atm-core/src/boundary/store.rs`, and define `ConfigDoctor` beside
  the config ingress boundary so deep backend/config investigation logic lives
  with the owning subsystem.
  Required tests: trait/object-safety and DTO unit coverage.
  Required doc or boundary updates: `atm-core` requirements, architecture, and
  boundaries.

- Define the store doctor contract for `atm-rusqlite`.
  Development work: document and implement the trait that reports store path,
  openability, migration/bootstrap readiness, and bounded storage findings.
  Required tests: in-process SQLite doctor tests.
  Required doc or boundary updates: `atm-rusqlite` requirements/architecture.

- Define daemon-owned runtime doctor aggregation.
  Development work: document that daemon doctor code aggregates injected
  subsystem reports plus daemon-only runtime state, may compare subsystem
  outputs for drift, and does not inspect SQLite directly.
  Required tests: aggregation-only unit coverage.
  Required doc or boundary updates: `atm-daemon` requirements/architecture.

- Define the backend-agnostic implementation rule.
  Development work: document that behavior traits are backend-neutral and may
  be implemented by both SQLite-backed and Claude-JSON-backed subsystems,
  without exposing one giant `Storage` god-interface. The Phase AA decision is
  that Claude JSON may implement the same `MailStore` / `RosterStore` plus
  doctor traits later; AA does not require that implementation to land now.
  Required tests: none beyond doc and trait-surface validation.
  Required doc or boundary updates: `requirements.md`, `architecture.md`, and
  `atm-core` docs.

## Split Recommendation

Keep the trait and DTO line separate from the composition transfer. This sprint
must settle the contracts before any crate starts moving code across them.

## Acceptance Criteria

- `MailStore`, `TaskStore`, and `RosterStore` are explicitly retained as the
  Phase AA storage-neutral capability surfaces
- `MailStoreDoctor`, `RosterStoreDoctor`, and `ConfigDoctor` are defined in
  `atm-core`
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
