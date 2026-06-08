---
title: Phase AC Plan
status: complete
branch: plan/phase-AC
worktree: ../atm-core-worktrees/plan/phase-AC
---

# Phase AC Plan

## Goal

Restore the original storage and RPC design:

- generic RPC envelope
- canonical domain structs
- interchangeable storage backends
- Claude inbox storage treated as a first-class backend
- SQLite storage treated as a backend, not as the natural home of business logic
- future SQL Server support kept viable by the shared storage contract

Phase `AC` exists because the current design drifted into:

- per-operation request/response storage DTOs
- duplicated message/task/roster representations across RPC, storage, and core
- backend-shaped seams instead of semantic storage traits
- concrete SQLite logic leaking upward into daemon/runtime/core paths

The required end state is:

- `crates/atm-storage` owns a small audited storage contract
- `crates/atm-storage-claude` implements that contract for Claude inbox JSON storage
- the SQLite backend implements the same contract with richer capabilities layered separately
- RPC carries canonical domain bodies under one generic envelope instead of per-message transport clones
- `atm-core` depends on storage semantics, not concrete backend details

## Design Rules

Phase `AC` is not exploratory redesign. It is a corrective return to the original model.

The governing rules are:

- storage traits are CRUD-style semantic traits, not RPC-style service entrypoints
- storage uses canonical shared structs
- RPC bodies decode into those same canonical shared structs
- backend-specific omissions are implementation behavior, not type proliferation
- notifications are a separate trait and occur only after durable write succeeds
- `atm-storage-claude`, `atm-storage-rusqlite`, and future `atm-storage-sqlserver` are peer backends

## Scope Rules

Phase `AC` may:

- create new storage crates
- move canonical storage-facing structs out of `atm-core`
- collapse duplicate RPC/storage/domain record types
- replace request/response-per-operation storage traits with semantic CRUD traits
- refactor daemon/runtime/core to depend on shared storage traits only
- delete obsolete wrappers, adapters, and backend-specific seams

Phase `AC` must not:

- preserve the current boundary DTO sprawl just by moving it to a new crate
- define a single giant `Storage` god-trait
- leave Claude storage as a special compatibility side-path rather than a real backend
- reintroduce concrete SQLite knowledge above the storage trait line
- make SQL Server support harder than it is today

## Baseline

- planning branch: `plan/phase-AC`
- expected execution integration branch: `integrate/phase-AC`
- prerequisite accepted line:
  - Phase `AA` and its follow-up fixes are either merged or frozen as the baseline
- authoritative supporting artifacts for this planning line:
  - `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md`
  - `docs/plans/phase-AC/readiness.md`
  - `docs/plans/phase-AC/issues.md`
  - `docs/plans/phase-AC/storage-surface-inventory.md`
  - `docs/plans/phase-AC/type-convergence-map.md`
  - `docs/plans/phase-AC/type-ledger.md`
  - `docs/plans/phase-AC/crate-graph-migration-map.md`
  - `docs/plans/phase-AC/implementation-ownership-map.md`
  - `docs/plans/phase-AC/atm-storage-contract-candidate.md`

## Phase Entry Criteria

Phase `AC` planning required a prerequisite reset before the implementation
sprints could be sequenced. That prerequisite is now complete in `AC.0`.

`AC.0` completion means:

- the storage/RPC drift is recorded as an explicit architecture issue
- the original design reset is frozen in `ADR-018`
- the initial storage trait inventory proves the current surface is oversized
- the target crate graph is accepted before any extraction begins
- the collateral planning docs exist so `AC.1+` can execute against concrete
  inventories instead of rediscovering scope

## Target Crate Graph

Required target graph:

- `atm-storage`
- `atm-storage-claude -> atm-storage`
- `atm-storage-rusqlite -> atm-storage`
- `atm-core -> atm-storage`

Forbidden target graph:

- `atm-storage-* -> atm-core`
- `atm-storage` containing RPC request/response envelopes
- `atm-core` depending on concrete storage crates outside the approved composition seams

## Target Storage Contract

The first contract pass must stay small enough to audit directly.

