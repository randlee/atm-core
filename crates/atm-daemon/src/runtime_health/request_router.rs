use super::{DaemonRequestDispatcher, require_dispatch_budget};
use atm_core::{
    ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline, RequestEnvelope,
    api::PeerMessageArray,
    error::AtmError,
    provenance::{WriteIngress, validate_write_provenance},
};

impl ApiRouter for DaemonRequestDispatcher {
    fn route(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "daemon API request exceeded its same-host deadline before routing",
            ));
        }
        if let ApiRequest::PeerMessages(messages) = request {
            return self.route_peer_message_array(*messages, ingress, deadline);
        }
        let mut request = request.into_inner();
        if let RequestEnvelope::Write(write) = &mut request {
            if ingress == AuthenticatedIngress::Local {
                // The local IPC payload is caller-controlled. Peer provenance is
                // established only by the HTTPS adapter after authentication.
                // Strip a local claim before applying the canonical provenance gate.
                write.authenticated_source_host = None;
            }
            let write_ingress = match &ingress {
                AuthenticatedIngress::Local => WriteIngress::Local,
                AuthenticatedIngress::Peer => WriteIngress::Peer,
            };
            validate_write_provenance(write_ingress, write.provenance())?;
        }
        if matches!(request, RequestEnvelope::ReloadRuntimeView)
            && ingress != AuthenticatedIngress::Local
        {
            return Err(AtmError::validation(
                "runtime reload is available only through authenticated local IPC",
            ));
        }
        self.dispatch_with_deadline(request, deadline)
            .map(ApiResponse::new)
    }
}

impl DaemonRequestDispatcher {
    fn route_peer_message_array(
        &self,
        mut messages: PeerMessageArray,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if ingress != AuthenticatedIngress::Peer {
            return Err(AtmError::validation(
                "peer message arrays are available only through authenticated peer ingress",
            ));
        }
        messages.validate()?;
        require_dispatch_budget(deadline, false)?;
        let response = if messages.messages.len() == 1
            && messages.messages[0].acknowledges_message_id.is_some()
        {
            // An acknowledgement already owns one atomic source-and-reply
            // store transition. Retaining that canonical singleton path lets
            // AK.9 encode a direct ACK as a one-element peer array.
            let acknowledgement = messages.messages.pop().ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "validated one-item peer acknowledgement array became empty before routing",
                )
            })?;
            validate_write_provenance(WriteIngress::Peer, acknowledgement.provenance())?;
            self.route_peer_acknowledgement(acknowledgement, deadline)?
        } else {
            self.route_peer_messages(messages.messages, deadline)?
        };
        require_dispatch_budget(deadline, true)?;
        Ok(ApiResponse::new(response))
    }
}
