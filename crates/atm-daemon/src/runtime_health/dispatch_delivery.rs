use std::collections::BTreeSet;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::ack_mail_with_runtime_and_post_send_emitter,
    boundary,
    clear::clear_mail_with_runtime,
    error::AtmError,
    list::list_mail,
    protocol::{CompatibilityVerdict, ReleaseVersion, SendRequestEnvelope, SendResponseEnvelope},
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    send::{RemoteTargetHost, SendRequest, send_mail_with_runtime_and_post_send_emitter},
};

use super::{DaemonGraftPostSendPort, DaemonPostSendHookEmitter, DaemonRequestDispatcher};

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let graft_post_send_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let post_send_emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_post_send_port));
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                self.dispatch_compose_send(request, &post_send_emitter)
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail_with_runtime_and_post_send_emitter(
                        request,
                        self.observability.as_ref(),
                        &self.service_runtime,
                        &post_send_emitter,
                    )?,
                )))
            }
            RequestEnvelope::Heartbeat(request) => {
                Ok(ResponseEnvelope::Heartbeat(self.record_heartbeat(request)?))
            }
            RequestEnvelope::CompatibilityPreflight(preflight) => Ok(
                ResponseEnvelope::CompatibilityVerdict(self.compatibility_verdict(preflight)?),
            ),
            RequestEnvelope::List(query) => Ok(ResponseEnvelope::List(list_mail(
                query,
                self.observability.as_ref(),
            )?)),
            RequestEnvelope::Peek(query) => Ok(ResponseEnvelope::Peek(Box::new(
                peek_mail_with_runtime(query, self.observability.as_ref(), &self.service_runtime)?,
            ))),
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(Box::new(
                read_mail_with_runtime(query, self.observability.as_ref(), &self.service_runtime)?,
            ))),
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(clear_mail_with_runtime(
                query,
                self.observability.as_ref(),
                &self.service_runtime,
            )?)),
            RequestEnvelope::Doctor(query) => Ok(ResponseEnvelope::Doctor(Box::new(
                self.project_doctor_report(query)?,
            ))),
        }
    }
}

impl DaemonRequestDispatcher {
    fn dispatch_compose_send(
        &self,
        request: SendRequest,
        post_send_emitter: &DaemonPostSendHookEmitter,
    ) -> Result<ResponseEnvelope, AtmError> {
        if request.remote_host.is_some() {
            return self.dispatch_remote_send(request);
        }
        let outcome = send_mail_with_runtime_and_post_send_emitter(
            request,
            self.observability.as_ref(),
            &self.service_runtime,
            post_send_emitter,
        )?;
        Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)))
    }

    fn dispatch_remote_send(&self, mut request: SendRequest) -> Result<ResponseEnvelope, AtmError> {
        let remote_host = request
            .remote_host
            .take()
            .ok_or_else(|| AtmError::daemon_unavailable("remote host is missing"))?;
        let endpoint = self.resolve_remote_endpoint(&remote_host)?;
        self.peer_transport_runtime.send_to_endpoint(
            endpoint,
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)),
        )
    }

    fn resolve_remote_endpoint(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<SocketAddr, AtmError> {
        let port = self.resolve_remote_port()?;
        (remote_host.as_str(), port)
            .to_socket_addrs()
            .map_err(|source| {
                AtmError::address_parse(format!(
                    "failed to resolve remote host `{}` on port {}",
                    remote_host.as_str(),
                    port
                ))
                .with_recovery(
                    "Use a reachable literal IP address or resolvable hostname for the remote host before retrying the remote send.",
                )
                .with_source(source)
            })?
            .next()
            .ok_or_else(|| {
                AtmError::address_parse(format!(
                    "remote host `{}` did not resolve to any socket addresses",
                    remote_host.as_str()
                ))
                .with_recovery(
                    "Use a reachable literal IP address or resolvable hostname for the remote host before retrying the remote send.",
                )
            })
    }

    fn resolve_remote_port(&self) -> Result<u16, AtmError> {
        let enabled_ports = self
            .peer_interface_config_store
            .list_interfaces()?
            .into_iter()
            .filter(|row| row.enabled)
            .map(|row| row.port)
            .collect::<BTreeSet<_>>();
        match enabled_ports.len() {
            1 => Ok(*enabled_ports.iter().next().expect("one enabled port")),
            0 => self
                .peer_transport_runtime
                .bound_addr()?
                .map(|addr| addr.port())
                .ok_or_else(|| {
                    AtmError::daemon_unavailable(
                        "remote delivery is unavailable because no enabled cross-host interface port is configured",
                    )
                    .with_recovery(
                        "Add and enable one daemon interface row with `atm daemon interfaces add ...`, restart atm-daemon if required, and retry the remote send.",
                    )
                }),
            _ => Err(AtmError::validation(
                "remote delivery is ambiguous because multiple enabled cross-host ports are configured".to_string(),
            )
            .with_recovery(
                "Reduce the enabled daemon interface set to one shared port before retrying remote sends that specify only `<host>`.",
            )),
        }
    }

    fn compatibility_verdict(
        &self,
        preflight: atm_core::protocol::CompatibilityPreflight,
    ) -> Result<CompatibilityVerdict, AtmError> {
        let daemon_release = ReleaseVersion::current();
        if preflight.wire_version == atm_core::protocol::ATM_FRAME_VERSION_V1
            && preflight.client_release == daemon_release
        {
            return Ok(CompatibilityVerdict::Compatible { daemon_release });
        }
        Ok(CompatibilityVerdict::Incompatible {
            client_release: preflight.client_release,
            daemon_release,
            code: atm_core::error_codes::AtmErrorCode::ClientDaemonVersionIncompatible,
        })
    }
}
