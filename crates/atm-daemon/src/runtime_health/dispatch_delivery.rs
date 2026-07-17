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
    send::{SendRequest, send_mail_with_runtime_and_post_send_emitter},
};

use crate::peer_transport::delivery::{RemoteFailureKind, SendOutcome as RemoteSendOutcome};

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
        let Some(remote_host) = request.remote_host.clone() else {
            let outcome = send_mail_with_runtime_and_post_send_emitter(
                request,
                self.observability.as_ref(),
                &self.service_runtime,
                post_send_emitter,
            )?;
            return Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)));
        };
        match self.cross_host_delivery.deliver_remote(request, remote_host) {
            Ok(RemoteSendOutcome::Delivered(response)) => Ok(*response),
            Ok(RemoteSendOutcome::Deferred) => Err(AtmError::daemon_unavailable(
                "remote delivery was deferred because the cross-host path is not currently healthy",
            )
            .with_recovery(
                "Verify the daemon interface rows and remote host reachability, then retry the remote send after the cross-host path is healthy.",
            )),
            Ok(RemoteSendOutcome::RejectedTerminal(kind)) => Err(
                remote_failure_kind_error(kind)
            ),
            Ok(RemoteSendOutcome::OutcomeUnknown) => Err(AtmError::daemon_unavailable(
                "remote delivery outcome is unknown",
            )
            .with_recovery(
                "Check the destination daemon and local replay state before retrying the remote send.",
            )),
            Err(error) => Err(error.into_atm_error()),
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

fn remote_failure_kind_error(kind: RemoteFailureKind) -> AtmError {
    match kind {
        RemoteFailureKind::TerminalProtocolRejected => AtmError::validation(
            "remote daemon rejected the cross-host request protocol".to_string(),
        )
        .with_recovery(
            "Align the sender and receiver daemon protocol versions before retrying.",
        ),
        RemoteFailureKind::TerminalMalformedTarget => {
            AtmError::address_parse("remote target is malformed").with_recovery(
                "Use `atm send <agent>@<team>.<host> ...` or `atm send <agent>@<team> --host <host> ...` with a valid host token before retrying.",
            )
        }
    }
}
