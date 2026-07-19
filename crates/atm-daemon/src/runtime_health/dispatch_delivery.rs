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
    schema::AtmMessageId,
    send::{
        SendCommandOutcome, SendOutcome, SendRequest, persist_remote_delivery_receipt_with_runtime,
        send_mail_with_runtime_and_post_send_emitter,
    },
};

use crate::peer_transport::delivery::SendOutcome as RemoteSendOutcome;

use super::{DaemonGraftPostSendPort, DaemonPostSendHookEmitter, DaemonRequestDispatcher};

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let graft_post_send_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let post_send_emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_post_send_port));
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                self.dispatch_compose_send(*request, &post_send_emitter)
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
        let deferred_receipt_message_id = AtmMessageId::new();
        match self.cross_host_delivery.deliver_remote(
            request.clone(),
            remote_host.clone(),
            deferred_receipt_message_id,
        ) {
            Ok(RemoteSendOutcome::Delivered(response)) => Ok(*response),
            Ok(RemoteSendOutcome::Deferred {
                receipt_message_id, ..
            }) => Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                build_remote_deferred_outcome(
                    &self.service_runtime,
                    &request,
                    &remote_host,
                    receipt_message_id,
                    "ATM deferred remote delivery because the cross-host path is not currently healthy. The daemon will retry this remote send in the background.",
                )?,
            ))),
            Ok(RemoteSendOutcome::RejectedTerminal(error)) => Err(error),
            Ok(RemoteSendOutcome::OutcomeUnknown {
                receipt_message_id, ..
            }) => Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                build_remote_deferred_outcome(
                    &self.service_runtime,
                    &request,
                    &remote_host,
                    receipt_message_id,
                    "ATM could not confirm the remote delivery outcome. The daemon retained the remote send for bounded replay and will report the final result through the sender inbox.",
                )?,
            ))),
            Err(error) => Err(error.into_atm_error()),
        }
    }

    fn compatibility_verdict(
        &self,
        preflight: atm_core::protocol::CompatibilityPreflight,
    ) -> Result<CompatibilityVerdict, AtmError> {
        let daemon_release = ReleaseVersion::current();
        if preflight.wire_version == atm_core::protocol::ATM_FRAME_VERSION_V1
            && preflight
                .client_release
                .is_same_compatibility_line(&daemon_release)
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

fn build_remote_deferred_outcome(
    runtime: &atm_core::LocalServiceRuntime,
    request: &SendRequest,
    remote_host: &atm_core::send::RemoteTargetHost,
    receipt_message_id: AtmMessageId,
    receipt_body: &str,
) -> Result<SendOutcome, AtmError> {
    let _receipt = persist_remote_delivery_receipt_with_runtime(
        runtime,
        &request.home_dir,
        &request.caller_team,
        &request.caller_identity,
        receipt_message_id,
        &request.to,
        remote_host.as_str(),
        request.task_id.clone(),
        receipt_body,
    )?;
    Ok(SendOutcome {
        action: atm_core::types::CommandAction::Send,
        team: request
            .to
            .team
            .clone()
            .unwrap_or_else(|| request.caller_team.clone()),
        agent: request.to.agent.clone(),
        sender: request.caller_identity.clone(),
        outcome: SendCommandOutcome::Deferred,
        message_id: receipt_message_id,
        receipt_message_id: Some(receipt_message_id),
        requires_ack: request.requires_ack || request.task_id.is_some(),
        task_id: request.task_id.clone(),
        summary: Some(format!(
            "ATM deferred remote delivery to {} via {}",
            request.to,
            remote_host.as_str()
        )),
        message: Some(receipt_body.to_string()),
        warnings: Vec::new(),
        dry_run: false,
    })
}
