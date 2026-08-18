# peer-tls Boundary Inventory

## PeerTls

Canonical machine-readable boundary source:
- [../../boundaries/peer-tls/peer-tls.toml](../../boundaries/peer-tls/peer-tls.toml)

Purpose:
- own bounded Tokio-Rustls client/server handshakes and return the sealed core
  `BoxedPeerIo` stream

Rules:
- configuration enters only through `Arc<dyn PeerConfigStore>`; certificate
  parsing, fingerprints, provider selection, and pinning remain canonical
  `atm_storage::tls` helpers
- this crate never owns HTTP encoding, delivery, routing, SQL, daemon
  lifecycle, or the legacy daemon/fixture path
- `atm-daemon-bootstrap` is the only future composition consumer; the
  Tokio/Axum runtime receives only `PeerIoAdapter`, never Rustls types
