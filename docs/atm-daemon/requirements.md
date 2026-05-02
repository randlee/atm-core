# ATM-Daemon Crate Requirements

## 1. Purpose

This document defines the `atm-daemon` crate requirements.

The `atm-daemon` crate owns the runtime wrapper around the Phase Q ATM system.
Product behavior remains defined in [`../requirements.md`](../requirements.md).
`atm-daemon` must satisfy those product requirements without re-owning
`atm-core` business logic.

This crate is introduced by the Phase Q implementation line. It is not present
in the pre-Phase-Q workspace yet.

## 2. Ownership

`atm-daemon` owns:

- singleton daemon startup and host ownership
- same-host daemon API transport
- cross-host daemon-to-daemon transport
- runtime composition of `atm-core` service boundaries
- live agent status cache
- runtime watch/reconcile loop if enabled
- daemon-side `sc-observability` emission

`atm-daemon` does not own:

- mail business logic
- workflow/state-machine rules
- direct CLI parsing or rendering
- direct ownership of SQLite semantics beyond using the `atm-core` store
  boundary

## 3. Requirement Namespace

The `atm-daemon` crate uses the `REQ-DAEMON-*` namespace.

Initial allocation:

- `REQ-DAEMON-RUNTIME-*`
- `REQ-DAEMON-TRANSPORT-*`
- `REQ-DAEMON-STATUS-*`
- `REQ-DAEMON-TEST-*`
- `REQ-DAEMON-OBS-*`

Initial crate requirement IDs:

- `REQ-DAEMON-RUNTIME-001` `atm-daemon` owns singleton runtime enforcement and
  must make it impossible for two active daemons to run on one host. Satisfies
  the runtime ownership aspects of:
  `REQ-CORE-DAEMON-001`, `REQ-CORE-QA-RUNTIME-001`.
- `REQ-DAEMON-RUNTIME-002` `atm-daemon` owns runtime composition only and must
  remain a thin wrapper over `atm-core` service boundaries. Satisfies:
  `REQ-CORE-DAEMON-002`, `REQ-CORE-BOUNDARY-001`.
- `REQ-DAEMON-TRANSPORT-001` `atm-daemon` owns one protocol with two transport
  implementations:
  - Unix domain socket for same-host
  - TCP/TLS for cross-host daemon-to-daemon traffic
  Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`.
- `REQ-DAEMON-TRANSPORT-002` `atm-daemon` owns bounded transient retry for
  remote delivery and must not create a durable long-lived remote outbox.
  Satisfies:
  `REQ-CORE-TRANSPORT-003`, `REQ-CORE-TRANSPORT-004`.
- `REQ-DAEMON-STATUS-001` `atm-daemon` owns the live agent-status cache and
  must keep it separate from SQLite roster/mail truth. Satisfies:
  `REQ-CORE-RUNTIME-002`.
- `REQ-DAEMON-TEST-001` `atm-daemon` must not define the core test strategy.
  Core correctness must remain testable without daemon process spawning.
  Satisfies:
  `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-DAEMON-OBS-001` `atm-daemon` owns daemon/runtime/transport structured
  event emission through `sc-observability`. Satisfies:
  `REQ-CORE-OBS-002`.
- `REQ-DAEMON-HEALTH-001` `atm-daemon` owns the daemon health interface
  consumed by `atm doctor`. Satisfies:
  `REQ-CORE-DOCTOR-002`.

## 4. Required References

The `atm-daemon` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-Q.md`](../plan-phase-Q.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)

## 5. Phase Q Runtime Requirements

Requirement IDs:
- `REQ-DAEMON-RUNTIME-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-DAEMON-TRANSPORT-001`
- `REQ-DAEMON-TRANSPORT-002`
- `REQ-DAEMON-STATUS-001`
- `REQ-DAEMON-TEST-001`
- `REQ-DAEMON-OBS-001`
- `REQ-DAEMON-HEALTH-001`

Required runtime rules:
- exactly one daemon may be active on a host at a time
- daemon startup must fail deterministically if a live daemon already owns the
  runtime
- stale ownership cleanup must never allow two live daemons
- remote delivery must be daemon-to-daemon only
- daemon memory is the live truth for agent status
- daemon code must not bypass `atm-core` subsystem boundaries
- daemon tests must not become the normal mechanism for validating core ATM
  correctness
- daemon runtime failures must remain typed across transport/runtime boundaries
  rather than collapsing into panic/unwrap control flow
- daemon runtime and transport paths must emit structured observability events
- daemon must expose one explicit health/status query interface for `atm doctor`
