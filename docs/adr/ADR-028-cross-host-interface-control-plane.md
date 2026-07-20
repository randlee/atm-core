# ADR-028 — Cross-Host Interface Control Plane

| Field | Value |
| --- | --- |
| ID | ADR-028 |
| Status | Superseded by ADR-034 |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-026, ADR-027, Phase AG |

## Context

Early Phase AG execution proved cross-host validation could not close on the
existing surface. The daemon had no durable operator-managed control plane for
which interfaces it should bind or advertise, and env-driven peer wiring was
too ad hoc to serve as the intended product contract.

## Historical proposal (retired)

Phase AG proposed a SQLite-backed cross-host interface control plane managed
through CLI commands rather than environment variables.

The control plane must define:

- one row per bind/advertise interface surface
- enable/disable lifecycle
- stale/refresh handling for roaming hosts
- doctor-visible bind success/failure state

## Consequences

- env-driven peer addressing remains historical/transitional only
- daemon bind behavior becomes deny-by-default when no enabled rows exist
- This decision is retained as historical context. ADR-034 owns the integrated
  HTTP/HTTPS interface-control contract.