Target contract shape:

- `2` core CRUD traits:
  - `MessageStore`
  - `RosterStore`
- `1` required notification trait:
  - `StorageNotifier`
- no more than `4` optional capability traits without an ADR:
  - e.g. `StorageHealth`, `ReplayStore`, `RepairableStorage`, `TransactionalStorage`
- no request/response wrapper pair per operation
- no backend-specific structs in the shared contract

Illustrative shape:

```rust
pub trait MessageStore {
    fn save_message(&self, message: &Message) -> Result<(), AtmError>;
    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError>;
    fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError>;
    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError>;
}

pub trait RosterStore {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError>;
    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError>;
    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;
}

pub trait StorageNotifier {
    fn message_received(&self, event: &MessageReceivedEvent) -> Result<(), AtmError>;
    fn roster_changed(&self, event: &RosterChangedEvent) -> Result<(), AtmError>;
}
```

Task-storage rule:

- task storage is explicitly out of scope for the initial Phase `AC` contract
- current `TaskStore` code is treated as speculative, not as an approved
  baseline the shared contract must preserve
- speculative task-store code should be deleted by default during `AC.6`;
  quarantine is only a fallback if immediate removal is blocked by unrelated
  stabilization work
- if task storage is approved later, the first canonical implementation starts
  from Claude-code task schema plus Pydantic validation, with SQLite sync only
  afterward if still needed

## RPC Rule

RPC is not the storage API.

Required model:

```rust
pub struct RpcEnvelope {
    pub header: RpcHeader,
    pub body: bytes::Bytes,
}
```

Rules:

- `RpcEnvelope` is owned by `atm-daemon-client` for this phase line
- `atm-storage` is not an allowed owner because the envelope is transport, not
  storage
- RPC carries one generic envelope
- message-like bodies decode into canonical domain structs
- storage persists those same canonical domain structs
- per-message RPC clones are deleted unless a true semantic difference exists

## Sprint Sequence

### AC.0 Storage Architecture Reset ADR And Violation Inventory

Purpose:

- freeze the original design reset in an ADR
- document the current storage/RPC drift and crate-graph violations
- define the non-negotiable rules for `atm-storage`

Completed in planning branch:
- `plan/phase-AC`

Completed in planning worktree:
- `../atm-core-worktrees/plan/phase-AC`

Completion note:
- `AC.0` is a planning prerequisite, not a deferred implementation sprint
- execution sprint work begins with `AC.1`
- `AC.0` remains a reviewable sprint artifact while the downstream `AC.1+`
  planning and implementation sequence is refined

### AC.1 `atm-storage` Contract And Canonical Domain Types

Purpose:

- create `crates/atm-storage`
- define the small audited trait set
- define canonical storage/RPC-shared domain structs
- define notifications as a separate post-commit trait

Execution branch:
- `feature/pAC-s1-atm-storage-contract-and-canonical-types`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s1-atm-storage-contract-and-canonical-types`

### AC.2 `atm-storage-claude` Extraction

Purpose:

- extract Claude inbox storage into `crates/atm-storage-claude`
- implement the shared storage traits for Claude storage
- keep JSON salvage, file locking, source discovery, and rewrite mechanics internal
- make deferred task-storage policy explicit so backend interchangeability
  claims stay reviewable rather than implied

Execution branch:
- `feature/pAC-s2-atm-storage-claude-extraction`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s2-atm-storage-claude-extraction`

### AC.3 SQLite Backend Convergence

Purpose:

- adapt the SQLite backend to the same `atm-storage` traits
- ensure the concrete SQLite backend does not depend on `atm-core`
- ensure post-commit notifications happen only after durable write success
- do not preserve speculative SQLite task persistence as approved backend scope

