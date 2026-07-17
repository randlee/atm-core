use std::sync::Arc;

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::ack_mail_with_runtime_and_reply_delivery,
    boundary,
    clear::clear_mail_with_runtime,
    error::AtmError,
    list::list_mail,
    protocol::{CompatibilityVerdict, ReleaseVersion, SendRequestEnvelope, SendResponseEnvelope},
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    schema::AtmMessageId,
    send::{RemoteTargetHost, SendRequest, send_mail_with_runtime_and_post_send_emitter},
};

use crate::peer_transport::delivery::SendOutcome as RemoteSendOutcome;

use super::{DaemonGraftPostSendPort, DaemonPostSendHookEmitter, DaemonRequestDispatcher};

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        request: RequestEnvelope,
        peer_origin: Option<&str>,
    ) -> Result<ResponseEnvelope, AtmError> {
        let graft_post_send_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let post_send_emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_post_send_port));
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(mut request)) => {
                request.origin_host = peer_origin.map(RemoteTargetHost::parse).transpose()?;
                self.dispatch_compose_send(request, &post_send_emitter)
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail_with_runtime_and_reply_delivery(
                        request,
                        self.observability.as_ref(),
                        &self.service_runtime,
                        &post_send_emitter,
                        self,
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

impl boundary::AckReplyDeliveryPort for DaemonRequestDispatcher {
    fn deliver_reply(&self, reply: SendRequest) -> Result<AtmMessageId, AtmError> {
        let Some(remote_host) = reply.remote_host.clone() else {
            return Err(AtmError::daemon_unavailable(
                "cross-host acknowledgement reply delivery requires a remote host target",
            )
            .with_recovery(
                "Retry `atm ack` after confirming the acknowledged message carried a remote origin host.",
            ));
        };
        match self.cross_host_delivery.deliver_remote(reply, remote_host) {
            Ok(RemoteSendOutcome::Delivered(response)) => {
                extract_delivered_message_id(*response)
            }
            Ok(RemoteSendOutcome::Deferred) => Err(AtmError::daemon_unavailable(
                "remote acknowledgement reply delivery was deferred because the cross-host path is not currently healthy",
            )
            .with_recovery(
                "Verify the daemon interface rows and remote host reachability, then retry the acknowledgement after the cross-host path is healthy.",
            )),
            Ok(RemoteSendOutcome::RejectedTerminal(error)) => Err(error),
            Ok(RemoteSendOutcome::OutcomeUnknown) => Err(AtmError::daemon_unavailable(
                "remote acknowledgement reply delivery outcome is unknown",
            )
            .with_recovery(
                "Check the destination daemon and local replay state before retrying the acknowledgement.",
            )),
            Err(error) => Err(error.into_atm_error()),
        }
    }
}

/// Extract the remote-assigned message id from a delivered cross-host send
/// response. The reply's real id must come from the destination daemon's
/// response, never from a locally fabricated value.
fn extract_delivered_message_id(response: ResponseEnvelope) -> Result<AtmMessageId, AtmError> {
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => Ok(outcome.message_id),
        other => Err(AtmError::daemon_unavailable(format!(
            "remote acknowledgement reply delivery response did not include a delivered message id: {other:?}"
        ))
        .with_recovery(
            "Check the destination daemon's send response shape before retrying the acknowledgement.",
        )),
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
            Ok(RemoteSendOutcome::RejectedTerminal(error)) => Err(error),
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
