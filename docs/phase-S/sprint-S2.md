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
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`

## Governing ADRs

- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Hard Dependencies

- S.1 boundary extraction is complete
- the Windows hosting model decision remains the user-scoped same-host daemon
  model recorded in S.0
- the temporary Windows CI lint narrowing remains in place until S.4 removes it

## Exact Code Targets

- `crates/atm-daemon/src/lib.rs`
  - imports and concrete fields that currently depend on `UnixListener` and
    `UnixStream`
  - `PreparedRuntimeServer`
  - `RuntimeServerTransport::prepare_runtime`
  - `RuntimeServerTransport::prepare_runtime_at_socket_path`
  - `remove_stale_socket`
  - `handle_connection`
- `crates/atm-daemon/src/tests.rs`
  - replace Unix-only same-host transport tests with shared harness coverage

## Required Work

1. Implement the Windows side of the local-IPC adapter using the Phase S crate
   decision.
2. Move Unix socket path semantics fully into the Unix adapter instead of
   leaving them in shared composition/runtime code.
3. Replace non-Unix same-host runtime stubs with real Windows transport
   behavior.
4. Keep request framing, deadlines, and typed error mapping identical across
   Unix and Windows.
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
  shutdown are proven through explicit synchronization or bounded observable
  state transitions

## Required Validation

- `just lint`
- workspace tests
- same-host functional tests on Unix and Windows CI
