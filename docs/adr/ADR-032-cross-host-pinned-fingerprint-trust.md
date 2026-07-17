# ADR-032 — Cross-Host Pinned-Fingerprint Trust Model

| Field | Value |
| --- | --- |
| ID | ADR-032 |
| Status | Accepted |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-028, ADR-029, ADR-030, ADR-031, Phase AG |

## Context

ATM cross-host transport in Phase AG uses operator-managed host pairs on a
local/VPN network. Each peer host is explicitly approved in SQLite with:

- host authorization state
- a SHA256 certificate fingerprint for the peer daemon identity

The current implementation uses rustls for channel protection and mutual
certificate presentation, but it does not yet maintain a PKI trust-anchor set,
webpki chain-building policy, or certificate-expiry enforcement model.

The repository therefore needs an explicit decision on whether the current
Phase AG line treats pinned-fingerprint trust as the accepted security model or
whether full X.509 chain validation is required before merge.

## Decision

For Phase AG, ATM secure cross-host transport accepts a pinned-fingerprint-only
trust model.

That means:

- the TLS handshake must reject a peer when the presented leaf certificate
  fingerprint does not exactly match the SQLite-approved fingerprint for that
  host
- fingerprint mismatch is treated as a validation failure, not a best-effort
  warning and not a post-handshake cleanup path
- the implementation does not claim PKI trust-anchor validation, chain
  validation, or certificate-expiry enforcement in this phase

The pinned fingerprint is the trust root for this phase. The certificate is a
host-bound capability token approved by the operator, not a browser-style PKI
credential.

## Consequences

- secure peer transport code may cite ADR-032 when intentionally using
  fingerprint-pinning instead of chain validation
- documentation and QA must describe the current model as pinned-fingerprint
  trust, not as full X.509 validation
- any future addition of trust-anchor validation, chain validation, or expiry
  enforcement is an additive hardening step layered underneath this phase's pin
  check and should supersede this ADR when adopted
- transport-security sequencing remains governed by ADR-030; ADR-032 governs
  the actual trust model for the current implementation line
