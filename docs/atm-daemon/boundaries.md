# ATM-Daemon Boundary Inventory

This document captures runtime-owned concrete adapters for Phase R.

Current design assumption:
- `atm-daemon` is the production runtime composition root
- `allowed_dependents: []` means no external crate should depend on these
  daemon-private concrete adapters
- Test doubles planned; not yet landed. Until they exist,
  `allowed_test_double_paths` remains empty for the daemon-owned adapter
  records below.

## SocketServerTransportAdapter

```yaml
boundary_id: BOUNDARY-ServerTransport-Socket
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: SocketServerTransportAdapter

public:
  trait: ServerTransport
  facade: null

implementation:
  type: LocalSocketServerTransport
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - listener_accept_loop
    - framed_request_receive
    - framed_response_send
  io_forbidden:
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - tokio
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - LocalSocketServerTransport
    - tokio::net::UnixListener

contracts:
  request_types:
    - AtmProtocol requests
  response_types:
    - AtmProtocol responses
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses:
    - tokio::net::UnixListener

enforcement:
  lint_rules:
    - LINT-BOUNDARY-SERVER-TRANSPORT-SOCKET-EDGES
    - LINT-BOUNDARY-SERVER-TRANSPORT-SOCKET-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_cli_to_daemon_edge

status:
  state: stub_landed
  notes:
    - thin clients must stop at ClientTransport and AtmProtocol
    - stub implementation currently lives at crate root and is assembled through atm_daemon::composition
```

Purpose:
- Owns the runtime listener implementation for the ServerTransport contract.

Notes:
- Runtime composition stays in daemon-owned code, but business logic does not.

## PeerClientTransportAdapter

```yaml
boundary_id: BOUNDARY-ClientTransport-Peer
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: PeerClientTransportAdapter

public:
  trait: ClientTransport
  facade: null

implementation:
  type: PeerClientTransport
  module: atm_daemon
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - outbound_remote_protocol_requests
    - peer_request_deadlines
    - peer_response_decode
  io_forbidden:
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - tokio
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - PeerClientTransport

contracts:
  request_types:
    - AtmProtocol requests
  response_types:
    - AtmProtocol responses
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-CLIENT-TRANSPORT-PEER-EDGES
    - LINT-BOUNDARY-CLIENT-TRANSPORT-PEER-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_cli_to_daemon_edge

status:
  state: stub_landed
  notes:
    - remote daemon-to-daemon delivery uses the same shared client transport family
    - stub implementation currently lives at crate root and is assembled through atm_daemon::composition
```

Purpose:
- Owns the daemon-side outbound client transport used for remote peer delivery.

Notes:
- This closes the design gap between the shared ClientTransport contract and daemon-to-daemon remote delivery.

## FileWatchEventSourceAdapter

```yaml
boundary_id: BOUNDARY-WatchEventSource-File
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: FileWatchEventSourceAdapter

public:
  trait: WatchEventSource
  facade: null

implementation:
  type: FileWatchEventSource
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - filesystem_watch_events
  io_forbidden:
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - FileWatchEventSource
    - notify::recommended_watcher

contracts:
  request_types:
    - watch subscription requests
  response_types:
    - watch event batches
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses:
    - notify::recommended_watcher

enforcement:
  lint_rules:
    - LINT-BOUNDARY-WATCH-EVENT-SOURCE-FILE-EDGES
    - LINT-BOUNDARY-WATCH-EVENT-SOURCE-FILE-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_watch_io_outside_boundary

status:
  state: active
  notes:
    - raw watch event capture stays runtime-private
    - crate-root daemon adapter delegates through atm_core::boundary_support and is assembled through atm_daemon::composition
```

Purpose:
- Owns the runtime file-watch implementation behind the WatchEventSource contract.

Notes:
- This adapter captures events only; it does not own reconcile policy.

## DaemonReconcileCoordinatorAdapter

