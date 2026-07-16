# ADR-030 — Cross-Host Transport Security Sequencing

| Field | Value |
| --- | --- |
| ID | ADR-030 |
| Status | Proposed |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-028, ADR-029, AG-FIND-001, Phase AG |

## Context

Current requirements/architecture describe cross-host transport as TCP/TLS,
while the implementation line remains functionally focused and not yet fully
secured. The phase needs an explicit sequencing decision so functional closure
does not implicitly claim transport-security closure.

## Decision

Phase AG will sequence transport security after the functional cross-host
control-plane and host-pair validation work.

Functional closure must not:

- imply TLS closure
- imply peer-auth closure beyond the explicit host-authorization surface
- silently downgrade the documented transport-security requirement

## Consequences

- AG.8 owns the planning/reconciliation closure for transport security
- AG.10 owns the secured-transport implementation closure
- any earlier release verdict must explicitly state whether it excludes
  transport-security guarantees

## Follow-on Remote-Send Routing Constraint

If ordinary cross-host `atm send` routing is updated after this ADR's security
work, the routing contract must stay narrow and explicit:

- the supported remote-send CLI forms are exactly:
  - `atm send <agent>@<team>.<host> ...`
  - `atm send <agent>@<team> --host <host> ...`
- those two forms normalize into one typed remote-host field on the request
- send makes one routing decision at the boundary:
  - empty remote-host field -> local mailbox path
  - non-empty remote-host field -> cross-host queue / remote-delivery boundary
- host-routing logic must not leak through general daemon/runtime code after
  that classification point
