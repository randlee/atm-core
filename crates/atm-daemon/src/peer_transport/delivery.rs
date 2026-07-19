use std::collections::BTreeSet;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::RequestEnvelope;
use atm_core::schema::AtmMessageId;
use atm_core::send::{RemoteTargetHost, SendRequest};
use atm_storage::{PeerInterfaceConfigStore, PeerInterfaceRow};

use super::PeerTransportRuntime;
use crate::outbound_delivery_policy::{
    OutboundDeliveryDisposition, TransportAttemptResult, requires_replay, shared_outcome_policy,
};

const DEFAULT_REMOTE_RETRY_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteDeliveryDecision {
    HealthyImmediateWait,
    DeferredRetry,
}

#[derive(Debug)]
pub(crate) enum CrossHostDeliveryInfraError {
    RuntimeUnavailable(AtmError),
    StorageUnavailable(AtmError),
    InternalInvariantViolation(AtmError),
}

impl CrossHostDeliveryInfraError {
    pub(crate) fn into_atm_error(self) -> AtmError {
        match self {
            Self::RuntimeUnavailable(error)
            | Self::StorageUnavailable(error)
            | Self::InternalInvariantViolation(error) => error,
        }
    }
}

pub(crate) trait CrossHostDelivery: boundary::sealed::Sealed + Send + Sync {
    fn deliver_remote(
        &self,
        request: SendRequest,
        remote_host: RemoteTargetHost,
        deferred_receipt_message_id: AtmMessageId,
    ) -> Result<OutboundDeliveryDisposition, CrossHostDeliveryInfraError>;
}

pub(crate) trait RemoteEndpointResolver: boundary::sealed::Sealed + Send + Sync {
    fn resolve_endpoint(
        &self,
        remote_host: &RemoteTargetHost,
        bound_addr_hint: Option<SocketAddr>,
    ) -> Result<SocketAddr, CrossHostDeliveryInfraError>;
}

#[derive(Clone)]
pub(crate) struct DaemonRemoteEndpointResolver {
    peer_interface_config_store: Arc<dyn PeerInterfaceConfigStore + Send + Sync>,
    bound_addr_hint: Option<SocketAddr>,
}

impl std::fmt::Debug for DaemonRemoteEndpointResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRemoteEndpointResolver")
            .field(
                "peer_interface_config_store",
                &"dyn PeerInterfaceConfigStore",
            )
            .field("bound_addr_hint", &self.bound_addr_hint)
            .finish()
    }
}

impl DaemonRemoteEndpointResolver {
    pub(crate) fn new(
        peer_interface_config_store: Arc<dyn PeerInterfaceConfigStore + Send + Sync>,
        bound_addr_hint: Option<SocketAddr>,
    ) -> Self {
        Self {
            peer_interface_config_store,
            bound_addr_hint,
        }
    }
}

impl boundary::sealed::Sealed for DaemonRemoteEndpointResolver {}

impl RemoteEndpointResolver for DaemonRemoteEndpointResolver {
    fn resolve_endpoint(
        &self,
        remote_host: &RemoteTargetHost,
        bound_addr_hint: Option<SocketAddr>,
    ) -> Result<SocketAddr, CrossHostDeliveryInfraError> {
        let (port, interface_family_preference) = resolve_remote_port_for_host(
            self.peer_interface_config_store
                .list_interfaces()
                .map_err(CrossHostDeliveryInfraError::StorageUnavailable)?,
            bound_addr_hint.or(self.bound_addr_hint),
            remote_host,
        )?;
        let preferred_family = self
            .bound_addr_hint
            .map(|addr| addr.is_ipv4())
            .or(bound_addr_hint.map(|addr| addr.is_ipv4()))
            .or(interface_family_preference);
        let resolved = (remote_host.as_str(), port)
            .to_socket_addrs()
            .map_err(|source| {
                CrossHostDeliveryInfraError::RuntimeUnavailable(
                    AtmError::address_parse(format!(
                        "failed to resolve remote host `{}` on port {}",
                        remote_host.as_str(),
                        port
                    ))
                    .with_recovery(
                        "Use a reachable literal IP address or resolvable hostname for the remote host before retrying the remote send.",
                    )
                    .with_source(source),
                )
            })?
            .collect::<Vec<_>>();
        select_resolved_remote_endpoint(&resolved, preferred_family).ok_or_else(|| {
            CrossHostDeliveryInfraError::RuntimeUnavailable(
                AtmError::address_parse(format!(
                    "remote host `{}` did not resolve to any socket addresses",
                    remote_host.as_str()
                ))
                .with_recovery(
                    "Use a reachable literal IP address or resolvable hostname for the remote host before retrying the remote send.",
                ),
            )
        })
    }
}

