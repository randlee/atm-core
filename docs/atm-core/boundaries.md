# ATM-Core Boundary Inventory

This document is the initial Phase R boundary inventory for `atm-core`.

Contract-owner records intentionally use:
- `implementation.type: null`
- `implementation.module: null`
- `implementation.visibility: trait_only`
- `implementation.constructor: none`

That means `atm-core` owns the public contract and semantics, but not a default
concrete adapter in this crate.

## AtmProtocol

```yaml
boundary_id: BOUNDARY-AtmProtocol
owner_package: atm-core
owner_crate_path: atm_core
name: AtmProtocol

public:
  trait: AtmProtocol
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - protocol_request_types
    - protocol_response_types
    - frame_payload_contract
  io_forbidden:
    - socket_io
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents:
    - atm
    - atm-daemon
    - atm-graft
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: global
  forbidden:
    - daemon_api
    - DaemonRequestEnvelope
    - DaemonResponseEnvelope

contracts:
  request_types:
    - AtmRequestEnvelope
  response_types:
    - AtmResponseEnvelope
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-ATM-PROTOCOL-NAMING
  review_gates:
    - no_daemon_shaped_protocol_types

status:
  state: planned
  notes:
    - ack is represented inside send-shape request data, not as a top-level protocol family
```

Purpose:
- Owns the shared ATM request/response contract used by local CLI, daemon, and
  thin extension clients.

Notes:
- This is the first boundary updated explicitly for `atm-graft`.

## ClientTransport

```yaml
boundary_id: BOUNDARY-ClientTransport
owner_package: atm-core
owner_crate_path: atm_core
name: ClientTransport

public:
  trait: ClientTransport
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - outbound_protocol_requests
    - request_deadlines
    - response_decoding
  io_forbidden:
    - sqlite
    - inbox_jsonl
    - process_spawn

dependencies:
  allowed_dependents:
    - atm
    - atm-daemon
    - atm-graft
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - atm_daemon::client

contracts:
  request_types:
    - AtmRequestEnvelope
  response_types:
    - AtmResponseEnvelope
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::InProcessClientTransport
  forbidden_test_bypasses:
    - atm_daemon::client

enforcement:
  lint_rules:
    - LINT-BOUNDARY-CLIENT-TRANSPORT-REFERENCES
  review_gates:
    - no_cli_to_daemon_internal_edge

status:
  state: planned
  notes:
    - designed for thin callers such as atm-graft
    - daemon-to-daemon remote delivery also depends on this outbound client boundary
```

Purpose:
- Owns the outbound ATM request path from thin clients into a server/runtime.

Notes:
- The public workflow surface above this boundary should stay centered on send
  and receive.

## WatchEventSource

```yaml
boundary_id: BOUNDARY-WatchEventSource
owner_package: atm-core
owner_crate_path: atm_core
name: WatchEventSource

public:
  trait: WatchEventSource
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - filesystem_watch_events
    - watch_event_delivery
  io_forbidden:
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - notify::recommended_watcher

contracts:
  request_types:
    - watch subscription requests
  response_types:
    - watch event batches
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubWatchEventSource
  forbidden_test_bypasses:
    - notify::recommended_watcher

enforcement:
  lint_rules:
    - LINT-BOUNDARY-WATCH-EVENT-SOURCE-REFERENCES
  review_gates:
    - no_watch_io_outside_boundary

status:
  state: planned
  notes:
    - watch source owns event capture only, not reconcile policy
```

Purpose:
- Owns filesystem watch event capture and delivery to the runtime reconcile layer.

Notes:
- This keeps raw watch APIs out of store, transport, and service logic.

## ReconcileCoordinator

```yaml
boundary_id: BOUNDARY-ReconcileCoordinator
owner_package: atm-core
owner_crate_path: atm_core
name: ReconcileCoordinator

public:
  trait: ReconcileCoordinator
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - watch_event_coalescing
    - ingress_reconcile_triggering
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - mailbox::store::observe_source_files
    - mailbox::store::with_locked_source_files

contracts:
  request_types:
    - reconcile requests
  response_types:
    - reconcile outcomes
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubReconcileCoordinator
  forbidden_test_bypasses:
    - mailbox::store::observe_source_files

enforcement:
  lint_rules:
    - LINT-BOUNDARY-RECONCILE-COORDINATOR-REFERENCES
  review_gates:
    - no_store_or_transport_bypass_in_reconcile

status:
  state: planned
  notes:
    - reconcile owns coalescing and trigger policy, not raw watch APIs
```

Purpose:
- Owns watch-driven reconcile policy and ingress triggering above raw watch events.

Notes:
- This closes the missing watch/reconcile boundary gap in the initial Phase R set.