```yaml
boundary_id: BOUNDARY-ReconcileCoordinator-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonReconcileCoordinatorAdapter

public:
  trait: ReconcileCoordinator
  facade: null

implementation:
  type: DaemonReconcileCoordinator
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - watch_event_coalescing
    - ingress_reconcile_triggering
  io_forbidden:
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonReconcileCoordinator

contracts:
  request_types:
    - reconcile requests
  response_types:
    - reconcile outcomes
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-RECONCILE-COORDINATOR-DAEMON-EDGES
    - LINT-BOUNDARY-RECONCILE-COORDINATOR-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_store_or_transport_bypass_in_reconcile

status:
  state: active
  notes:
    - runtime reconcile remains separate from raw watch source implementation
    - crate-root daemon adapter delegates through atm_core::boundary_support and is assembled through atm_daemon::composition
```

Purpose:
- Owns the runtime implementation of reconcile policy behind the ReconcileCoordinator contract.

Notes:
- This adapter should trigger ingress/service work without bypassing other boundaries.

## DaemonRequestDispatcherAdapter

```yaml
boundary_id: BOUNDARY-RequestDispatcher-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonRequestDispatcherAdapter

public:
  trait: RequestDispatcher
  facade: null

implementation:
  type: DaemonRequestDispatcher
  module: atm_daemon
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - runtime_request_routing
  io_forbidden:
    - sqlite
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonRequestDispatcher

contracts:
  request_types:
    - AtmProtocol requests
  response_types:
    - AtmProtocol responses
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-REQUEST-DISPATCHER-DAEMON-EDGES
    - LINT-BOUNDARY-REQUEST-DISPATCHER-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_socket_specific_business_logic

status:
  state: stub_landed
  notes:
    - dispatcher remains thin and runtime-owned
    - runtime composition will land through atm_daemon::composition when this adapter is introduced
```

Purpose:
- Owns the runtime dispatcher implementation that routes protocol requests into core services.

Notes:
- This adapter exists to keep transport loops and service logic separate.

## DaemonConfigIngressAdapter

```yaml
boundary_id: BOUNDARY-ConfigIngress-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonConfigIngressAdapter

public:
  trait: ConfigIngress
  facade: null

implementation:
  type: DaemonConfigIngress
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - persisted_config_loading
    - config_document_validation
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonConfigIngress

contracts:
  request_types:
    - config load requests
  response_types:
    - typed ATM config models
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses:
    - std::fs::read_to_string

enforcement:
  lint_rules:
    - LINT-BOUNDARY-CONFIG-INGRESS-DAEMON-EDGES
    - LINT-BOUNDARY-CONFIG-INGRESS-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_direct_config_parser_calls

status:
  state: active
  notes:
    - atm_core owns the contract; daemon runtime now supplies the crate-root adapter implementation
    - composition must continue to avoid a direct atm-daemon -> atm-rusqlite dependency
```

Purpose:
- Owns the daemon runtime adapter behind the ConfigIngress contract.

Notes:
- This adapter owns the daemon-side `ConfigIngress` implementation at the adapter boundary.

## DaemonInboxIngressAdapter

```yaml
boundary_id: BOUNDARY-InboxIngress-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonInboxIngressAdapter

public:
  trait: InboxIngress
  facade: null

implementation:
  type: DaemonInboxIngress
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - inbound_mailbox_import
    - compatibility_surface_translation
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonInboxIngress

contracts:
  request_types:
    - ingress scan requests
  response_types:
    - typed ingress result models
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses:
    - mailbox::store::observe_source_files

enforcement:
  lint_rules:
    - LINT-BOUNDARY-INBOX-INGRESS-DAEMON-EDGES
    - LINT-BOUNDARY-INBOX-INGRESS-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_direct_mailbox_helper_calls

status:
  state: active
  notes:
    - atm_core owns the contract; daemon runtime now supplies the crate-root adapter implementation
    - watcher-driven reconcile now routes through this boundary rather than directly to mailbox helpers
```

Purpose:
- Owns the daemon runtime adapter behind the InboxIngress contract.

Notes:
- This adapter owns compatibility inbox import, fingerprint, and diagnostic behavior at the daemon boundary.

## DaemonInboxExportAdapter

