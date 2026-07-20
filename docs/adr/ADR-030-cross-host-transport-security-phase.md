# ADR-030 — Cross-Host Transport Security Sequencing

| Field | Value |
| --- | --- |
| ID | ADR-030 |
| Status | Superseded by ADR-034 |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-028, ADR-029, AG-FIND-001, Phase AG |

## Context

Current requirements/architecture describe cross-host transport as TCP/TLS,
while the implementation line remains functionally focused and not yet fully
secured. The phase needs an explicit sequencing decision so functional closure
does not implicitly claim transport-security closure.

## Historical proposal (retired)

Phase AG sequenced transport security after the functional cross-host
control-plane and host-pair validation work.

Functional closure must not:

- imply TLS closure
- imply peer-auth closure beyond the explicit host-authorization surface
- silently downgrade the documented transport-security requirement

## Consequences

- This decision is retained as historical sequencing context. ADR-034 makes
  HTTPS with authenticated peers part of the cross-host contract rather than a
  later optional layer.

The remote-target contract and dispatch-boundary decision is tracked
independently in ADR-031 so transport-security sequencing does not become the
accidental home for send-routing policy.
