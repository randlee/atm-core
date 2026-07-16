---
id: AG.10
title: Secured Cross-Host Transport Implementation
status: planned
branch: feature/pAG-s10-secured-cross-host-transport
worktree: ../atm-core-worktrees/feature/pAG-s10-secured-cross-host-transport
target: integrate/phase-AG
---

# Sprint AG.10 — Secured Cross-Host Transport Implementation

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.10
worktree: ../atm-core-worktrees/feature/pAG-s10-secured-cross-host-transport
branch: feature/pAG-s10-secured-cross-host-transport
status: planned
estimated_scope: large
```

## Goal

Implement the secured daemon-to-daemon transport defined by AG.8 without
regressing the already-proved functional cross-host path.

## Deliverables

- TLS-backed daemon-to-daemon transport on the existing peer-listener/send path
- on-demand generation of a local self-signed daemon identity certificate when
  no local certificate exists yet
- durable storage for the local daemon certificate/private key and approved
  peer trust material
- explicit operator approval flow for trusting a remote peer certificate
- explicit insecure-mode support for controlled development/operator fallback
- doctor/runtime projection showing whether the daemon is running in secure or
  insecure mode and whether peer trust material is configured
- secure loopback, secure LAN, and secure routed/VPN validation coverage

## Boundary And Type Contract

The sprint must land explicit ownership for secure peer state rather than
burying TLS decisions inside socket code.

Illustrative implementation signatures:

```rust
pub enum PeerSecurityMode {
    SecureRequired,
    InsecureAllowed,
}

pub struct LocalPeerIdentity {
    pub certificate_der: Vec<u8>,
    pub private_key_handle: PrivateKeyHandle,
    pub fingerprint_sha256: String,
}

pub struct TrustedPeerRecord {
    pub host: String,
    pub fingerprint_sha256: String,
    pub display_name: Option<String>,
    pub approved_at: DateTime<Utc>,
}

pub trait PeerIdentityStore {
    fn load_or_create_local_identity(&self) -> Result<LocalPeerIdentity, AtmError>;
    fn load_trusted_peer(&self, host: &str) -> Result<Option<TrustedPeerRecord>, AtmError>;
    fn upsert_trusted_peer(&self, record: TrustedPeerRecord) -> Result<(), AtmError>;
}

pub trait SecurePeerTransport {
    fn connect_secure(
        &self,
        endpoint: SocketAddr,
        identity: &LocalPeerIdentity,
        trusted_peer: Option<&TrustedPeerRecord>,
        mode: PeerSecurityMode,
    ) -> Result<SecurePeerChannel, AtmError>;

    fn accept_secure(
        &self,
        accepted: AcceptedSocket,
        identity: &LocalPeerIdentity,
        allowlist_host: &str,
        mode: PeerSecurityMode,
    ) -> Result<AuthenticatedPeer, AtmError>;
}
```

These names are illustrative, but equivalent explicit type/boundary ownership is
required.

## Required Validation

- secure transport implementation follows ADR-030 and does not silently widen
  or narrow the trust model
- secure handshake failure never falls back silently to insecure mode
- doctor/runtime output reports secure vs insecure mode explicitly
- secure loopback row `AG-VAL-012` executes on the implemented surface
- secure LAN host-pair rerun row `AG-VAL-013` executes on the implemented
  surface
- unauthorized or untrusted peer rejection row `AG-VAL-014` executes on the
  implemented surface
- secure routed/VPN host-pair rerun row `AG-VAL-015` executes on the
  implemented surface
- existing AG.7 functional rows still pass under the secure transport path

## Unit-Test Plan

- local identity is generated exactly once when absent and reused when present
- trusted peer approval persists and is consulted during handshake
- wrong certificate fingerprint is rejected deterministically
- secure handshake failure does not silently downgrade to insecure mode
- insecure mode requires an explicit configuration choice
- loopback/local diagnostic surfaces remain explicitly scoped and do not widen
  remote trust

## Integration-Test Plan

- secure loopback send/read/ack succeeds through the real peer-listener request
  path (`AG-VAL-012`)
- secure daemon-to-daemon handshake succeeds for approved peers
  (`AG-VAL-013`, `AG-VAL-015`)
- unauthorized or untrusted peer is rejected before mailbox mutation
  (`AG-VAL-014`)
- the existing AG.7 functional matrix still passes when secure mode is active

## Smoke-Test Plan

- secure loopback smoke (`AG-VAL-012`)
- secure LAN host-pair rerun (`AG-VAL-013`)
- secure rejection proof (`AG-VAL-014`)
- secure routed/VPN host-pair rerun (`AG-VAL-015`)

## Non-Closure / Out Of Scope

- no automatic trust of unknown peers without approval
- no silent downgrade from secure to insecure transport
- no claim that certificate trust replaces the durable host allowlist

## Entry Gate

- AG.8 planning/reconciliation work is complete
- AG.7 live host-pair environment is ready enough to rerun once the secure path
  lands

## Acceptance Criteria

- the sprint owns the actual secured-transport implementation rather than only
  planning prose
- secure and insecure modes are explicit and observable
- certificate/trust ownership is explicit in types and storage boundaries
- secure loopback, integration, and smoke coverage are all specified and
  required
