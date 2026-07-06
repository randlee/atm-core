use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::protocol::{self, RequestEnvelope, RequestId as CoreRequestId, ResponseEnvelope};
use atm_daemon_client::graft_rpc;
use atm_daemon_client::{
    DaemonLocalIpcEndpoint, FramePayload, MessageKind, RequestId as DaemonRequestId, RpcEnvelope,
    exchange_envelope as daemon_exchange_envelope, try_connect as daemon_try_connect,
};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;
use std::io::Write;

use crate::{
    ADVISORY_STREAM_READ_DEADLINE, AdvisoryFetchRequest, AdvisoryFetchResponse,
    AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse,
    AdvisorySessionUnregistrationRequest, AdvisorySessionUnregistrationResponse,
    AdvisoryStreamRequest, AdvisoryTransport, SAME_HOST_REQUEST_DEADLINE,
};

pub(crate) use atm_daemon_client::unexpected_response;

#[derive(Debug)]
pub(crate) struct GraftLocalIpcClientTransport {
    endpoint: DaemonLocalIpcEndpoint,
}

pub(crate) struct ActiveAdvisoryStream {
    pub(crate) stream: LocalSocketStream,
    pub(crate) request_id: DaemonRequestId,
}

impl GraftLocalIpcClientTransport {
    pub(crate) fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    pub(crate) fn probe_connection(&self) -> Result<LocalSocketStream, AtmError> {
        daemon_try_connect(&self.endpoint)
    }

    /// This function performs blocking IPC I/O. Callers in async contexts must
    /// wrap this in `tokio::task::spawn_blocking`.
    pub(crate) fn round_trip(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        let (_, envelope) = encode_request_envelope(request)?;
        let response =
            daemon_exchange_envelope(&self.endpoint, envelope, SAME_HOST_REQUEST_DEADLINE)?;
        decode_response_envelope(response)
    }

    pub(crate) fn open_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError> {
        let mut stream = self.probe_connection()?;
        stream
            .set_send_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable(
                    "failed to configure graft advisory-stream write timeout",
                )
                .with_source(source)
            })?;
        let request_id = DaemonRequestId::new(protocol::next_request_id().into_inner())?;
        let frame = graft_rpc::request_to_frame_payload(
            request_id,
            graft_rpc::RequestEnvelope::AdvisoryStream(request),
        )?;
        graft_rpc::write_frame(
            &mut stream,
            &frame,
            "failed to write graft advisory-stream request frame",
        )?;
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush graft advisory-stream request frame")
                .with_source(source)
        })?;
        // The live advisory stream is read-only after the registration
        // handshake, so the write timeout is cleared before the receive loop.
        stream.set_send_timeout(None).map_err(|source| {
            AtmError::daemon_unavailable(
                "failed to clear graft advisory-stream write timeout after request publish",
            )
            .with_source(source)
        })?;
        stream
            .set_recv_timeout(Some(ADVISORY_STREAM_READ_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable(
                    "failed to configure bounded graft advisory-stream read timeout",
                )
                .with_source(source)
            })?;
        Ok(ActiveAdvisoryStream { stream, request_id })
    }
}

impl AdvisoryTransport for GraftLocalIpcClientTransport {
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        match self.round_trip_graft(graft_rpc::RequestEnvelope::AdvisoryRegister(request))? {
            graft_rpc::ResponseEnvelope::AdvisoryRegister(response) => Ok(response),
            graft_rpc::ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            other => Err(unexpected_response("graft register", other)),
        }
    }

    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        match self.round_trip_graft(graft_rpc::RequestEnvelope::AdvisoryUnregister(request))? {
            graft_rpc::ResponseEnvelope::AdvisoryUnregister(response) => Ok(response),
            graft_rpc::ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            other => Err(unexpected_response("graft unregister", other)),
        }
    }

    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        match self.round_trip_graft(graft_rpc::RequestEnvelope::AdvisoryFetch(request))? {
            graft_rpc::ResponseEnvelope::AdvisoryFetch(response) => Ok(response),
            graft_rpc::ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            other => Err(unexpected_response("graft fetch", other)),
        }
    }

    fn drain_nudges(
        &self,
        request: crate::AdvisoryDrainRequest,
    ) -> Result<crate::AdvisoryDrainResponse, AtmError> {
        match self.round_trip_graft(graft_rpc::RequestEnvelope::AdvisoryDrain(request))? {
            graft_rpc::ResponseEnvelope::AdvisoryDrain(response) => Ok(response),
            graft_rpc::ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            other => Err(unexpected_response("graft drain", other)),
        }
    }

    fn supports_live_advisory_stream(&self) -> bool {
        true
    }

    fn open_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError> {
        GraftLocalIpcClientTransport::open_advisory_stream(self, request)
    }
}

impl GraftLocalIpcClientTransport {
    fn round_trip_graft(
        &self,
        request: graft_rpc::RequestEnvelope,
    ) -> Result<graft_rpc::ResponseEnvelope, AtmError> {
        let mut stream = self.probe_connection()?;
        stream
            .set_send_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable(
                    "failed to configure graft local IPC request write timeout",
                )
                .with_source(source)
            })?;
        stream
            .set_recv_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable(
                    "failed to configure graft local IPC response read timeout",
                )
                .with_source(source)
            })?;
        let request_id = DaemonRequestId::new(protocol::next_request_id().into_inner())?;
        let frame = graft_rpc::request_to_frame_payload(request_id, request)?;
        graft_rpc::write_frame(
            &mut stream,
            &frame,
            "failed to write graft local IPC request",
        )?;
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush graft local IPC request")
                .with_source(source)
        })?;
        let response = graft_rpc::read_frame(
            &mut stream,
            "failed to read graft local IPC response",
            "graft local IPC response frame exceeded the maximum supported size",
        )?
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon closed the graft local IPC connection before returning a response frame",
            )
            .with_recovery(
                "Retry the graft request after atm-daemon reaches serving state and inspect daemon logs if the problem persists.",
            )
        })?;
        let (_, response) = graft_rpc::response_from_raw_parts(
            response.request_id,
            response.message_kind.code(),
            response.flags,
            response.bytes,
        )?;
        Ok(response)
    }
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