## ServerTransport

```yaml
boundary_id: BOUNDARY-ServerTransport
owner_package: atm-core
owner_crate_path: atm_core
name: ServerTransport

public:
  trait: ServerTransport
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - listener_accept_loop
    - framed_request_receive
    - framed_response_send
  io_forbidden:
    - sqlite
    - inbox_jsonl
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - rusqlite::Connection

contracts:
  request_types:
    - AtmRequestEnvelope
  response_types:
    - AtmResponseEnvelope
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::InProcessServerTransport
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-SERVER-TRANSPORT-REFERENCES
  review_gates:
    - no_server_business_logic

status:
  state: planned
  notes:
    - server transports stay runtime-only and are not exposed to thin client crates
```

Purpose:
- Owns inbound ATM request serving and response framing for runtime hosts.

Notes:
- Listener/runtime code should remain thin and dispatch through RequestDispatcher.

## RequestDispatcher

```yaml
boundary_id: BOUNDARY-RequestDispatcher
owner_package: atm-core
owner_crate_path: atm_core
name: RequestDispatcher

public:
  trait: RequestDispatcher
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - protocol_request_routing
  io_forbidden:
    - socket_io
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - tokio::net::TcpListener
    - tokio::net::UnixListener

contracts:
  request_types:
    - AtmProtocol requests
  response_types:
    - AtmProtocol responses
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubRequestDispatcher
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-DISPATCHER-REFERENCES
  review_gates:
    - no_socket_specific_dispatch_logic

status:
  state: planned
  notes:
    - dispatcher is a service boundary, not a socket adapter
```

Purpose:
- Owns routing of typed protocol requests to the correct service handlers.

Notes:
- Transport-specific listeners should not embed request-family logic.

## MailStore

```yaml
boundary_id: BOUNDARY-MailStore
owner_package: atm-core
owner_crate_path: atm_core
name: MailStore

public:
  trait: MailStore
  facade: null
  notes: planned trait name — not yet landed (Wave 2, R.4)

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - message_lifecycle_state
    - mailbox_projection_state
  io_forbidden:
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-rusqlite
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteMailStore
    - RusqliteStore
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
    - LINT-BOUNDARY-MAILSTORE-REFERENCES
  review_gates:
    - no_concrete_store_leakage

status:
  state: deferred
  notes:
    - Wave 2 (R.4) — implementation sprint not yet scheduled
    - mail state remains distinct from task and roster state
```

Purpose:
- Owns durable message lifecycle and mailbox-facing state.

Notes:
- This stays the canonical durable truth behind send and receive workflows.

## TaskStore

```yaml
boundary_id: BOUNDARY-TaskStore
owner_package: atm-core
owner_crate_path: atm_core
name: TaskStore

public:
  trait: TaskStore
  facade: null
  notes: planned trait name — not yet landed (Wave 2, R.4)

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - task_state
    - task_message_linkage
  io_forbidden:
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-rusqlite
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteTaskStore
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
    - LINT-BOUNDARY-TASKSTORE-REFERENCES
  review_gates:
    - no_concrete_store_leakage

status:
  state: deferred
  notes:
    - Wave 2 (R.4) — implementation sprint not yet scheduled
    - ack-specific state changes belong here even when ack is modeled through send
```

Purpose:
- Owns durable task-domain state and task/message linkage.

Notes:
- `ack` is not a top-level public method, but it still mutates task state.

## RosterStore

```yaml
boundary_id: BOUNDARY-RosterStore
owner_package: atm-core
owner_crate_path: atm_core
name: RosterStore

public:
  trait: RosterStore
  facade: null
  notes: planned trait name — not yet landed (Wave 2, R.4)

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - durable_roster_state
    - member_routing_metadata
  io_forbidden:
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-rusqlite
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteRosterStore
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
    - LINT-BOUNDARY-ROSTERSTORE-REFERENCES
  review_gates:
    - no_concrete_store_leakage

status:
  state: deferred
  notes:
    - Wave 2 (R.4) — implementation sprint not yet scheduled
    - live status belongs elsewhere; this boundary owns durable roster truth only
```

Purpose:
- Owns durable roster state and routing-relevant member metadata.

Notes:
- Runtime status remains outside durable roster ownership.

## ConfigIngress

