//! Opaque stream-wrapping port for the canonical Tokio peer HTTP path.
//!
//! The port owns no TLS policy, peer configuration, routing, or HTTP protocol.
//! Its sole production implementation is `peer-tls`; the Tokio/Axum runtime
//! consumes this opaque stream after AO.2.

use std::future::Future;
use std::pin::Pin;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::sealed;
use crate::api::RequestDeadline;
use crate::error::AtmError;
use crate::types::HostName;

/// The opaque bidirectional byte stream returned by a peer transport adapter.
///
/// HTTP remains above this boundary. Implementations may wrap the supplied
/// TCP stream, but they cannot add an ATM message or routing surface here.
pub trait PeerIo: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T> PeerIo for T where T: AsyncRead + AsyncWrite + Send + Unpin {}

/// Object-safe opaque peer stream used by the runtime's one HTTP path.
pub type BoxedPeerIo = Box<dyn PeerIo>;

/// An accepted peer stream together with the identity authenticated by its
/// transport adapter.
///
/// The runtime deliberately receives neither certificates nor TLS session
/// types. It needs only this already-authenticated hostname to retain the
/// established peer provenance on the canonical HTTP request.
pub struct AcceptedPeerIo {
    io: BoxedPeerIo,
    source_host: HostName,
}

impl AcceptedPeerIo {
    #[must_use]
    pub const fn new(io: BoxedPeerIo, source_host: HostName) -> Self {
        Self { io, source_host }
    }

    #[must_use]
    pub fn into_parts(self) -> (BoxedPeerIo, HostName) {
        (self.io, self.source_host)
    }
}

/// BOUNDARY-PeerIoAdapter — see `boundaries/atm-core/peer-io-adapter.toml`.
///
/// This is an intentionally sealed, object-safe port. `peer-tls` is its only
/// authorized production implementation under ADR-001. The trait accepts and
/// produces transport bytes only; certificate policy stays in `atm_storage`.
pub trait PeerIoAdapter: sealed::Sealed + Send + Sync {
    /// Wrap an accepted TCP stream before application bytes are read and
    /// return the adapter-authenticated source hostname.
    fn accept<'adapter>(
        &'adapter self,
        stream: TcpStream,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<AcceptedPeerIo, AtmError>> + Send + 'adapter>>;

    /// Connect to and wrap one configured peer before application bytes are written.
    fn connect<'adapter>(
        &'adapter self,
        peer: HostName,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<BoxedPeerIo, AtmError>> + Send + 'adapter>>;
}

#[cfg(test)]
mod tests {
    use super::PeerIoAdapter;

    #[test]
    fn adapter_contract_is_object_safe() {
        fn requires_dyn_adapter(_: &dyn PeerIoAdapter) {}
        let _ = requires_dyn_adapter;
    }
}
