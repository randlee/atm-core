# ADR-028 — Cross-Host Interface Control Plane

| Field | Value |
| --- | --- |
| ID | ADR-028 |
| Status | Accepted |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-026, ADR-027, Phase AG |

## Context

Early Phase AG execution proved cross-host validation could not close on the
existing surface. The daemon had no durable operator-managed control plane for
which interfaces it should bind or advertise, and env-driven peer wiring was
too ad hoc to serve as the intended product contract.

## Decision

ATM will add a SQLite-backed cross-host interface control plane managed through
CLI commands rather than environment variables as the primary operator surface.

The control plane must define:

- one row per bind/advertise interface surface
- enable/disable lifecycle
- stale/refresh handling for roaming hosts
- doctor-visible bind success/failure state

The accepted control-plane shape is:

- durable table: `daemon_peer_interfaces`
- one authoritative row key: `(interface_name, bind_addr, port)`
- CLI ownership:
  - `atm daemon interfaces add`
  - `atm daemon interfaces update`
  - `atm daemon interfaces enable`
  - `atm daemon interfaces disable`
  - `atm daemon interfaces remove`
  - `atm daemon interfaces list`
- daemon ownership:
  - load enabled rows at startup and reload
  - attempt every enabled row independently
  - persist `last_bound_at` or `last_bind_error` per row
  - leave stale/degraded rows visible instead of deleting them
- compatibility rule:
  - `ATM_DAEMON_PEER_ADDR` and config-file listener inputs remain transitional
    only when no durable interface rows are configured

## Consequences

- env-driven peer addressing remains historical/transitional only
- daemon bind behavior becomes deny-by-default when no enabled rows exist
- AG.4 closes the implementation for the interface configuration half of the
  Phase AG control plane
