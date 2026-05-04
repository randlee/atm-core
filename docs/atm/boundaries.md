# ATM Boundary Inventory

This document captures CLI-owned concrete adapters for Phase R.

## LocalSocketClientTransportAdapter

```yaml
boundary_id: BOUNDARY-ClientTransport-CLI
owner_package: atm
owner_crate_path: atm
name: LocalSocketClientTransportAdapter

public:
  trait: ClientTransport
  facade: null

implementation:
  type: LocalSocketClientTransport
  module: atm::transport::local_socket
  visibility: private
  constructor: private

composition:
  roots: []

ownership:
  io_owns:
    - outbound_local_socket_requests
    - client_request_deadlines
    - response_decode
  io_forbidden:
    - sqlite
    - process_spawn_for_notifications

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - tokio
  forbidden_edges:
    - atm -> atm-daemon
    - atm -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - LocalSocketClientTransport
    - atm_daemon::client
    - rusqlite::Connection

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
  forbidden_test_bypasses:
    - atm_daemon::client
    - rusqlite::Connection

enforcement:
  lint_rules:
    - LINT-BOUNDARY-CLIENT-TRANSPORT-CLI-EDGES
    - LINT-BOUNDARY-CLIENT-TRANSPORT-CLI-REFERENCES
  review_gates:
    - no_public_impl
    - no_public_constructor
    - no_cli_to_daemon_edge
    - no_cli_to_sqlite_edge

status:
  state: planned
  notes:
    - thin extension clients such as atm-graft may reuse the contract shape without depending on atm internals
```

Purpose:
- Owns the CLI-local implementation of the ClientTransport contract.

Notes:
- The CLI stays thin: parse, map request, call transport, render response.
