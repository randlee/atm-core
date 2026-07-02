# AC.0 Storage Architecture Reset ADR And Violation Inventory

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.0
worktree: ../atm-core-worktrees/plan/phase-AC
branch: plan/phase-AC
status: complete
estimated_scope: small
```

## Goal

Freeze the storage-contract reset in an ADR and document the exact current
violations that Phase `AC` must remove.

## Scope Summary

This sprint is architecture reset only. It defines the target crate graph,
freezes the generic RPC envelope rule, freezes canonical shared domain structs
as the target model, and records the current storage/RPC drift so later
sprints delete the right code instead of relocating it.

Production-ready commitment:
- every deliverable listed in this sprint is expected to land at a
  production-ready level for the planning/reset scope this sprint claims;
  shape-only inventory or hand-wavy ADR language is not an accepted closure

Although `AC.0` is completed in the planning worktree, it remains a normal
reviewable sprint artifact. Later planning detail in `AC.1+` is downstream of
this accepted reset and must not silently reinterpret it.

## Search Targets

`AC.0` must search for and inventory the exact surfaces that make the current
design too large or too backend-specific.

Mandatory search targets:

- storage-facing public traits, structs, and enums under:
  - `crates/atm-core/src/boundary/`
  - `crates/atm-core/src/delivery_execution.rs`
- request / response DTO families under:
  - `crates/atm-core/src/boundary/`
- Claude storage seams under:
  - `crates/atm-core/src/mailbox/`
  - `crates/atm-core/src/delivery_execution.rs`
- SQLite coupling under:
  - `crates/atm-core/`
  - `crates/atm-runtime/`
  - `crates/atm-daemon/`
  - `crates/atm-rusqlite/`
- duplicated semantic families for:
  - message
  - roster
  - task

The point of the search is not only to count symbols. It is to produce the
delete / converge / move map that later sprints need.

## Search Method

The minimum reproducible search set is:

```bash
rg -n "pub trait|pub struct|pub enum" \
  crates/atm-core/src/boundary \
  crates/atm-core/src/delivery_execution.rs -S

rg -n "Request|Response|ProjectionMailboxWriter|SourceIngress|MailStore|TaskStore|RosterStore" \
  crates/atm-core/src/boundary \
  crates/atm-core/src/mailbox \
  crates/atm-core/src/delivery_execution.rs -S

rg -n "atm-rusqlite|rusqlite|sqlite" \
  crates/atm-core crates/atm-daemon crates/atm-runtime crates/atm-rusqlite -S
```

Equivalent stronger searches are allowed, but `AC.0` may not omit any of the
search categories above.

## Governing Sources

- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/project-plan.md`
- `crates/atm-core/src/boundary/mail.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary/runtime.rs`
- `crates/atm-core/src/delivery_execution.rs`

## Prerequisites

- none; this sprint is the planning prerequisite for the rest of Phase `AC`

## Out Of Scope

- no new storage crate yet
- no backend extraction yet
- no runtime behavior change

## Deliverables

- `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md`
  freezes these rules:
  - storage traits are CRUD-style and semantic
  - RPC uses a generic envelope plus canonical domain bodies
  - `atm-storage-claude`, SQLite, and future SQL Server are peer backends
  - notifications are a separate post-commit trait

- `docs/plans/phase-AC/issues.md` captures the accepted violation inventory and the
  oversized current-surface baseline:
  - request/response storage DTO proliferation
  - duplicate domain structs across layers
  - Claude storage not treated as a first-class backend
  - SQLite logic leakage above the storage seam
  - missing notification model
  - crate-graph drift that makes SQL Server harder

- The sprint doc includes the explicit target crate graph:

  ```text
  atm-storage
  atm-storage-claude -> atm-storage
  atm-storage-rusqlite -> atm-storage
  atm-core -> atm-storage
  ```

- `docs/plans/phase-AC/storage-surface-inventory.md` records:
  - the concrete storage-facing modules searched
  - the accepted size baseline for the current surface
  - the major overgrowth categories later sprints must shrink

- `docs/plans/phase-AC/type-convergence-map.md` records:
  - the semantic families that must converge into canonical shared types
  - the current duplicate message / roster / task families
  - the handoff expectations for `AC.1` and `AC.5`

- `docs/plans/phase-AC/type-ledger.md` records:
  - the exhaustive storage-adjacent type list for the `AC.0` search scope
  - the keep / merge / delete / backend-only disposition for each current type
  - the owning sprint for each type-level transition

- `docs/plans/phase-AC/crate-graph-migration-map.md` records:
  - the approved target graph
  - the forbidden graph
  - which later sprint owns each transition

- `docs/plans/phase-AC/implementation-ownership-map.md` records:
  - the current concrete implementers of storage-facing traits
  - the current mailbox module ownership candidates
  - which later sprint owns each migration or deletion lane

- `docs/plans/phase-AC/atm-storage-contract-candidate.md` records:
  - the first-pass shared trait surface for `AC.1`
  - the first-pass canonical shared type set
  - the explicit non-goals that must stay out of the shared contract

## Acceptance Criteria

- the ADR states the storage and RPC reset rules unambiguously
- the issue inventory exists and names the accepted drift set
- the target crate graph is recorded explicitly
- the search targets and reproducible search method are documented explicitly
- the collateral planning documents exist and are sufficient for `AC.1+`
- the remaining sprint line is downstream of this accepted baseline

## Required Validation

- `git diff --check`
- `rg -n "Phase AC|atm-storage|atm-storage-claude" docs -S`
- `rg -n "pub trait|pub struct|pub enum" crates/atm-core/src/boundary crates/atm-core/src/delivery_execution.rs -S`
- `rg -n "Request|Response|ProjectionMailboxWriter|SourceIngress|MailStore|TaskStore|RosterStore" crates/atm-core/src/boundary crates/atm-core/src/mailbox crates/atm-core/src/delivery_execution.rs -S`
- `rg -n "atm-rusqlite|rusqlite|sqlite" crates/atm-core crates/atm-daemon crates/atm-runtime crates/atm-rusqlite -S`

## Required Document Updates

- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/plans/phase-AC/issues.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/storage-surface-inventory.md`
- `docs/plans/phase-AC/type-convergence-map.md`
- `docs/plans/phase-AC/type-ledger.md`
- `docs/plans/phase-AC/crate-graph-migration-map.md`
- `docs/plans/phase-AC/implementation-ownership-map.md`
- `docs/plans/phase-AC/atm-storage-contract-candidate.md`
- `docs/project-plan.md`
- `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md`

## Risks And Watchouts

- if the ADR leaves room for RPC-shaped storage traits, the phase will drift immediately
- if the issue inventory is incomplete, later deletion work will silently miss scope
- if the collateral docs do not name concrete modules and duplicate families,
  later sprint planning will drift back into rediscovery instead of execution
