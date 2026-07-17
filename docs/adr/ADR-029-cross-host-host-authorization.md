# ADR-029 — Cross-Host Host Authorization

| Field | Value |
| --- | --- |
| ID | ADR-029 |
| Status | Accepted |
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

- exact-host-only matching after one canonical lowercase normalization
- the current transport's presented host token is the remote socket IP literal
  rather than reverse-DNS output
- no wildcards, subnet trust, prefix/suffix matching, or regex matching
- enforcement before mailbox, ack, or roster mutation
- doctor-visible enabled/disabled host state

## Consequences

- AG-FIND-004 is closed through this control plane, not through ad hoc runtime
  exceptions; the remaining AG.6 work is doctor projection, not additional
  trust logic
- future transport security can layer on top of this authorization surface but
  does not replace the need for the explicit host policy
- the loopback self-test path remains subject to this same authorization gate,
  so `peer_loopback_delivery` does not bypass remote-host admission
- AG.5 owns the implementation closure for this ADR
