use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, RequestId, ResponseEnvelope};
use atm_daemon_client::{
    DaemonLocalIpcEndpoint, exchange as daemon_exchange, try_connect as daemon_try_connect,
};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;
use std::io::Write;

use crate::{ADVISORY_STREAM_READ_DEADLINE, SAME_HOST_REQUEST_DEADLINE};

pub(crate) use atm_daemon_client::unexpected_response;

#[derive(Debug)]
pub(crate) struct GraftLocalIpcClientTransport {
    endpoint: DaemonLocalIpcEndpoint,
}

pub(crate) struct ActiveAdvisoryStream {
    pub(crate) stream: LocalSocketStream,
    pub(crate) request_id: RequestId,
}

impl GraftLocalIpcClientTransport {
    pub(crate) fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    pub(crate) fn try_connect(&self) -> Result<LocalSocketStream, AtmError> {
        daemon_try_connect(&self.endpoint)
    }

    /// This function performs blocking IPC I/O. Callers in async contexts must
    /// wrap this in `tokio::task::spawn_blocking`.
    pub(crate) fn exchange(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        daemon_exchange(&self.endpoint, request, SAME_HOST_REQUEST_DEADLINE)
    }

    pub(crate) fn open_advisory_stream(
        &self,
        request: atm_core::graft::AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError> {
        let mut stream = self.try_connect()?;
        stream
            .set_send_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable(
                    "failed to configure graft advisory-stream write timeout",
                )
                .with_source(source)
            })?;
        let request_id = atm_core::protocol::next_request_id();
        let frame = atm_core::protocol::request_to_frame_payload(
            request_id,
            RequestEnvelope::AdvisoryStream(request),
        )?;
        atm_core::protocol::write_frame(
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

impl boundary::sealed::Sealed for GraftLocalIpcClientTransport {}

impl ClientTransport for GraftLocalIpcClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.exchange(request)
    }
}
