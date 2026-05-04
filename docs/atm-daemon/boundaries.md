# ATM-Daemon Boundary Inventory

This document captures runtime-owned concrete adapters for Phase R.

Current design assumption:
- `atm-daemon` is the production runtime composition root
- direct dependency on `atm-rusqlite` is allowed for runtime assembly in the
  current design line
- `allowed_dependents: []` means no external crate should depend on these
  daemon-private concrete adapters

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
  module: atm_daemon::transport::local_socket
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::bootstrap

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
    - atm-rusqlite
    - tokio
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon

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
  allowed_test_double_paths:
    - atm_core::test_support::InProcessServerTransport
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
  state: planned
  notes:
    - thin clients must stop at ClientTransport and AtmProtocol
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
  module: atm_daemon::transport::peer_client
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::bootstrap

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
    - atm-rusqlite
    - tokio
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon

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
  allowed_test_double_paths:
    - atm_core::test_support::InProcessClientTransport
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
  state: planned
  notes:
    - remote daemon-to-daemon delivery uses the same shared client transport family
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
  module: atm_daemon::watch::source
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::bootstrap

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
    - atm-rusqlite
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon

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
  allowed_test_double_paths:
    - atm_core::test_support::StubWatchEventSource
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
  state: planned
  notes:
    - raw watch event capture stays runtime-private
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
  module: atm_daemon::watch::reconcile
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::bootstrap

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
    - atm-rusqlite
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon

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
  allowed_test_double_paths:
    - atm_core::test_support::StubReconcileCoordinator
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
  state: planned
  notes:
    - runtime reconcile remains separate from raw watch source implementation
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
  module: atm_daemon::dispatcher
  visibility: private
  constructor: private

composition:
  roots:
    - atm_daemon::bootstrap

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
    - atm-rusqlite
  forbidden_edges:
    - atm -> atm-daemon
    - atm-graft -> atm-daemon

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
  allowed_test_double_paths:
    - atm_core::test_support::StubRequestDispatcher
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
  state: planned
  notes:
    - dispatcher remains thin and runtime-owned
```

Purpose:
- Owns the runtime dispatcher implementation that routes protocol requests into core services.

Notes:
- This adapter exists to keep transport loops and service logic separate.
