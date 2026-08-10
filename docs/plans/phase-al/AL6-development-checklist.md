# AL.6 Loopback TCP Adapter — Completion Checklist

Implementation branch: `feature/pal-s6-loopback-tcp`  
Implementation commit: `ee240f7e` (this checklist is recorded in the
follow-up documentation commit)  
Sprint source: [sprint-AL6-loopback-tcp.md](./sprint-AL6-loopback-tcp.md)

This checklist is the implementation-level evidence for AL.6. It verifies the
adapter against the Phase AL rule: physical transports may establish and
authenticate a connection, but they must pass the unchanged typed request into
the one canonical Axum route. No item permits an `atm-daemon` repair or a
transport-specific decoder, route, DTO, storage call, hook path, retry, or
replay path.

## Required deliverables

- [x] **Loopback-only listener.** `LoopbackTcpConfig` rejects non-loopback
  addresses before lifecycle start and permits port zero so Tokio can select an
  available loopback port; `HttpRuntime::start` verifies and publishes the
  address returned after bind. Evidence:
  `loopback_tcp::validate_loopback_config` and
  `non_loopback_tcp_configuration_fails_before_lifecycle_start`.
- [x] **Existing endpoint-record contract.** A fresh `LocalCapability` and
  `LocalHttpEndpointRecord` are published atomically after bind. The record
  preserves IPv4 versus IPv6 address family, is owner-restricted, and its guard
  removes only the exact record generation it published. Evidence:
  `loopback_tcp::publish_loopback_endpoint_record`,
  `endpoint_record_preserves_its_loopback_address_family`, and
  `loopback_shutdown_drains_an_in_flight_canonical_request_before_record_cleanup`.
- [x] **Windows-compatible record protection.** The adapter contains the
  Windows owner-only ACL implementation behind `cfg(windows)` and declares its
  narrow `windows-sys` dependency only for Windows.
- [x] **Capability authentication before application work.** The adapter
  accepts only a loopback `ConnectInfo` peer with exactly one valid
  `X-ATM-Local-Capability` header, strips that header, and calls the canonical
  route only afterwards. Missing, incorrect, and duplicate values return the
  normal ADR-032 JSON error before `CanonicalWriteHandler`. Evidence:
  `authenticate_loopback_request` and
  `loopback_rejects_missing_and_mismatched_capability_before_handler`.
- [x] **One canonical handler.** TCP constructs the existing AL.2
  `canonical_message_router` with `AuthenticatedConnector::local`; it adds
  authentication middleware only. It creates no loopback request type, wire
  codec, recipient routing, storage call, or nudge path. Evidence:
  `al6_loopback_tcp_is_capability_authentication_over_the_one_client_and_router`.
- [x] **One shared client translation.** `LoopbackTcpConnector` reads the
  active endpoint record before every connection, adds only the capability
  header, and then calls `execute_reqwest_request` under the AL.4
  `HttpRuntimeClient`. Request encoding and response decoding remain shared.
  Evidence: `loopback_shared_client_uses_the_active_record_and_canonical_handler`.
- [x] **Stale or missing record rejection.** The shared client rejects missing
  and owner-instance-mismatched endpoint records before it attempts a
  connection, therefore before `ApiRouter`. Evidence:
  `loopback_client_rejects_a_missing_endpoint_record_before_connecting` and
  `loopback_client_rejects_a_stale_owner_record_before_connecting`.

## Required validation

- [x] **Unix parity.** The same typed write fixture produces the same status,
  headers, and JSON bytes over AL.5 UDS and AL.6 loopback TCP. Evidence:
  `loopback_and_uds_return_identical_canonical_json`.
- [x] **Admission/body limit.** An oversized TCP body is rejected before the
  canonical handler and retains the ADR-032 JSON error schema. Evidence:
  `loopback_body_limit_rejects_before_handler`.
- [x] **Graceful shutdown.** An in-flight canonical TCP request blocks drain;
  the endpoint record remains during the request and is removed after the
  request completes. Evidence:
  `loopback_shutdown_drains_an_in_flight_canonical_request_before_record_cleanup`.
- [x] **Static minimality guard.** The architecture test requires canonical
  routing, local capability authentication, endpoint-record publication, and
  the shared Reqwest path while rejecting raw framing, synchronous socket I/O,
  legacy transport code, peer arrays, resend, and replay.
- [x] **Native validation.** `cargo test -p atm-http-runtime` (54 tests),
  `cargo test -p atm-architecture --test boundary_enforcement` (45 tests),
  `just test`, and `just lint` (25 checks) passed on 2026-08-07.
- [ ] **Windows CI execution.** The Windows-gated real loopback fixture
  `windows_loopback_fixture_uses_the_same_capability_authenticated_route` is
  present. Local cross-compilation is blocked by the macOS host lacking a
  Windows C toolchain and SDK headers required by `ring`; this remains a CI
  verification gate, not a claimed local pass.

## Critical review findings and closure tasks

This review re-read `sprint-AL6-loopback-tcp.md` against the maintained
Tokio/Axum implementation. The tasks below are deliberately small and stay
inside the AL.6 physical-adapter boundary; they do not alter the shared typed
route, DTOs, storage, hook, UDS adapter, or legacy daemon.

- [x] **AL6-CR-001 — remove synchronous test-port reservation.** The initial
  implementation rejected a port-zero loopback configuration, making tests
  reserve a port with `std::net::TcpListener`, drop it, then bind it again.
  That is a needless TOCTOU race and synchronous socket setup. Tokio now binds
  port zero directly and publishes its actual assigned address; the test
  `os_selected_loopback_port_is_published_from_the_bound_listener` proves it.
- [x] **AL6-CR-002 — keep endpoint publication comprehensible.**
  `publish_loopback_endpoint_record` exceeded the repository function-length
  limit by combining validation, serialization, staging, permissions, and
  publication. It is split into named helpers with the same atomic
  create/write/sync/restrict/rename sequence.
- [x] **AL6-CR-003 — do not perform record cleanup synchronously in `Drop`.**
  The record guard previously read and removed files in `Drop`, which can run
  on a Tokio worker during shutdown. `HttpRuntime::finish` now awaits the
  guard's blocking-pool cleanup only after the Axum task has drained; a static
  architecture guard rejects restoration of that `Drop` implementation.
- [x] **AL6-CR-004 — validate the endpoint record at the client boundary.**
  A record with a matching owner id but a non-loopback address must not cause
  an outbound connection. `LoopbackTcpConnector` rejects it before Reqwest and
  `ApiRouter`; the matching negative test proves this.

## Review result

All locally verifiable AL.6 acceptance criteria and review findings are
implemented and covered. The only outstanding proof is execution of the
already-present Windows fixture in the repository Windows CI lane. No legacy
daemon code was changed.
