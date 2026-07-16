# ADR-030 — Cross-Host Transport Security Sequencing

| Field | Value |
| --- | --- |
| ID | ADR-030 |
| Status | Accepted |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-028, ADR-029, AG-FIND-001, Phase AG |

## Context

Current requirements and architecture describe cross-host transport as
TCP/TLS, but the active `1.3.1` implementation line still uses plain TCP peer
transport. Phase AG now has the functional control-plane pieces needed to make
cross-host delivery operable (`AG.4` interface control, `AG.5` host allowlist,
`AG.6` doctor visibility, and `AG.7` code-path/harness closure), but AG.7's
real Windows/macOS or Windows/Mac-Studio live host-pair evidence is still
pending.

The phase therefore needs an explicit decision covering three separate truths:

- functional cross-host work can advance before transport security is complete
- functional closure must not be misreported as encryption or peer-auth closure
- the secured transport implementation needs a concrete, reviewable direction
  rather than a generic "TLS later" placeholder

## Decision

Phase AG will keep transport security as a separate late sprint concern.

- AG.8 owns the documentation, readiness, and implementation-plan
  reconciliation for transport security.
- AG.10 owns the secured transport implementation.
- No AG sprint before AG.10 may claim TLS, encryption, certificate validation,
  or authenticated peer-identity closure.
- Any release-usable verdict issued before AG.10 passes must explicitly state
  that cross-host is, at most, functionally usable and not transport-secure.

## Accepted Security Direction For AG.10

AG.10 will implement one explicit secure transport mode plus one explicit
fallback mode.

### Secure mode

- use TLS for daemon-to-daemon transport
- generate a local self-signed daemon identity certificate on demand when no
  daemon certificate exists yet
- persist the local daemon certificate and private key in daemon-owned durable
  state
- require explicit peer trust material before a remote peer is treated as
  authenticated
- allow operators to approve trust exchange rather than requiring manual file
  surgery
- reject unauthorized or untrusted peers before mailbox mutation
- keep the existing durable host allowlist as an additional gate; transport
  security does not replace the allowlist

### Insecure mode

- an explicit no-certificate / no-TLS mode may still exist for controlled
  development or operator-directed fallback
- insecure mode must be opt-in and visible in doctor/runtime status
- insecure mode must not silently activate because secure negotiation failed

### Trust exchange direction

- the product should support certificate sharing with explicit user approval
  instead of assuming manual certificate distribution as the only workflow
- the approval step is the trust boundary; automatic trust without approval is
  forbidden

## Non-Goals

This ADR does not authorize AG.8 to:

- implement TLS in this sprint
- claim that AG.7 live host-pair validation is already complete
- waive or downgrade the documented transport-security requirement
- replace the durable host allowlist with certificate trust alone
- silently fall back from secure mode to insecure mode on handshake failure

## Consequences

- `AG-FIND-001` remains open until AG.10 lands and the secure smoke/integration
  rows pass
- current docs must say plainly that the shipped line is plain TCP today
- current docs must also say plainly that the control-plane and local harness
  work does not by itself authorize transport-security claims
- AG.10 must carry concrete type/boundary contracts for certificate storage,
  trust decisions, secure peer handshake, doctor projection, and explicit
  insecure-mode visibility