Execution branch:
- `feature/pAC-s3-sqlite-backend-convergence`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s3-sqlite-backend-convergence`

Parallelism rule for `AC.2` and `AC.3`:
- the two backend sprints may run in parallel only while they respect the
  non-overlapping `atm-core` ownership split declared in their sprint docs
- if either sprint needs to cross that split, `AC.2` merges first and `AC.3`
  merge-forwards from the updated branch before continuing

### AC.4 `atm-core` Storage Boundary Adoption

Purpose:

- make `atm-core`, runtime, and daemon paths depend on storage traits only
- delete direct concrete backend logic above the composition seam
- stop treating SQLite as the natural home of business logic

Execution branch:
- `feature/pAC-s4-atm-core-storage-boundary-adoption`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s4-atm-core-storage-boundary-adoption`

### AC.5 RPC Envelope And Domain Type Unification

Purpose:

- replace per-message transport structs with the generic RPC envelope
- make RPC bodies and storage share the same canonical domain structs
- delete redundant message/roster DTO layers

Execution branch:
- `feature/pAC-s5-rpc-envelope-and-domain-type-unification`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s5-rpc-envelope-and-domain-type-unification`

### AC.6 Cleanup And Deletion Closeout

Purpose:

- delete obsolete storage wrappers and transport clones
- close remaining backend leakage
- leave behind a deletion-closed contract surface that can be audited directly

Execution branch:
- `feature/pAC-s6-cleanup-and-deletion-closeout`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s6-cleanup-and-deletion-closeout`

### AC.7 SQL Server Readiness Proof

Purpose:

- prove the resulting contract is small enough to audit and suitable for a
  future SQL Server backend
- record the exact remaining backend-only work for `atm-storage-sqlserver`

Execution branch:
- `feature/pAC-s7-sqlserver-readiness-proof`

Execution worktree:
- `../atm-core-worktrees/feature/pAC-s7-sqlserver-readiness-proof`

## Immediate Planning Outputs

Phase `AC` planning is not complete until these artifacts exist and agree:

- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/issues.md`
- `docs/plans/phase-AC/sprint-AC0.md`
- `docs/plans/phase-AC/sprint-AC1.md`
- `docs/plans/phase-AC/sprint-AC2.md`
- `docs/plans/phase-AC/sprint-AC3.md`
- `docs/plans/phase-AC/sprint-AC4.md`
- `docs/plans/phase-AC/sprint-AC5.md`
- `docs/plans/phase-AC/sprint-AC6.md`
- `docs/plans/phase-AC/sprint-AC7.md`
- `docs/plans/phase-AC/storage-surface-inventory.md`
- `docs/plans/phase-AC/type-convergence-map.md`
- `docs/plans/phase-AC/type-ledger.md`
- `docs/plans/phase-AC/crate-graph-migration-map.md`
- `docs/plans/phase-AC/implementation-ownership-map.md`
- `docs/plans/phase-AC/atm-storage-contract-candidate.md`
- `docs/plans/phase-AC/sqlserver-readiness-proof.md` reserved as the AC.7 proof artifact path

## Phase Exit Criteria

Phase `AC` is not complete until:

- `crates/atm-storage` exists and contains only the small audited storage contract
- the shared storage contract does not use per-operation request/response wrappers
- canonical shared `Message` and `Roster*` structs are used at both RPC-body
  and storage boundaries
- `atm-storage-claude` and the SQLite backend implement the same approved core
  storage traits
- the concrete SQLite backend does not depend on `atm-core`
- notifications are modeled through a separate trait with post-commit semantics
- `atm-core`, daemon, and runtime composition paths no longer contain direct concrete storage logic above the approved composition seam
- the repo has no remaining message-shaped RPC/storage/domain struct proliferation that contradicts the generic envelope model
- the resulting storage contract is explicitly documented as suitable for a future `atm-storage-sqlserver` implementation
- speculative task-store code is not treated as an approved compatibility line
  inside the shared storage contract

## Phase Execution Guardrails

- `capability-candidate` rows are fail-closed:
  - if a later sprint does not explicitly promote one to a named capability
    trait, it must be deleted or internalized
- `AC.3` owns the backend naming cutover to `atm-storage-rusqlite`; rename
  deferral is not an accepted outcome
- any new public type introduced during `AC.1`..`AC.7` must be added to
  `docs/plans/phase-AC/type-ledger.md` in the same change or the sprint is not
  complete