```yaml
boundary_id: BOUNDARY-InboxExport-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonInboxExportAdapter

public:
  trait: InboxExport
  facade: null

implementation:
  type: DaemonInboxExport
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - outbound_mailbox_projection
    - compatibility_surface_translation
  io_forbidden:
    - sqlite
    - socket_io
    - process_spawn

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonInboxExport

contracts:
  request_types:
    - export write requests
  response_types:
    - typed export result models
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses:
    - mailbox::store::with_locked_source_files

enforcement:
  lint_rules:
    - LINT-BOUNDARY-INBOX-EXPORT-DAEMON-EDGES
    - LINT-BOUNDARY-INBOX-EXPORT-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_direct_mailbox_helper_calls

status:
  state: active
  notes:
    - atm_core owns the contract; daemon runtime now supplies the crate-root adapter implementation
    - send and receive compatibility writes must stay behind this adapter rather than reaching mailbox helpers directly
```

Purpose:
- Owns the daemon runtime adapter behind the InboxExport contract.

Notes:
- This adapter owns compatibility export and write-bound projection behavior at the daemon boundary.

## Policy Placement

Compatibility and recovery policy placement for daemon-owned config/inbox adapters:

- `ConfigIngress` may own document loading, syntax validation, and translation into typed ATM config models.
- `ConfigIngress` must not own daemon auto-start policy, retained command fallback policy, or mailbox/task workflow mutation.
- `InboxIngress` may own compatibility-shape translation, identity fingerprint derivation, and ingress diagnostics over imported source files.
- `InboxIngress` must not own read/ack/clear business policy, workflow-state mutation policy, or mailbox lifecycle transitions beyond import normalization.
- `InboxExport` may own projection from ATM-owned source records back into compatibility mailbox shapes and write-bound export validation.
- `InboxExport` must not own read-path reconciliation, task-state updates, or notification/runtime policy.

## DaemonNotificationSinkAdapter

```yaml
boundary_id: BOUNDARY-NotificationSink-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonNotificationSinkAdapter

public:
  trait: NotificationSink
  facade: null

implementation:
  type: DaemonNotificationSink
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - outbound_notification_delivery
  io_forbidden:
    - sqlite
    - socket_io

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonNotificationSink
    - std::process::Command

contracts:
  request_types:
    - notification payload requests
  response_types:
    - notification delivery results
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses:
    - std::process::Command

enforcement:
  lint_rules:
    - LINT-BOUNDARY-NOTIFICATION-SINK-DAEMON-EDGES
    - LINT-BOUNDARY-NOTIFICATION-SINK-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_process_spawn_outside_notification_boundary

status:
  state: active
  notes:
    - crate-root daemon adapter delegates through atm_core::boundary_support and is assembled through atm_daemon::composition
```

Purpose:
- Owns the daemon runtime adapter behind the NotificationSink contract.

Notes:
- This keeps process-spawn or plugin-style delivery out of service logic.

## DaemonStatusSourceAdapter

```yaml
boundary_id: BOUNDARY-StatusSource-Daemon
owner_package: atm-daemon
owner_crate_path: atm_daemon
name: DaemonStatusSourceAdapter

public:
  trait: StatusSource
  facade: null

implementation:
  type: DaemonStatusSource
  module: atm_daemon
  visibility: pub(crate)
  constructor: pub(crate)

composition:
  roots:
    - atm_daemon::composition::compose_runtime

ownership:
  io_owns:
    - runtime_status_snapshots
    - status_event_delivery
  io_forbidden:
    - sqlite
    - socket_io

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon
    - atm-daemon -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - DaemonStatusSource

contracts:
  request_types:
    - status source requests
  response_types:
    - status snapshot batches
  error_types:
    - AtmError

testing:
  allowed_test_double_paths: []
  forbidden_test_bypasses: []

enforcement:
  lint_rules:
    - LINT-BOUNDARY-STATUS-SOURCE-DAEMON-EDGES
    - LINT-BOUNDARY-STATUS-SOURCE-DAEMON-REFERENCES
  review_gates:
    - no_public_impl
    - no_status_leakage_into_roster_store

status:
  state: active
  notes:
    - crate-root daemon adapter delegates through atm_core::boundary_support and is assembled through atm_daemon::composition
```

Purpose:
- Owns the daemon runtime adapter behind the StatusSource contract.

Notes:
- Durable roster truth remains separate from runtime status sourcing.
