# ADR-030 — Cross-Host Transport Security Sequencing

| Field | Value |
| --- | --- |
| ID | ADR-030 |
| Status | Accepted |
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

For the current Phase AG implementation line, ATM secure cross-host transport
uses a deliberate pinned-fingerprint trust model:

- each allowed remote host is paired with an explicitly approved SHA256 peer
  certificate fingerprint stored in SQLite
- rustls verifier callbacks must reject the TLS handshake when the presented
  peer certificate fingerprint does not match the approved row for that host
- this model does not perform PKI trust-anchor validation or certificate-expiry
  enforcement yet

That fingerprint-only model is accepted for this phase because ATM's current
cross-host deployment target is operator-managed host pairs on a local/VPN
network, where explicit peer approval is the primary trust primitive and the
certificate acts as a pinned capability token rather than a browser-style PKI
credential.

## Consequences

- AG.8 owns the planning/reconciliation closure for transport security
- AG.10 owns the secured-transport implementation closure
- any earlier release verdict must explicitly state whether it excludes
  transport-security guarantees
- code implementing secure peer transport must cite this ADR when intentionally
  using pinned-fingerprint trust instead of chain validation
- future work can layer chain validation and expiry checks underneath the pin,
  but until that lands the repository must not describe the current model as
  full X.509 chain validation

The remote-target contract and dispatch-boundary decision is tracked
independently in ADR-031 so transport-security sequencing does not become the
accidental home for send-routing policy.
