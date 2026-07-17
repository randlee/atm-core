use std::collections::BTreeSet;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use atm_core::boundary;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendRequestEnvelope};
use atm_core::send::{RemoteTargetHost, SendRequest};
use atm_storage::PeerInterfaceConfigStore;

use super::PeerTransportRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteDeliveryDecision {
    HealthyImmediateWait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteFailureKind {
    TerminalProtocolRejected,
    TerminalMalformedTarget,
}

#[derive(Debug)]
pub(crate) enum SendOutcome {
    Delivered(Box<ResponseEnvelope>),
    Deferred,
    RejectedTerminal(RemoteFailureKind),
    OutcomeUnknown,
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
    ) -> Result<SendOutcome, CrossHostDeliveryInfraError>;
}

#[derive(Clone)]
pub(crate) struct DaemonCrossHostDelivery {
    peer_interface_config_store: Arc<dyn PeerInterfaceConfigStore + Send + Sync>,
    peer_transport_runtime: PeerTransportRuntime,
}

impl std::fmt::Debug for DaemonCrossHostDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonCrossHostDelivery")
            .field(
                "peer_interface_config_store",
                &"dyn PeerInterfaceConfigStore",
            )
            .field("peer_transport_runtime", &self.peer_transport_runtime)
            .finish()
    }
}

impl DaemonCrossHostDelivery {
    pub(crate) fn new(
        peer_interface_config_store: Arc<dyn PeerInterfaceConfigStore + Send + Sync>,
        peer_transport_runtime: PeerTransportRuntime,
    ) -> Self {
        Self {
            peer_interface_config_store,
            peer_transport_runtime,
        }
    }

    fn decide_remote_delivery(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<(RemoteDeliveryDecision, SocketAddr), CrossHostDeliveryInfraError> {
        let endpoint = self.resolve_remote_endpoint(remote_host)?;
        Ok((RemoteDeliveryDecision::HealthyImmediateWait, endpoint))
    }

    fn resolve_remote_endpoint(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<SocketAddr, CrossHostDeliveryInfraError> {
        let port = self.resolve_remote_port()?;
        (remote_host.as_str(), port)
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
            .next()
            .ok_or_else(|| {
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

    fn resolve_remote_port(&self) -> Result<u16, CrossHostDeliveryInfraError> {
        let enabled_ports = self
            .peer_interface_config_store
            .list_interfaces()
            .map_err(CrossHostDeliveryInfraError::StorageUnavailable)?
            .into_iter()
            .filter(|row| row.enabled)
            .map(|row| row.port)
            .collect::<BTreeSet<_>>();
        match enabled_ports.len() {
            1 => Ok(*enabled_ports.iter().next().expect("one enabled port")),
            0 => self
                .peer_transport_runtime
                .bound_addr()
                .map_err(CrossHostDeliveryInfraError::RuntimeUnavailable)?
                .map(|addr| addr.port())
                .ok_or_else(|| {
                    CrossHostDeliveryInfraError::RuntimeUnavailable(
                        AtmError::daemon_unavailable(
                            "remote delivery is unavailable because no enabled cross-host interface port is configured",
                        )
                        .with_recovery(
                            "Add and enable one daemon interface row with `atm daemon interfaces add ...`, restart atm-daemon if required, and retry the remote send.",
                        ),
                    )
                }),
            _ => Err(CrossHostDeliveryInfraError::InternalInvariantViolation(
                AtmError::validation(
                    "remote delivery is ambiguous because multiple enabled cross-host ports are configured".to_string(),
                )
                .with_recovery(
                    "Reduce the enabled daemon interface set to one shared port before retrying remote sends that specify only `<host>`.",
                ),
            )),
        }
    }

    fn classify_error(error: &AtmError) -> SendOutcome {
        if error.code == AtmErrorCode::RemoteDeliveryOutcomeUnknown {
            return SendOutcome::OutcomeUnknown;
        }
        if error.code == AtmErrorCode::AddressParseFailed {
            return SendOutcome::RejectedTerminal(RemoteFailureKind::TerminalMalformedTarget);
        }
        if error.is_validation() {
            return SendOutcome::RejectedTerminal(RemoteFailureKind::TerminalProtocolRejected);
        }
        if error.code == AtmErrorCode::DaemonUnavailable || error.is_timeout() {
            return SendOutcome::Deferred;
        }
        SendOutcome::Deferred
    }
}

impl boundary::sealed::Sealed for DaemonCrossHostDelivery {}

impl CrossHostDelivery for DaemonCrossHostDelivery {
    fn deliver_remote(
        &self,
        mut request: SendRequest,
        remote_host: RemoteTargetHost,
    ) -> Result<SendOutcome, CrossHostDeliveryInfraError> {
        let (decision, endpoint) = self.decide_remote_delivery(&remote_host)?;
        request.remote_host = None;
        match decision {
            RemoteDeliveryDecision::HealthyImmediateWait => self
                .peer_transport_runtime
                .send_to_endpoint(
                    endpoint,
                    RequestEnvelope::Send(SendRequestEnvelope::Compose(request)),
                )
                .map(Box::new)
                .map(SendOutcome::Delivered)
                .or_else(|error| Ok(Self::classify_error(&error))),
        }
    }
}
