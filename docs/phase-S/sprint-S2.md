# Phase S.2 — Windows Local IPC Implementation

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.2"
status: planned
estimated_scope: L
```

## Goal

Implement real Windows same-host daemon transport behind the local-IPC
boundary while preserving one shared protocol, dispatcher, and test harness.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-DAEMON-TRANSPORT-001`
- `REQ-DAEMON-TRANSPORT-008`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`

## Governing ADRs

- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Governing ICD Sections

- `docs/atm-daemon/protocol-icd.md §5` shared ATM frame
- `docs/atm-daemon/protocol-icd.md §6.5` packet-kind to workflow mapping
- `docs/atm-daemon/protocol-icd.md §8.2` same-host local IPC
- `docs/atm-daemon/protocol-icd.md §10` timeout and failure semantics
- `docs/atm-daemon/protocol-icd.md §14` test and reuse rules

## Hard Dependencies

- S.1 boundary extraction is complete
- the Windows hosting model decision remains the user-scoped same-host daemon
  model recorded in S.0
- the temporary Windows CI lint narrowing remains in place until S.4 removes it

## Exact Code Targets

- `crates/atm-core/src/protocol.rs`
  - `daemon_socket_path`
  - `daemon_local_ipc_name`
  - `daemon_local_ipc_name_from_path`
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - `PreparedRuntimeServer::bind`
  - `PreparedRuntimeServer::serve_with_deadlines_and_accept_probe`
  - `LocalIpcServerTransportAdapter::{prepare_runtime, prepare_runtime_at_socket_path}`
  - `prepare_local_ipc_endpoint`
  - `handle_connection`
- `crates/atm-daemon/src/tests.rs`
  - keep daemon-private tests transport-neutral above the local-IPC adapter
- `crates/atm/src/composition.rs`
  - `LocalSocketClientTransport`
  - `DaemonLocalIpcEndpoint`
  - `LaunchGateGuard`
- `crates/atm-daemon/tests/run_daemon_production_path.rs`
  - shared same-host production-path coverage through the real local IPC path

## Required Work

1. Implement the Windows side of the local-IPC adapter using the Phase S crate
   decision.
2. Move Unix socket path semantics fully into the Unix adapter instead of
   leaving them in shared composition/runtime code.
3. Replace non-Unix same-host runtime stubs with real Windows transport
   behavior.
4. Keep request framing, deadlines, and typed error mapping identical across
   Unix and Windows.
4.1 Preserve the ICD packet family exactly:
   - no local-only packet kinds
   - no local-only header variant
   - no local-only error response shape
4.2 Preserve one logical endpoint contract and same-user access-control policy
   across Unix and Windows even though the adapter internals differ.
5. Add shared same-host functional coverage that runs the real local-IPC path
   on both platform families.
6. Use the S.0 anti-flake synchronization contract for Windows and Unix
   transport tests.

## Acceptance Criteria

- Windows same-host daemon hosting is real, not `daemon_unavailable(...)`
- request/response framing and dispatcher behavior are shared across Unix and
  Windows
- same-host functional tests prove the real transport on Windows through the
  shared harness
- no fixed sleeps are used to stabilize the transport tests; readiness and
  shutdown are proven through explicit synchronization such as channel
  handshakes, `Barrier`, or `Condvar` predicates, following the contract in
  `docs/testing-guidelines.md §5`

## Required Validation

- `just lint`
- workspace tests
- same-host functional tests on Unix and Windows CI
