---
id: AG.10
title: Secured Cross-Host Transport Implementation
status: planned
branch: feature/pAG-s10-secured-transport
worktree: ../atm-core-worktrees/feature/pAG-s10-secured-transport
target: develop
---

# Sprint AG.10 — Secured Cross-Host Transport Implementation

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.10
worktree: ../atm-core-worktrees/feature/pAG-s10-secured-transport
branch: feature/pAG-s10-secured-transport
status: planned
estimated_scope: medium
```

## Goal

Implement the actual secured daemon-to-daemon transport after AG.8 finishes the
security design/reconciliation work.

## Deliverables

- secured daemon-to-daemon transport implementation
- peer-auth / credential handling implementation
- secure loopback validation support
- secure LAN and routed/VPN validation support

## Boundary And Type Contract

Illustrative implementation signatures:

```rust
pub struct PeerCredential {
    pub host_name: String,
    pub fingerprint_sha256: String,
    pub cert_not_after: Option<DateTime<Utc>>,
}

pub trait SecureTransportNegotiator {
    fn connect_secure(&self, endpoint: SocketAddr, credential: &PeerCredential) -> Result<SecurePeerChannel, AtmError>;
    fn accept_secure(&self, socket: AcceptedSocket) -> Result<AuthenticatedPeer, AtmError>;
}

pub trait PeerCredentialStore {
    fn local_identity(&self) -> Result<LocalPeerIdentity, AtmError>;
    fn trusted_peer(&self, host: &Hostname) -> Result<Option<PeerCredential>, AtmError>;
}
```

These names are illustrative, but the sprint requires equivalent explicit type
ownership for secure negotiation and credential lookup.

## Required Validation

- secure transport implementation follows AG.8's approved security direction
- no insecure fallback is taken silently
- secure loopback row `AG-VAL-012` executes on the implemented surface
- secure LAN host-pair rerun row `AG-VAL-013` executes on the implemented
  surface
- unauthorized/unauthenticated peer rejection row `AG-VAL-014` executes on the
  implemented surface
- secure routed/VPN host-pair rerun row `AG-VAL-015` executes on the
  implemented surface

## Unit-Test Plan

- peer-auth handshake rejection on bad or missing credentials
- encryption negotiation failure does not fall back silently to insecure mode
- loopback/local diagnostic surfaces remain explicitly scoped and do not
  accidentally widen remote trust

## Integration-Test Plan

- secure daemon-to-daemon handshake succeeds for authorized peers
  (`AG-VAL-013`, `AG-VAL-015`)
- unauthorized or unauthenticated peer is rejected before mailbox mutation
  (`AG-VAL-014`)
- existing AG.7 functional rows still pass under the secured transport
  (`AG-VAL-013`, `AG-VAL-015`)

## Smoke-Test Plan

- secure loopback smoke (`AG-VAL-012`)
- secure LAN host-pair rerun (`AG-VAL-013`)
- secure routed/VPN host-pair rerun (`AG-VAL-015`)

## Entry Gate

- AG.8 planning/reconciliation work is complete

## Acceptance Criteria

- the sprint owns the actual secured-transport implementation rather than only
  planning prose
- a Boundary And Type Contract exists for the secure transport surface
- secure loopback, integration, and smoke coverage are all specified and
  required
