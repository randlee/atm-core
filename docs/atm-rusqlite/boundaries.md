# ATM-Rusqlite Boundary Inventory

This document captures the planned concrete SQLite adapters for Phase R.

Until the final composition crate is settled, the adapter records intentionally
leave `composition.roots` empty and forbid direct dependency from caller crates.

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
  module: atm_rusqlite::mail_store
  visibility: private
  constructor: private

composition:
  roots: []

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
  state: planned
  notes:
    - composition root remains to be named by a Phase R ADR
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
  module: atm_rusqlite::task_store
  visibility: private
  constructor: private

composition:
  roots: []

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
  state: planned
  notes:
    - ack-related task transitions still resolve through send-shape workflows, not a separate public ack API
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
  module: atm_rusqlite::roster_store
  visibility: private
  constructor: private

composition:
  roots: []

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
  state: planned
  notes:
    - durable roster truth remains distinct from live status caches
```

Purpose:
- Owns the SQLite-backed implementation of the RosterStore contract.

Notes:
- Thin extensions such as atm-graft must not depend on this crate directly.