```yaml
boundary_id: BOUNDARY-ConfigIngress
owner_package: atm-core
owner_crate_path: atm_core
name: ConfigIngress

public:
  trait: ConfigIngress
  facade: null
  notes: planned trait name — not yet landed (Wave 2, R.4)

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - persisted_config_loading
    - config_document_validation
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - load_team_config
    - std::fs::read_to_string

contracts:
  request_types:
    - config load requests
  response_types:
    - typed ATM config models
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubConfigIngress
  forbidden_test_bypasses:
    - std::fs::read_to_string

enforcement:
  lint_rules:
    - LINT-BOUNDARY-CONFIG-INGRESS-REFERENCES
  review_gates:
    - no_direct_config_parser_calls

status:
  state: deferred
  notes:
    - Wave 2 (R.4) — implementation sprint not yet scheduled
    - this boundary replaces direct command/service calls into a concrete parser module
```

Purpose:
- Owns loading and validating persisted ATM/team configuration into typed models.

Notes:
- This is one of the main explicit corrections to the Phase Q leakage.

## InboxIngress

```yaml
boundary_id: BOUNDARY-InboxIngress
owner_package: atm-core
owner_crate_path: atm_core
name: InboxIngress

public:
  trait: InboxIngress
  facade: null
  notes: planned trait name — not yet landed (Wave 2, R.4)

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - inbound_mailbox_import
    - compatibility_surface_translation
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - mailbox::store::observe_source_files

contracts:
  request_types:
    - ingress scan requests
  response_types:
    - typed ingress result models
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubInboxIngress
  forbidden_test_bypasses:
    - mailbox::store::observe_source_files

enforcement:
  lint_rules:
    - LINT-BOUNDARY-INBOX-INGRESS-REFERENCES
  review_gates:
    - no_direct_mailbox_helper_calls

status:
  state: deferred
  notes:
    - Wave 2 (R.4) — implementation sprint not yet scheduled
    - watcher-driven reconcile should call this boundary rather than store helpers directly
```

Purpose:
- Owns import from compatibility/shared inbox surfaces into ATM-owned state.

Notes:
- The import path stays separate from durable store ownership.

## InboxExport

```yaml
boundary_id: BOUNDARY-InboxExport
owner_package: atm-core
owner_crate_path: atm_core
name: InboxExport

public:
  trait: InboxExport
  facade: null
  notes: planned trait name — not yet landed (Wave 2, R.4)

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - outbound_mailbox_projection
    - compatibility_surface_translation
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - mailbox::store::with_locked_source_files
    - mailbox::store::commit_source_files

contracts:
  request_types:
    - export write requests
  response_types:
    - typed export result models
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubInboxExport
  forbidden_test_bypasses:
    - mailbox::store::with_locked_source_files

enforcement:
  lint_rules:
    - LINT-BOUNDARY-INBOX-EXPORT-REFERENCES
  review_gates:
    - no_direct_mailbox_helper_calls

status:
  state: deferred
  notes:
    - Wave 2 (R.4) — implementation sprint not yet scheduled
    - send and receive state transitions should reach compatibility files through this boundary only
```

Purpose:
- Owns projection of ATM-owned state back to compatibility/shared inbox surfaces.

Notes:
- This is the write-facing sibling of InboxIngress, not a general store boundary.

## NotificationSink

```yaml
boundary_id: BOUNDARY-NotificationSink
owner_package: atm-core
owner_crate_path: atm_core
name: NotificationSink

public:
  trait: NotificationSink
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - outbound_notification_delivery
  io_forbidden:
    - sqlite
    - socket_io

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden:
    - std::process::Command
    - maybe_run_post_send_hook

contracts:
  request_types:
    - notification payload requests
  response_types:
    - notification delivery results
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubNotificationSink
  forbidden_test_bypasses:
    - std::process::Command

enforcement:
  lint_rules:
    - LINT-BOUNDARY-NOTIFICATION-SINK-REFERENCES
  review_gates:
    - no_direct_process_spawn

status:
  state: planned
  notes:
    - a thin extension crate should never need to reach into process-spawn internals
```

Purpose:
- Owns outward delivery of notifications, hooks, or plugin-facing events.

Notes:
- This replaces direct `Command::new` use in business-flow code.

## StatusSource

```yaml
boundary_id: BOUNDARY-StatusSource
owner_package: atm-core
owner_crate_path: atm_core
name: StatusSource

public:
  trait: StatusSource
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - inbound_runtime_status_updates
  io_forbidden:
    - sqlite
    - socket_io

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies: []
  forbidden_edges: []

references:
  scope: outside_owner_crate
  forbidden: []

contracts:
  request_types:
    - status update notifications
  response_types:
    - status snapshots
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::StubStatusSource
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-STATUS-SOURCE-REFERENCES
  review_gates:
    - no_status_leakage_into_roster_store

status:
  state: planned
  notes:
    - live status remains separate from durable roster truth
```

Purpose:
- Owns inbound runtime status/activity updates before they become ATM-visible state.

Notes:
- This stays distinct from `RosterStore` to avoid durable/live-state collapse.
