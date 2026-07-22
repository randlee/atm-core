# Phase S.1 — Cross-Platform Host Boundary Extraction

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.1"
status: complete
estimated_scope: L
```

## Goal

Extract same-host transport, lifecycle control, and host ownership into
explicit daemon-owned portability boundaries so runtime orchestration no longer
depends directly on Unix APIs.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-P-TEST-001`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`
- `REQ-DAEMON-TRANSPORT-008`
- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-TRANSPORT-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`

## Governing ICD Sections

- `docs/atm-daemon/protocol-icd.md §5` shared ATM frame
- `docs/atm-daemon/protocol-icd.md §6` packet kind registry
- `docs/atm-daemon/protocol-icd.md §8` exchange rules
- `docs/atm-daemon/protocol-icd.md §10` timeout and failure semantics

## Hard Dependencies

- S.0 documentation hardening is accepted
- the allowed OS-difference inventory is frozen before S.1 code extraction
- the temporary Windows CI lint narrowing remains in place until S.4 removes it

## Exact Code Targets

- `crates/atm-core/src/protocol.rs`
  - `FramePayload`
  - `read_bounded_stream`
  - daemon frame/path helpers that currently encode Unix socket assumptions
- `crates/atm-core/src/boundary/mod.rs`
  - `AtmProtocol`
  - `ClientTransport`
  - `ServerTransport`
- `crates/atm-daemon/src/composition.rs`
  - `RuntimeComposition::start`
  - `RuntimeComposition::start_with_socket_path_for_test`
  - `validate_runtime_home_dir`
  - `compose_runtime`
- same-host endpoint validation is currently split between:
  - `crates/atm/src/composition.rs::DaemonLocalIpcEndpoint::new`
  - `crates/atm-core/src/protocol.rs::daemon_local_ipc_name_from_path`
- `crates/atm-daemon/src/lib.rs`
  - runtime crate-root ownership and adapter re-exports only
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - `PreparedRuntimeServer::bind`
  - `PreparedRuntimeServer::serve_with_runtime_hooks`
  - `PreparedRuntimeServer::serve_with_deadlines_and_accept_probe`
  - `drain_active_connections_for_shutdown`
  - `handle_connection`
  - `ActiveConnectionRegistry::{register, interrupt_all, wait_for_connection_change}`
- `crates/atm-daemon/src/lifecycle_control.rs`
  - `LifecycleControlSourceAdapter::install`
- `crates/atm-daemon/src/host_ownership.rs`
  - `HostOwnershipAdapter::{acquire, acquire_at}`
  - `host_runtime_lock_path`
- `crates/atm/src/composition.rs`
  - `LocalIpcClientTransportAdapter::{try_connect, exchange}`
  - `resolve_daemon_local_ipc_endpoint`

## Required Work

1. Extract a platform-neutral local-IPC adapter contract.
2. Extract a platform-neutral lifecycle-control source contract.
3. Extract a platform-neutral host-ownership contract.
4. Remove direct `UnixListener`, `UnixStream`, and signal constant references
   from composition/runtime orchestration.
4.1 Replace EOF-delimited framing with the ICD-framed transport contract from:
   - `protocol-icd.md §5`
   - `protocol-icd.md §10`
5. Replace broad `#[cfg(unix)]` entrypoint gating with adapter-owned platform
   selection.
6. Limit new OS-sensitive surface area to these daemon-owned facades only:
   - `LocalIpcServerTransportAdapter`
   - `LifecycleControlSourceAdapter`
   - `HostOwnershipAdapter`
7. Move logical endpoint naming and same-user access-control policy behind the
   local-IPC adapter instead of leaving platform endpoint details in
   callers.

## Required Document Updates

- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- machine-readable boundary records under `boundaries/atm-daemon/`

## Acceptance Criteria

- runtime orchestration above the adapter layer no longer depends directly on
  Unix listener/stream or signal types
- the only allowed OS-specific seams are the documented portability
  boundaries
- S.1 does not introduce new public cross-crate OS-specific traits or helper
  types outside the three documented daemon-owned portability facades
- any remaining unsupported-path stub is temporary, explicitly documented, and
  limited to the still-unimplemented adapter
- any new same-host daemon tests added by S.1 are bounded, explicit about
  readiness predicates, and do not introduce unbounded wait or panic-stranded
  shared-hook behavior

## Required Validation

- `just lint`
- review grep shows no new same-host `#[cfg(unix)]` outside the owned adapter
  modules
