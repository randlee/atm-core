# ATM-Daemon Boundary Inventory

This document captures runtime-owned concrete adapters for Phase R.

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
  roots: []

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
  roots: []

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
