# ATM-Rusqlite Boundary Inventory

This document captures the concrete SQLite adapters for Phase R.

Current design assumption:
- concrete sqlite adapters stay private to this crate
- no external crate should depend on `atm-rusqlite` directly
- any future runtime composition must go through boundary traits/facades rather than a direct daemon-to-sqlite crate edge

Important crate-private assembly/state-root structs that must stay visible in
review:
- `SqliteBoundaryAssembly`
  - owns composition of the three store adapters over one shared SQLite root
- `SharedDb`
  - owns connection/bootstrap/transaction policy for the shared host-scoped
    database

These are not public boundary traits, but they are important private
implementation surfaces for `R.14` and later closeout review.

## SqliteMailStoreAdapter

```yaml
boundary_id: BOUNDARY-MailStore-Sqlite
owner_package: atm-rusqlite
owner_crate_path: atm_rusqlite
name: SqliteMailStoreAdapter

public:
  trait: MailStore
  facade: null

implementation:
  type: SqliteMailStore
  module: atm_rusqlite
  visibility: private
  constructor: private

composition:
  roots:
    - atm_rusqlite::assemble_boundary

ownership:
  io_owns:
    - sqlite
    - mailbox_state_persistence
  io_forbidden:
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - rusqlite
  forbidden_edges:
    - atm -> atm-rusqlite
    - atm-daemon -> atm-rusqlite
    - atm-graft -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteMailStore
    - SqliteMailStore::open
    - rusqlite::Connection

contracts:
  request_types:
    - MailStore method inputs
  response_types:
    - atm-core mailbox DTOs
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::InMemoryMailStore
  forbidden_test_bypasses:
    - rusqlite::Connection

enforcement:
  lint_rules:
    - LINT-BOUNDARY-MAILSTORE-SQLITE-EDGES
    - LINT-BOUNDARY-MAILSTORE-SQLITE-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_public_reexport

status:
  state: active
  notes:
    - crate-root implementation remains temporary; module split is still deferred
    - assembled through atm_rusqlite::SqliteBoundaryAssembly
```

Purpose:
- Owns the SQLite-backed implementation of the MailStore contract.

Notes:
- Caller crates should know only the MailStore trait, never this concrete type.

## SqliteTaskStoreAdapter

```yaml
boundary_id: BOUNDARY-TaskStore-Sqlite
owner_package: atm-rusqlite
owner_crate_path: atm_rusqlite
name: SqliteTaskStoreAdapter

public:
  trait: TaskStore
  facade: null

implementation:
  type: SqliteTaskStore
  module: atm_rusqlite
  visibility: private
  constructor: private

composition:
  roots:
    - atm_rusqlite::assemble_boundary

ownership:
  io_owns:
    - sqlite
    - task_state_persistence
  io_forbidden:
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - rusqlite
  forbidden_edges:
    - atm -> atm-rusqlite
    - atm-daemon -> atm-rusqlite
    - atm-graft -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteTaskStore
    - SqliteTaskStore::open
    - rusqlite::Connection

contracts:
  request_types:
    - TaskStore method inputs
  response_types:
    - atm-core task DTOs
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::InMemoryTaskStore
  forbidden_test_bypasses:
    - rusqlite::Connection

enforcement:
  lint_rules:
    - LINT-BOUNDARY-TASKSTORE-SQLITE-EDGES
    - LINT-BOUNDARY-TASKSTORE-SQLITE-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_public_reexport

status:
  state: active
  notes:
    - ack-related task transitions still resolve through send-shape workflows, not a separate public ack API
    - crate-root implementation remains temporary; module split is still deferred
    - assembled through atm_rusqlite::SqliteBoundaryAssembly
```

Purpose:
- Owns the SQLite-backed implementation of the TaskStore contract.

Notes:
- This remains separate from mail and roster persistence to preserve ownership clarity.

## SqliteRosterStoreAdapter

```yaml
boundary_id: BOUNDARY-RosterStore-Sqlite
owner_package: atm-rusqlite
owner_crate_path: atm_rusqlite
name: SqliteRosterStoreAdapter

public:
  trait: RosterStore
  facade: null

implementation:
  type: SqliteRosterStore
  module: atm_rusqlite
  visibility: private
  constructor: private

composition:
  roots:
    - atm_rusqlite::assemble_boundary

ownership:
  io_owns:
    - sqlite
    - durable_roster_persistence
  io_forbidden:
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - rusqlite
  forbidden_edges:
    - atm -> atm-rusqlite
    - atm-daemon -> atm-rusqlite
    - atm-graft -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteRosterStore
    - SqliteRosterStore::open
    - rusqlite::Connection

contracts:
  request_types:
    - RosterStore method inputs
  response_types:
    - atm-core roster DTOs
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::InMemoryRosterStore
  forbidden_test_bypasses:
    - rusqlite::Connection

enforcement:
  lint_rules:
    - LINT-BOUNDARY-ROSTERSTORE-SQLITE-EDGES
    - LINT-BOUNDARY-ROSTERSTORE-SQLITE-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_public_reexport

status:
  state: active
  notes:
    - durable roster truth remains distinct from live status caches
    - crate-root implementation remains temporary; module split is still deferred
    - assembled through atm_rusqlite::SqliteBoundaryAssembly
```

Purpose:
- Owns the SQLite-backed implementation of the RosterStore contract.

Notes:
- Thin extensions such as atm-graft must not depend on this crate directly.
