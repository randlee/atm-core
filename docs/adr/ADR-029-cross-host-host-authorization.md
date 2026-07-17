# ADR-029 — Cross-Host Host Authorization

| Field | Value |
| --- | --- |
| ID | ADR-029 |
| Status | Proposed |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-028, Phase AG, AG-FIND-004 |

## Context

Early AG execution and the loopback follow-up review proved inbound cross-host
daemon traffic lacked a durable functional trust gate. The product needs a
pre-security authorization layer before mailbox mutation so real host-pair
validation can close meaningfully.

## Decision

ATM will use a SQLite-backed deny-by-default exact-host allowlist as the
functional inbound authorization surface for cross-host daemon traffic.

The policy must define:

- exact-hostname-only matching
- no wildcards, subnet trust, prefix/suffix matching, or regex matching
- enforcement before mailbox, ack, or roster mutation
- doctor-visible enabled/disabled host state

## Consequences

- AG-FIND-004 is closed through this control plane, not through ad hoc runtime
  exceptions
- future transport security can layer on top of this authorization surface but
  does not replace the need for the explicit host policy
- AG.5 owns the implementation closure for this ADR
