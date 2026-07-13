use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::protocol::{
    self, CompatibilityPreflight, RequestEnvelope, RequestId as CoreRequestId, ResponseEnvelope,
};
use atm_daemon_client::{
    DaemonLocalIpcEndpoint, FramePayload, MessageKind, RequestId as DaemonRequestId, RpcEnvelope,
    exchange_envelope as daemon_exchange_envelope, try_connect as daemon_try_connect,
};

pub(crate) use atm_daemon_client::unexpected_response;

use crate::SAME_HOST_REQUEST_DEADLINE;

#[derive(Debug)]
pub(crate) struct GraftLocalIpcClientTransport {
    endpoint: DaemonLocalIpcEndpoint,
}

impl GraftLocalIpcClientTransport {
    pub(crate) fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    pub(crate) fn probe_connection(&self) -> Result<(), AtmError> {
        daemon_try_connect(&self.endpoint).map(|_| ())
    }

    pub(crate) fn round_trip(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        let (_, envelope) = encode_request_envelope(request.clone())?;
        let response = if request_requires_compatibility_verification(&request) {
            let mut verified = atm_daemon_client::verify_connection_compatibility(
                &self.endpoint,
                CompatibilityPreflight {
                    client_release: atm_daemon_client::ReleaseVersion::current(),
                    wire_version: protocol::ATM_FRAME_VERSION_V1,
                },
                SAME_HOST_REQUEST_DEADLINE,
            )?;
            verified.dispatch_write(&self.endpoint, envelope, SAME_HOST_REQUEST_DEADLINE)?
        } else {
            daemon_exchange_envelope(&self.endpoint, envelope, SAME_HOST_REQUEST_DEADLINE)?
        };
        decode_response_envelope(response)
    }
}

fn request_requires_compatibility_verification(request: &RequestEnvelope) -> bool {
    matches!(request, RequestEnvelope::Send(_) | RequestEnvelope::Clear(_))
}

fn encode_request_envelope(
    request: RequestEnvelope,
) -> Result<(CoreRequestId, RpcEnvelope), AtmError> {
    let request_id = protocol::next_request_id();
    let frame = protocol::request_to_frame_payload(request_id, request)?;
    Ok((
        request_id,
        RpcEnvelope::from_frame_payload(encode_daemon_frame(frame)?),
    ))
}

fn decode_response_envelope(envelope: RpcEnvelope) -> Result<ResponseEnvelope, AtmError> {
    let frame = decode_daemon_frame(envelope.into_frame_payload())?;
    let (_, response) = protocol::response_from_frame_payload(frame)?;
    Ok(response)
}

fn encode_daemon_frame(frame: protocol::FramePayload) -> Result<FramePayload, AtmError> {
    Ok(FramePayload {
        request_id: DaemonRequestId::new(frame.request_id.into_inner())?,
        message_kind: MessageKind::try_from(frame.message_kind.code())?,
        flags: frame.flags,
        bytes: frame.bytes,
    })
}

fn decode_daemon_frame(frame: FramePayload) -> Result<protocol::FramePayload, AtmError> {
    Ok(protocol::FramePayload {
        request_id: CoreRequestId::new(frame.request_id.into_inner())?,
        message_kind: protocol::MessageKind::try_from(frame.message_kind.code())?,
        flags: frame.flags,
        bytes: frame.bytes,
    })
}

impl boundary::sealed::Sealed for GraftLocalIpcClientTransport {}

impl ClientTransport for GraftLocalIpcClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.round_trip(request)
    }
}
