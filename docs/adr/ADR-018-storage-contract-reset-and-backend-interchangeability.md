# ADR-018: Storage Contract Reset And Backend Interchangeability

## Status

Accepted

Phase AD supersession note:
- `ADR-019` retires the former `atm-storage-claude` crate from the accepted
  product
  architecture because Claude Code no longer uses that backend
- this ADR still governs the requirement for a shared storage contract,
  backend interoperability, and future SQL backend support

Phase AI supersession note:
- ADR-033 retires this ADR's generic byte-envelope RPC decision. Phase AI uses
  versioned HTTP JSON over UDS/HTTPS, while this ADR continues to govern
  backend-neutral storage traits and canonical domain records.

## Context

The original ATM architecture required:

- a generic RPC envelope
- canonical shared domain structs
- interchangeable storage backends
- Claude inbox storage treated as a real backend rather than a compatibility-only side path
- future SQL Server support kept viable by the shared storage contract

The current implementation drifted away from that model. The storage and RPC
surface now reflects backend-shaped and operation-shaped growth rather than a
small semantic contract:

- storage traits and boundaries are modeled as request/response-per-operation
  entrypoints instead of CRUD-style semantic storage traits
- message, roster, and task representations are duplicated across RPC,
  storage, and internal execution paths
- Claude inbox storage is handled as a special path instead of as a peer
  backend
- SQLite became the implicit home of business logic, which allowed concrete
  SQLite behavior to leak upward into runtime, daemon, and core paths
- notification semantics are underspecified because writes and emitted events
  are not frozen behind a separate post-commit contract

The current storage-facing boundary volume is materially larger than the target
architecture needs:

- roughly `13` traits
- roughly `95` structs
- roughly `3` enums

That surface is too large to audit as the shared storage contract and is
evidence that the current design models calls to storage rather than the
storage domain itself.

## Decision

ATM will restore the original model through a new shared storage contract and
shared canonical domain types.

### 1. Storage Contract Ownership

ATM will create a small audited shared contract crate:

- `crates/atm-storage`

That crate owns:

- shared storage traits
- shared canonical domain structs used by storage and RPC bodies
- small shared query / key / mutation structs only where semantically required

It must not own:

- per-operation request/response RPC-style DTO families
- backend-specific filesystem / lock / SQLite / SQL Server structs
- daemon/runtime composition logic

### 2. Required Backend Hierarchy

The approved backend graph is:

```text
atm-storage
atm-storage-rusqlite -> atm-storage
atm-core -> atm-storage
```

Future SQL Server support follows the same model:

```text
atm-storage-sqlserver -> atm-storage
```

Forbidden graph edges:

- `atm-storage-* -> atm-core`
- `atm-storage` depending on concrete backend crates
- daemon/runtime/core owning concrete backend behavior above the approved
  composition seam

Historical note:
- `AC.2` landed `atm-storage-claude` as a concrete backend before `ADR-019`
  retired it from the accepted line
- `ADR-019` later retires that backend from the accepted product architecture
  because Claude Code no longer uses it
- that retirement does not weaken the requirement that the shared
  `atm-storage` contract remain backend-interoperable and future-SQL-ready
- backend interoperability does not require multiple live concrete backends at
  every release; it requires that the shared contract stays capable of
  supporting additional concrete backends without architectural rewrite

### 3. Storage Traits Are Semantic CRUD Traits

The shared storage contract is CRUD-style and semantic.

Required core traits:

- `MessageStore`
- `RosterStore`

Required separate trait:

- `StorageNotifier`

Notifications are not part of CRUD mutation success itself. They are emitted
only after durable write success.

Optional capability traits are allowed only when they express truly additional
backend capability rather than operation wrapper growth. More than four
capability traits requires a follow-up ADR.

### 4. Historical RPC Envelope (superseded)

The former generic byte-envelope RPC is not the storage API and is no longer
the accepted transport contract.

ADR-033 replaces it with a versioned HTTP/OpenAPI contract whose adapters
encode the same canonical domain types. ATM must not define per-transport
message clones where an existing canonical type suffices.

### 5. Canonical Domain Structs Are Shared Across Layers

ATM uses one canonical shared representation for the semantic domain records
that cross both RPC and storage boundaries.

Examples include:

- `Message`
- `RosterMember`
- `RosterSnapshot`

Backend-specific omissions are implementation behavior, not a reason to create
parallel struct families. Claude storage may ignore ATM-only fields it cannot
persist, but it still consumes the same shared semantic record types.

### 6. Task Storage Is Deferred, Not Part Of The Initial AC Contract

Task storage is explicitly out of scope for the initial Phase `AC` storage
reset.

Existing `TaskStore` code and SQLite task persistence are not treated as an
approved architectural baseline for this phase. They are speculative legacy
surface, not a contract the new storage model must preserve.

If task storage is approved later, the required starting point is:

- canonical Claude-code task storage behavior
- validation against the Claude schema and its Pydantic models
- only then any SQLite synchronization or secondary persistence work

Phase `AC` must therefore not:

- include `TaskStore` in the initial shared `atm-storage` contract
- treat SQLite task persistence as a source of truth
- preserve speculative task-store code as if it were a required compatibility
  line

### 7. Historical Claude Backend Note

`AC.2` landed Claude inbox JSON storage as a first-class backend
implementation of the shared storage contract.

`ADR-019` retires that concrete backend from the accepted product
architecture because Claude Code no longer uses it.

The retirement of the Claude backend does not retire:

- the shared semantic `MessageStore` / `RosterStore` contract
- backend interoperability as an architectural requirement
- future SQL backend support
- the ability to add a new concrete backend without redefining the core
  storage architecture

### 8. SQLite Is One Backend, Not The Architecture

SQLite remains a backend implementation of the shared storage contract. It is
not the natural home of ATM business logic.

Concrete SQLite behavior such as:

- transactional details
- query/index behavior
- write-worker policies
- backend-specific repair/replay mechanics

must remain below the shared storage trait line or behind explicit capability
traits.

## Consequences

Positive:

- restores the originally intended architecture
- keeps SQLite and future SQL Server on one shared model through the shared
  storage contract even as concrete backend count changes over time
- shrinks the shared storage audit surface
- reduces DTO proliferation across RPC and storage
- makes daemon/runtime/backend boundaries easier to enforce mechanically

Costs:

- significant refactoring across `atm-core`, Claude storage paths, and SQLite
  backend seams
- deletion of existing request/response storage DTO families
- deletion or quarantine of speculative task-store code from the shared
  storage reset line
- migration work to converge shared types before backend extraction completes
- new crate-boundary reviews and test movement

## Mechanical Enforcement

Phase `AC` must establish durable enforcement around this ADR:

- the shared contract crate must remain small enough to audit directly
- request/response-per-operation DTO families must not be reintroduced into
  `atm-storage`
- backend crates must not depend on `atm-core`
- RPC message bodies must converge on canonical domain structs instead of
  layer-specific clones

The readiness and sprint plans in `docs/plans/phase-AC/` are the authoritative
closure gates for this ADR.

## Phase AC Implementation Split

This ADR is implemented by Phase `AC`:

- `AC.0` planning-line ADR + violation inventory freeze
- `AC.1` `atm-storage` contract and canonical domain types
- `AC.2` `atm-storage-claude` (historical; retired in `ADR-019`)
- `AC.3` SQLite backend convergence
- `AC.4` `atm-core` storage-boundary adoption
- `AC.5` RPC envelope and domain type unification for message/roster bodies
- `AC.6` cleanup and deletion closeout
- `AC.7` SQL Server readiness proof