#[derive(Clone)]
pub(crate) struct DaemonCrossHostDelivery {
    endpoint_resolver: Arc<dyn RemoteEndpointResolver + Send + Sync>,
    peer_transport_runtime: PeerTransportRuntime,
}

impl std::fmt::Debug for DaemonCrossHostDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonCrossHostDelivery")
            .field("endpoint_resolver", &"dyn RemoteEndpointResolver")
            .field("peer_transport_runtime", &self.peer_transport_runtime)
            .finish()
    }
}

impl DaemonCrossHostDelivery {
    pub(crate) fn new(
        endpoint_resolver: Arc<dyn RemoteEndpointResolver + Send + Sync>,
        peer_transport_runtime: PeerTransportRuntime,
    ) -> Self {
        Self {
            endpoint_resolver,
            peer_transport_runtime,
        }
    }

    fn decide_remote_delivery(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<(RemoteDeliveryDecision, SocketAddr), CrossHostDeliveryInfraError> {
        let bound_addr = self
            .peer_transport_runtime
            .bound_addr()
            .map_err(CrossHostDeliveryInfraError::RuntimeUnavailable)?;
        let endpoint = self
            .endpoint_resolver
            .resolve_endpoint(remote_host, bound_addr)?;
        let decision = if bound_addr.is_some() {
            RemoteDeliveryDecision::HealthyImmediateWait
        } else {
            RemoteDeliveryDecision::DeferredRetry
        };
        Ok((decision, endpoint))
    }

    fn persist_remote_retry(
        &self,
        replay_request: &SendRequest,
        deferred_receipt_message_id: AtmMessageId,
        remote_host: &RemoteTargetHost,
    ) -> Result<(), CrossHostDeliveryInfraError> {
        self.peer_transport_runtime
            .persist_remote_request_for_retry(
                DEFAULT_REMOTE_RETRY_BUDGET,
                remote_host.clone(),
                replay_request.caller_team.clone(),
                replay_request.caller_identity.clone(),
                boundary::MessageKey::from(deferred_receipt_message_id),
                RequestEnvelope::Send(Box::new(replay_request.clone())),
                Some(replay_request.caller_team.clone()),
                Some(replay_request.caller_identity.clone()),
                Some(deferred_receipt_message_id),
                Some(replay_request.to.to_string()),
                Some(remote_host.as_str().to_string()),
            )
            .map_err(CrossHostDeliveryInfraError::RuntimeUnavailable)
    }

    fn deliver_immediate_wait(
        &self,
        endpoint: SocketAddr,
        request: SendRequest,
        replay_request: SendRequest,
        deferred_receipt_message_id: AtmMessageId,
        remote_host: &RemoteTargetHost,
    ) -> Result<OutboundDeliveryDisposition, CrossHostDeliveryInfraError> {
        let disposition = match self
            .peer_transport_runtime
            .send_to_endpoint_immediate_wait(endpoint, RequestEnvelope::Send(Box::new(request)))
        {
            Ok(response) => shared_outcome_policy(
                TransportAttemptResult::ImmediateResponse(Box::new(response)),
                deferred_receipt_message_id,
            ),
            Err(error) => shared_outcome_policy(
                TransportAttemptResult::ImmediateError(error),
                deferred_receipt_message_id,
            ),
        };
        if requires_replay(&disposition) {
            self.persist_remote_retry(&replay_request, deferred_receipt_message_id, remote_host)?;
        }
        Ok(disposition)
    }
}

fn select_resolved_remote_endpoint(
    resolved: &[SocketAddr],
    preferred_ipv4: Option<bool>,
) -> Option<SocketAddr> {
    preferred_ipv4
        .and_then(|is_ipv4| {
            resolved
                .iter()
                .copied()
                .find(|addr| addr.is_ipv4() == is_ipv4)
        })
        .or_else(|| resolved.first().copied())
}

impl boundary::sealed::Sealed for DaemonCrossHostDelivery {}

impl CrossHostDelivery for DaemonCrossHostDelivery {
    fn deliver_remote(
        &self,
        mut request: SendRequest,
        remote_host: RemoteTargetHost,
        deferred_receipt_message_id: AtmMessageId,
    ) -> Result<OutboundDeliveryDisposition, CrossHostDeliveryInfraError> {
        let (decision, endpoint) = self.decide_remote_delivery(&remote_host)?;
        request.remote_host = None;
        let replay_request = request.clone();
        match decision {
            RemoteDeliveryDecision::DeferredRetry => {
                self.persist_remote_retry(
                    &replay_request,
                    deferred_receipt_message_id,
                    &remote_host,
                )?;
                Ok(shared_outcome_policy(
                    TransportAttemptResult::ListenerUnavailable(
                        AtmError::daemon_unavailable(
                            "remote delivery is deferred because no healthy peer listener is currently bound",
                        )
                        .with_recovery(
                            "Restore the cross-host listener health and retry; the daemon queued the send for bounded background replay.",
                        ),
                    ),
                    deferred_receipt_message_id,
                ))
            }
            RemoteDeliveryDecision::HealthyImmediateWait => self.deliver_immediate_wait(
                endpoint,
                request,
                replay_request,
                deferred_receipt_message_id,
                &remote_host,
            ),
        }
    }
}

fn resolve_remote_port_for_host(
    interface_rows: Vec<PeerInterfaceRow>,
    bound_addr: Option<SocketAddr>,
    remote_host: &RemoteTargetHost,
) -> Result<(u16, Option<bool>), CrossHostDeliveryInfraError> {
    let enabled_rows = interface_rows
        .into_iter()
        .filter(|row| row.enabled)
        .collect::<Vec<_>>();
    let target_is_loopback = remote_host.targets_loopback();
    let scoped_rows = enabled_rows
        .iter()
        .filter(|row| row.bind_addr.is_loopback() == target_is_loopback)
        .collect::<Vec<_>>();
    let candidate_rows = if scoped_rows.is_empty() {
        enabled_rows.iter().collect::<Vec<_>>()
    } else {
        scoped_rows
    };
    let enabled_ports = candidate_rows
        .iter()
        .map(|row| row.port)
        .collect::<BTreeSet<_>>();
    match enabled_ports.len() {
        1 => Ok((
            *enabled_ports.iter().next().expect("one enabled port"),
            interface_family_preference_refs(&candidate_rows),
        )),
        0 => bound_addr.map(|addr| (addr.port(), Some(addr.is_ipv4()))).ok_or_else(|| {
            CrossHostDeliveryInfraError::RuntimeUnavailable(
                AtmError::daemon_unavailable(
                    "remote delivery is unavailable because no enabled cross-host interface port is configured",
                )
                .with_recovery(
                    "Add and enable one daemon interface row with `atm daemon interfaces add ...`, restart atm-daemon if required, and retry the remote send.",
                ),
            )
        }),
        _ => {
            if let Some(ip) = remote_host.literal_ip() {
                let matching_ports = candidate_rows
                    .iter()
                    .filter(|row| row.bind_addr == ip || row.advertise_addr == ip)
                    .map(|row| row.port)
                    .collect::<BTreeSet<_>>();
                if matching_ports.len() == 1 {
                    let matching_rows = candidate_rows
                        .into_iter()
                        .filter(|row| row.bind_addr == ip || row.advertise_addr == ip)
                        .collect::<Vec<_>>();
                    return Ok((
                        *matching_ports.iter().next().expect("one matching port"),
                        interface_family_preference_refs(&matching_rows),
                    ));
                }
            }
            Err(CrossHostDeliveryInfraError::InternalInvariantViolation(
                AtmError::validation(
                    "remote delivery is ambiguous because multiple enabled cross-host ports are configured".to_string(),
                )
                .with_recovery(
                    "Reduce the enabled daemon interface set to one shared port before retrying remote sends that specify only `<host>`, or target a host whose literal IP matches exactly one enabled interface row.",
                ),
            ))
        }
    }
}

fn interface_family_preference_refs(rows: &[&PeerInterfaceRow]) -> Option<bool> {
    let mut families = rows
        .iter()
        .map(|row| row.bind_addr.is_ipv4())
        .collect::<BTreeSet<_>>();
    if families.len() == 1 {
        families.pop_first()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_remote_port_for_host, select_resolved_remote_endpoint};
    use atm_core::send::parse_send_target;
    use atm_core::types::IsoTimestamp;
    use atm_storage::PeerInterfaceKind;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    fn interface_row(
        bind: Ipv4Addr,
        advertise: Ipv4Addr,
        port: u16,
        enabled: bool,
    ) -> atm_storage::PeerInterfaceRow {
        atm_storage::PeerInterfaceRow {
            interface_id: i64::from(port),
            interface_name: format!("if-{port}"),
            bind_addr: IpAddr::V4(bind),
            advertise_addr: IpAddr::V4(advertise),
            port,
            interface_kind: PeerInterfaceKind::Lan,
            enabled,
            configured_by: "test@test-team".to_string(),
            configured_at: IsoTimestamp::now(),
            updated_at: IsoTimestamp::now(),
            last_observed_at: None,
            refresh_deadline_at: None,
            stale_at: None,
            last_bound_at: None,
            last_bind_error: None,
        }
    }

    #[test]
    fn select_resolved_remote_endpoint_prefers_matching_listener_family() {
        let resolved = vec![
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 43101),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 43101),
        ];
        assert_eq!(
            select_resolved_remote_endpoint(&resolved, Some(true)),
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 43101))
        );
        assert_eq!(
            select_resolved_remote_endpoint(&resolved, Some(false)),
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 43101))
        );
    }

    #[test]
    fn select_resolved_remote_endpoint_falls_back_to_first_when_no_preference_matches() {
        let resolved = vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 43101)];
        assert_eq!(
            select_resolved_remote_endpoint(&resolved, Some(true)),
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 43101))
        );
        assert_eq!(
            select_resolved_remote_endpoint(&resolved, None),
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 43101))
        );
    }

    #[test]
    fn resolve_remote_port_prefers_matching_interface_row_for_literal_self_ip() {
        let rows = vec![
            interface_row(
                Ipv4Addr::new(127, 0, 0, 1),
                Ipv4Addr::new(127, 0, 0, 1),
                43145,
                true,
            ),
            interface_row(
                Ipv4Addr::new(192, 0, 0, 2),
                Ipv4Addr::new(192, 0, 0, 2),
                43101,
                true,
            ),
        ];
        let host = parse_send_target("qa-a@test-team.192.0.0.2", None)
            .expect("parse target")
            .remote_host
            .expect("host");
        let (port, family) =
            resolve_remote_port_for_host(rows, None, &host).expect("matching self-ip port");
        assert_eq!(port, 43101);
        assert_eq!(family, Some(true));
    }

    #[test]
    fn resolve_remote_port_carries_interface_family_for_unbound_localhost_resolution() {
        let rows = vec![interface_row(
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(127, 0, 0, 1),
            43101,
            true,
        )];
        let host = parse_send_target("qa-a@test-team.localhost", None)
            .expect("parse target")
            .remote_host
            .expect("host");
        let (port, family) =
            resolve_remote_port_for_host(rows, None, &host).expect("localhost port");
        assert_eq!(port, 43101);
        assert_eq!(family, Some(true));
    }

    #[test]
    fn resolve_remote_port_ignores_loopback_port_for_non_loopback_target() {
        let rows = vec![
            interface_row(
                Ipv4Addr::new(127, 0, 0, 1),
                Ipv4Addr::new(127, 0, 0, 1),
                43145,
                true,
            ),
            interface_row(
                Ipv4Addr::new(192, 168, 128, 82),
                Ipv4Addr::new(192, 168, 128, 82),
                43101,
                true,
            ),
        ];
        let host = parse_send_target("cm5@atm-m5.192.168.128.29", None)
            .expect("parse target")
            .remote_host
            .expect("host");
        let (port, family) =
            resolve_remote_port_for_host(rows, None, &host).expect("lan host port");
        assert_eq!(port, 43101);
        assert_eq!(family, Some(true));
    }

    #[test]
    fn remote_host_loopback_detection_matches_localhost_and_loopback_ip() {
        let localhost = parse_send_target(
            &format!(
                "{}@{}.localhost",
                atm_core::test_support::TEST_SENDER,
                atm_core::test_support::TEST_TEAM
            ),
            None,
        )
        .expect("localhost target")
        .remote_host
        .expect("localhost host");
        let loopback_ip = parse_send_target(
            &format!(
                "{}@{}.127.0.0.1",
                atm_core::test_support::TEST_SENDER,
                atm_core::test_support::TEST_TEAM
            ),
            None,
        )
        .expect("loopback target")
        .remote_host
        .expect("loopback host");
        let lan_ip = parse_send_target(
            &format!(
                "{}@{}.192.168.128.82",
                atm_core::test_support::TEST_SENDER,
                atm_core::test_support::TEST_TEAM
            ),
            None,
        )
        .expect("lan target")
        .remote_host
        .expect("lan host");

        assert!(localhost.targets_loopback());
        assert!(loopback_ip.targets_loopback());
        assert!(!lan_ip.targets_loopback());
    }
}
