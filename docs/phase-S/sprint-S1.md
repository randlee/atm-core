# Phase S.1 — Cross-Platform Host Boundary Extraction

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.1"
status: planned
estimated_scope: L
```

## Goal

Extract same-host transport, lifecycle control, and host ownership into
explicit daemon-owned portability boundaries so runtime orchestration no longer
depends directly on Unix APIs.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`
- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-TRANSPORT-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Hard Dependencies

- S.0 documentation hardening is accepted
- the allowed OS-difference inventory is frozen before S.1 code extraction
- the temporary Windows CI lint narrowing remains in place until S.4 removes it

## Exact Code Targets

- `crates/atm-daemon/src/composition.rs`
  - `RuntimeComposition::start`
  - `RuntimeComposition::start_with_socket_path_for_test`
  - `validate_runtime_socket_path`
  - `validate_runtime_home_dir`
  - `compose_runtime`
- `crates/atm-daemon/src/lib.rs`
  - `PreparedRuntimeServer::bind`
  - `PreparedRuntimeServer::serve_with_runtime_hooks`
  - `PreparedRuntimeServer::serve_with_deadlines_and_accept_probe`
  - `drain_active_connections_for_shutdown`
  - `handle_connection`
  - `ActiveConnectionRegistry::{register, interrupt_all, wait_for_connection_change}`
- `crates/atm-daemon/src/shutdown_signals.rs`
  - `DaemonShutdownSignals::install`

## Required Work

1. Extract a platform-neutral local-IPC adapter contract.
2. Extract a platform-neutral lifecycle-control source contract.
3. Extract a platform-neutral host-ownership contract.
4. Remove direct `UnixListener`, `UnixStream`, and signal constant references
   from composition/runtime orchestration.
5. Replace broad `#[cfg(unix)]` entrypoint gating with adapter-owned platform
   selection.
6. Limit new OS-sensitive surface area to these daemon-owned facades only:
   - `LocalIpcServerTransportAdapter`
   - `LifecycleControlSourceAdapter`
   - `HostOwnershipAdapter`

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

## Required Validation

- `just lint`
- review grep shows no new same-host `#[cfg(unix)]` outside the owned adapter
  modules
