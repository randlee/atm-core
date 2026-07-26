//! Trusted-peer authority selection for post-write recipient routing.
//!
//! This is deliberately outside the HTTPS adapter: choosing which registered
//! peer receives a host-qualified message is routing policy, while the adapter
//! only connects to the already-selected authority.

use std::net::{IpAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::types::HostName;
use atm_storage::TrustedPeer;

const DNS_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(5);
/// Literal-IP authority resolution is deliberately bounded. Operators should
/// address a registered hostname directly when their authority set exceeds
/// this control-plane safety cap.
const MAX_LITERAL_IP_AUTHORITY_CANDIDATES: usize = 32;

/// Resolves a delivery target to one configured hostname authority. Literal
/// addresses are only aliases of exactly one fresh forward-DNS result; they
/// never become durable peer records and reverse DNS is deliberately absent.
pub(crate) fn resolve_peer_authority(
    target: &HostName,
    peers: &[TrustedPeer],
) -> Result<TrustedPeer, AtmError> {
    if let Some(peer) = peers
        .iter()
        .find(|peer| peer.enabled && peer.host == *target)
    {
        return Ok(peer.clone());
    }
    let ip: IpAddr = target.as_str().parse().map_err(|_| {
        AtmError::validation_with_recovery(
            format!("no trusted HTTPS peer is configured for {target}"),
            "register the peer hostname first, then send to that hostname or one of its current forward-DNS addresses",
        )
    })?;
    let enabled_peers = peers.iter().filter(|peer| peer.enabled).collect::<Vec<_>>();
    if enabled_peers.len() > MAX_LITERAL_IP_AUTHORITY_CANDIDATES {
        return Err(AtmError::validation_with_recovery(
            format!(
                "literal peer IP {target} cannot be resolved across more than {MAX_LITERAL_IP_AUTHORITY_CANDIDATES} enabled trusted peers"
            ),
            "send to the registered hostname directly or reduce the enabled trusted-peer set",
        ));
    }
    let matches = enabled_peers
        .into_iter()
        .filter(|peer| {
            resolve_peer_addresses(peer, DNS_RESOLUTION_TIMEOUT)
                .is_ok_and(|addresses| addresses.contains(&ip))
        })
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [peer] => Ok(peer.clone()),
        [] => Err(AtmError::validation_with_recovery(
            format!("literal peer IP {target} matches no trusted hostname"),
            "register the peer hostname, verify its forward DNS includes this IP, or send to the registered hostname",
        )),
        _ => Err(AtmError::validation_with_recovery(
            format!("literal peer IP {target} matches multiple trusted hostnames"),
            "send to the intended registered hostname or correct the overlapping forward DNS records",
        )),
    }
}

fn resolve_peer_addresses(peer: &TrustedPeer, timeout: Duration) -> Result<Vec<IpAddr>, AtmError> {
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
    use super::{MAX_LITERAL_IP_AUTHORITY_CANDIDATES, resolve_peer_authority};
    use atm_storage::TrustedPeer;

    fn trusted(host: &str) -> TrustedPeer {
        TrustedPeer {
            host: host.parse().expect("host"),
            fingerprint: "00".repeat(32).parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        }
    }

    #[test]
    fn literal_ip_selects_its_single_forward_dns_authority() {
        let target = "127.0.0.1".parse().expect("target");
        assert_eq!(
            resolve_peer_authority(&target, &[trusted("localhost")])
                .expect("authority")
                .host
                .as_str(),
            "localhost"
        );
    }

    #[test]
    fn literal_ip_without_authority_fails_closed() {
        let target = "192.0.2.1".parse().expect("target");
        assert!(resolve_peer_authority(&target, &[trusted("localhost")]).is_err());
    }

    #[test]
    fn literal_ip_with_ambiguous_authority_fails_closed() {
        let target = "127.0.0.1".parse().expect("target");
        assert!(
            resolve_peer_authority(&target, &[trusted("localhost"), trusted("localhost")]).is_err()
        );
    }

    #[test]
    fn literal_ip_authority_resolution_has_a_bounded_candidate_set() {
        let target = "127.0.0.1".parse().expect("target");
        let peers = (0..=MAX_LITERAL_IP_AUTHORITY_CANDIDATES)
            .map(|_| trusted("localhost"))
            .collect::<Vec<_>>();
        let error = resolve_peer_authority(&target, &peers)
            .expect_err("literal-IP authority fan-out must fail closed above the cap");
        assert!(error.message().contains("more than"));
        assert!(error.message().contains("Recovery:"));
    }
}
