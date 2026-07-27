//! Shared bounded peer-authority DNS resolution used by both daemon callers.

use std::net::{IpAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::time::Duration;

use atm_core::error::AtmError;
use atm_storage::TrustedPeer;

/// Resolve a configured peer's hostname with one bounded worker and preserve
/// the lower-level cause at every adapter boundary.
pub(crate) fn resolve_peer_socket_addresses(
    peer: &TrustedPeer,
    timeout: Duration,
) -> Result<Vec<IpAddr>, AtmError> {
    let authority = format!("{}:{}", peer.host, peer.https_port);
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("atm-peer-dns".to_string())
        .spawn(move || {
            let _ = sender.send(
                authority
                    .to_socket_addrs()
                    .map(|addresses| addresses.map(|address| address.ip()).collect::<Vec<_>>()),
            );
        })
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to start bounded HTTPS DNS resolution",
                source,
            )
        })?;
    receiver
        .recv_timeout(timeout)
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "HTTPS DNS resolution timed out; verify peer forward DNS or retry",
                source,
            )
        })?
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to resolve configured HTTPS peer; verify forward DNS",
                source,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::resolve_peer_socket_addresses;
    use atm_core::error_codes::AtmErrorCode;
    use atm_storage::TrustedPeer;
    use std::num::NonZeroU16;
    use std::time::Duration;

    #[test]
    fn failed_dns_resolution_preserves_the_underlying_cause() {
        let peer = TrustedPeer {
            host: "not-a-real-host.invalid".parse().expect("host"),
            fingerprint: "00".repeat(32).parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("port"),
        };
        let error = resolve_peer_socket_addresses(&peer, Duration::from_secs(1))
            .expect_err("reserved invalid host must fail");
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(error.cause().is_some(), "DNS source cause must be retained");
        assert!(
            error.message().contains("forward DNS"),
            "the bounded resolver reports an actionable forward-DNS recovery path: {error:?}"
        );
    }
}
