//! Opaque authenticated peer-stream seam.
//!
//! The HTTP runtime deliberately owns only the established byte stream.  The
//! daemon bootstrap composes the concrete security adapter; this module never
//! imports TLS, certificates, or peer configuration.

use std::future::Future;
use std::pin::Pin;

use atm_core::error::AtmError;
use atm_core::types::HostName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

/// Opaque bidirectional byte stream admitted by the selected peer-wire
/// adapter.  It intentionally exposes no security implementation detail to
/// the HTTP route or application pipeline.
pub trait AuthenticatedPeerStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<Stream> AuthenticatedPeerStream for Stream where Stream: AsyncRead + AsyncWrite + Send + Unpin {}

/// Result stream returned after outbound peer authentication completes.
pub type EstablishedPeerStream = Box<dyn AuthenticatedPeerStream>;

/// Boxed asynchronous peer-stream establishment operation.
pub type PeerStreamFuture<'a, Output> =
    Pin<Box<dyn Future<Output = Result<Output, AtmError>> + Send + 'a>>;

/// One established authenticated inbound connection.
pub struct AcceptedPeerStream {
    /// Adapter-authenticated source identity, never derived from JSON or IP.
    pub source_host: HostName,
    /// The opaque stream that carries the ordinary canonical HTTP protocol.
    pub stream: EstablishedPeerStream,
}

/// Bootstrap-composed peer-stream establishment.
///
/// This narrow seam carries only TCP streams and the already-authenticated
/// inbound identity.  It cannot select a mode, inspect an HTTP DTO, or
/// introduce a second application route.
pub trait PeerStreamAdapter: Send + Sync {
    /// Authenticate an outbound TCP stream for the exact configured peer.
    fn connect<'a>(
        &'a self,
        stream: TcpStream,
        peer: &'a HostName,
    ) -> PeerStreamFuture<'a, EstablishedPeerStream>;

    /// Authenticate an inbound TCP stream before HTTP decoding begins.
    fn accept<'a>(&'a self, stream: TcpStream) -> PeerStreamFuture<'a, AcceptedPeerStream>;
}
