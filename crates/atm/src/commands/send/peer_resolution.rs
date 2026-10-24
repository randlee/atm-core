//! Trusted-peer host resolution for the `atm send` command.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

use atm_core::types::HostName;
use atm_storage::{AtmError, TrustedPeer};

use super::peer_resolution_error;

pub(super) fn resolve_trusted_ipv4(
    target_ip: Ipv4Addr,
    peers: &[TrustedPeer],
) -> std::result::Result<HostName, AtmError> {
    resolve_trusted_ipv4_with_lookup(target_ip, peers, |peer| {
        let authority = (peer.host.as_str(), peer.https_port.get());
        authority
            .to_socket_addrs()
            .map(|addresses| addresses.map(|address| address.ip()).collect())
            .map_err(|error| {
                peer_resolution_error(
                    format!("trusted peer '{}' could not be resolved: {error}", peer.host),
                    "Verify the registered DNS/mDNS authority is resolvable, then retry the send with its hostname.",
                )
            })
    })
}

pub(super) fn resolve_trusted_ipv4_with_lookup(
    target_ip: Ipv4Addr,
    peers: &[TrustedPeer],
    lookup: impl Fn(&TrustedPeer) -> std::result::Result<Vec<IpAddr>, AtmError>,
) -> std::result::Result<HostName, AtmError> {
    const MAX_ENABLED_PEER_LOOKUPS: usize = 32;

    let enabled_peers: Vec<&TrustedPeer> = peers.iter().filter(|peer| peer.enabled).collect();
    if enabled_peers.len() > MAX_ENABLED_PEER_LOOKUPS {
        return Err(peer_resolution_error(
            format!(
                "literal IP authority resolution exceeds the {MAX_ENABLED_PEER_LOOKUPS}-peer lookup bound"
            ),
            "Use the registered hostname directly, or reduce the enabled peer set before retrying the literal IP diagnostic.",
        ));
    }

    let mut matches = Vec::new();
    for peer in enabled_peers {
        if lookup(peer)?.contains(&IpAddr::V4(target_ip)) {
            matches.push(peer);
        }
    }
    match matches.as_slice() {
        [peer] => Ok(peer.host.clone()),
        [] => Err(peer_resolution_error(
            format!("no enabled trusted peer resolves to literal IP '{target_ip}'"),
            "Use a registered hostname, or add/enable the exact trusted peer before retrying.",
        )),
        _ => Err(peer_resolution_error(
            format!("literal IP '{target_ip}' resolves to multiple enabled trusted peers"),
            "Use the exact registered hostname instead of the shared IP address.",
        )),
    }
}

pub(super) fn resolve_trusted_host(
    raw_host: &str,
    peers: &[TrustedPeer],
) -> std::result::Result<Option<HostName>, AtmError> {
    let exact_or_local = |host: &HostName| {
        let canonical_lower = host.as_str().to_ascii_lowercase();
        host.as_str().eq_ignore_ascii_case(raw_host)
            || (!raw_host.to_ascii_lowercase().ends_with(".local")
                && canonical_lower
                    .strip_suffix(".local")
                    .is_some_and(|short| short.eq_ignore_ascii_case(raw_host)))
    };
    let matches: Vec<&TrustedPeer> = peers
        .iter()
        .filter(|peer| peer.enabled && exact_or_local(&peer.host))
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [peer] => Ok(Some(peer.host.clone())),
        _ => Err(peer_resolution_error(
            format!("trusted peer shorthand '{raw_host}' is ambiguous"),
            "Use the exact registered hostname from `atm peer trust list`, or remove the duplicate enabled peer record before retrying.",
        )),
    }
}
