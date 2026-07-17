use std::net::SocketAddr;
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
    send::{PeerLoopbackHost, SendRequest, send_mail_with_runtime_and_post_send_emitter},
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
        if request.peer_loopback_host.is_some() {
            return self.dispatch_loopback_send(request);
        }
        let outcome = send_mail_with_runtime_and_post_send_emitter(
            request,
            self.observability.as_ref(),
            &self.service_runtime,
            post_send_emitter,
        )?;
        Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)))
    }

    fn dispatch_loopback_send(
        &self,
        mut request: SendRequest,
    ) -> Result<ResponseEnvelope, AtmError> {
        let host = request
            .peer_loopback_host
            .take()
            .ok_or_else(|| AtmError::daemon_unavailable("loopback peer host is missing"))?;
        let endpoint = self.resolve_loopback_endpoint(&host)?;
        request.peer_loopback_delivery = true;
        self.peer_transport_runtime.send_to_endpoint(
            endpoint,
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)),
        )
    }

    fn resolve_loopback_endpoint(&self, host: &PeerLoopbackHost) -> Result<SocketAddr, AtmError> {
        let bound_addr = self.peer_transport_runtime.bound_addr()?.ok_or_else(|| {
            AtmError::daemon_unavailable(
                "loopback peer delivery is unavailable because the daemon peer listener is not running",
            )
            .with_recovery(
                "Add and enable a loopback daemon interface row with `atm daemon interfaces ...`, restart atm-daemon, and retry the loopback send after the peer listener is bound.",
            )
        })?;
        let host_addr = if host.as_str().eq_ignore_ascii_case("localhost") {
            "127.0.0.1".parse().expect("loopback localhost parses")
        } else {
            host.as_str().parse().map_err(|error| {
                AtmError::address_parse(format!(
                    "invalid loopback host `{}`: {error}",
                    host.as_str()
                ))
                .with_recovery(
                    "Use `loopback@localhost` or `loopback@<literal-ip>` before retrying the loopback send.",
                )
            })?
        };
        Ok(SocketAddr::new(host_addr, bound_addr.port()))
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
