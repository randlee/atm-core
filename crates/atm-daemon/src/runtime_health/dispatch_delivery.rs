use std::sync::Arc;

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::{commit_ack_mutation, resolve_ack_send_request},
    boundary,
    clear::clear_mail_with_runtime,
    error::AtmError,
    list::list_mail,
    protocol::{CompatibilityVerdict, ReleaseVersion},
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    schema::AtmMessageId,
    send::{
        SendCommandOutcome, SendOutcome, SendRequest, SendRequestRoute,
        persist_remote_delivery_receipt_with_runtime, route_send_request,
        send_mail_with_runtime_and_post_send_emitter,
    },
};

use super::{DaemonGraftPostSendPort, DaemonPostSendHookEmitter, DaemonRequestDispatcher};

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let graft_post_send_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let post_send_emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_post_send_port));
        match request {
            RequestEnvelope::Send(request) => self.dispatch_send(*request, &post_send_emitter),
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
    fn dispatch_send(
        &self,
        request: SendRequest,
        post_send_emitter: &DaemonPostSendHookEmitter,
    ) -> Result<ResponseEnvelope, AtmError> {
        let (request, ack_mutation) =
            if request.acknowledges_message_id.is_some() && request.source_remote_host.is_none() {
                let (request, mutation) = resolve_ack_send_request(&self.service_runtime, request)?;
                (request, Some(mutation))
            } else {
                (request, None)
            };
        let response = match route_send_request(&request) {
            SendRequestRoute::Local => {
                let outcome = send_mail_with_runtime_and_post_send_emitter(
                    request,
                    self.observability.as_ref(),
                    &self.service_runtime,
                    post_send_emitter,
                )?;
                Ok(ResponseEnvelope::Send(outcome))
            }
            SendRequestRoute::Remote(remote_host) => {
                match self
                    .service_runtime
                    .deliver_remote_send_request(request.clone(), remote_host.clone())?
                {
                    boundary::RemoteSendDeliveryOutcome::Delivered(response) => Ok(*response),
                    boundary::RemoteSendDeliveryOutcome::Deferred {
                        receipt_message_id, ..
                    } => Ok(ResponseEnvelope::Send(build_remote_deferred_outcome(
                        &self.service_runtime,
                        &request,
                        &remote_host,
                        receipt_message_id,
                        "ATM deferred remote delivery because the cross-host path is not currently healthy. The daemon will retry this remote send in the background.",
                    )?)),
                    boundary::RemoteSendDeliveryOutcome::RejectedTerminal(error) => Err(error),
                    boundary::RemoteSendDeliveryOutcome::OutcomeUnknown {
                        receipt_message_id,
                        ..
                    } => Ok(ResponseEnvelope::Send(build_remote_deferred_outcome(
                        &self.service_runtime,
                        &request,
                        &remote_host,
                        receipt_message_id,
                        "ATM could not confirm the remote delivery outcome. The daemon retained the remote send for bounded replay and will report the final result through the sender inbox.",
                    )?)),
                }
            }
        }?;
        if let (Some(mutation), ResponseEnvelope::Send(outcome)) = (ack_mutation, &response)
            && matches!(outcome.outcome, SendCommandOutcome::Sent)
        {
            commit_ack_mutation(&self.service_runtime, mutation)?;
        }
        Ok(response)
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
