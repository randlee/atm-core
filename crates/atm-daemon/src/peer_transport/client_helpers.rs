use super::*;

pub(super) fn peer_read_deadline_error(source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to apply remote peer read deadline")
        .with_recovery(
            "Restart atm-daemon and retry after the peer socket can accept bounded read deadlines again.",
        )
        .with_source(source)
}

pub(super) fn peer_write_deadline_error(source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to apply remote peer write deadline")
        .with_recovery(
            "Restart atm-daemon and retry after the peer socket can accept bounded write deadlines again.",
        )
        .with_source(source)
}

pub(super) fn peer_flush_error(source: std::io::Error) -> AtmError {
    AtmError::remote_delivery_outcome_unknown(
        "failed to flush the remote peer request frame before waiting for a response",
    )
    .with_source(source)
}

pub(super) fn peer_closed_before_response_error() -> AtmError {
    AtmError::remote_delivery_outcome_unknown(
        "remote peer closed the connection before returning a response frame",
    )
    .with_recovery(
        "Check the destination daemon or mailbox before retrying. If local durable replay is enabled, let the daemon resume the pending handoff rather than guessing success.",
    )
}

pub(super) fn peer_response_decode_error(error: AtmError) -> AtmError {
    AtmError::daemon_unavailable("failed to decode remote peer response frame")
        .with_recovery(
            "Align the peer daemon builds so both sides speak the same ATM daemon protocol before retrying the remote delivery.",
        )
        .with_source(error)
}

pub(super) fn peer_response_id_mismatch_error(
    response_id: atm_core::protocol::RequestId,
    request_id: atm_core::protocol::RequestId,
) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "remote peer response request_id {} did not match request_id {}",
        response_id, request_id
    ))
    .with_recovery(
        "Align the peer daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
    )
}

pub(super) fn daemon_terminate_flag() -> Result<Arc<AtomicBool>, AtmError> {
    Ok(crate::lifecycle_control::LifecycleControlSourceAdapter::install()?.terminate_flag())
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl ClientTransport for PeerClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let endpoint = self
            .endpoint
            .ok_or_else(remote_peer_endpoint_not_configured_error)?;
        self.send_to_endpoint(endpoint, request)
    }
}
