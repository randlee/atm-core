---
title: AI.8 cross-host control plane
status: complete
branch: feature/pAI-s8-crosshost-control-plane
worktree: ../atm-core-worktrees/feature/pAI-s8-crosshost-control-plane
target: integrate/phase-AI
---

# AI.8 — cross-host control plane

## Deliverables

1. Add storage-trait-backed SQLite records for enabled HTTPS interfaces, local
   certificate identity, and exact trusted peers (host identity + pinned
   fingerprint).
2. Add CLI lifecycle commands to list/manage interfaces, initialize/show the
   local certificate, and explicitly add/replace/revoke trusted peers.
3. Surface safe configured/bound/trust state in `atm doctor`.
4. Forbid environment-controlled peer address, bind address, or trust state.
5. Validate the complete enabled listener set, certificate reference, and
   exact trust records before startup. Any invalid enabled HTTPS configuration
   or bind preflight failure fails daemon startup without publishing a partial
   listener set; doctor reports the typed configuration/bind failure.

## Contract

```rust
pub struct HttpsInterface {
    pub bind_addr: SocketAddr,
    pub advertise_host: HostName,
    pub enabled: bool,
}

pub struct LocalCertificate {
    pub fingerprint: CertificateFingerprint,
    pub private_key_ref: PrivateKeyRef,
}

pub struct TrustedPeer {
    pub host: HostName,
    pub fingerprint: CertificateFingerprint,
    pub enabled: bool,
}

pub trait PeerConfigStore: Send + Sync {
    fn enabled_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError>;
    fn local_certificate(&self) -> Result<LocalCertificate, AtmError>;
    fn trusted_peer(&self, host: &HostName) -> Result<TrustedPeer, AtmError>;
}
```

This trait is configuration-only. It contains no delivery status, retry, or
mailbox state and is implemented by the selected storage backend.

## Acceptance criteria

- No enabled interface means no HTTPS listener.
- A peer record cannot be added or fingerprint replaced without explicit
  confirmation.
- Configuration is behind the storage trait; HTTP/HTTPS adapters do not use
  rusqlite types.
- Doctor never exposes private key material.
- Invalid enabled listener configuration fails before partial HTTPS service is
  published; a disabled interface never binds.

## Non-closure

AI.8 closes durable configuration and its operator surface only. HTTPS bind,
accept, and outbound delivery are AI.9 work and must not be implemented here.

## Required validation

Storage migration/trait tests; CLI integration tests; doctor redaction tests;
`just lint`; `just test`; configuration-boundary gate.
