use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, RequestId, ResponseEnvelope};
use atm_daemon_client::DaemonLocalIpcEndpoint;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;
use std::io::Write;

use crate::{ADVISORY_STREAM_READ_DEADLINE, SAME_HOST_REQUEST_DEADLINE};

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
        LocalSocketStream::connect(atm_core::protocol::daemon_local_ipc_name_from_path(
            self.endpoint.as_ref(),
        )?)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to connect to daemon local IPC endpoint at {}",
                self.endpoint.display()
            ))
            .with_source(source)
        })
    }

    /// This function performs blocking IPC I/O. Callers in async contexts must
    /// wrap this in `tokio::task::spawn_blocking`.
    pub(crate) fn exchange(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let mut stream = self.try_connect()?;
        stream
            .set_send_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure graft local IPC write timeout")
                    .with_source(source)
            })?;
        stream
            .set_recv_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure graft local IPC read timeout")
                    .with_source(source)
            })?;
        let request_id = atm_core::protocol::next_request_id();
        let frame = atm_core::protocol::request_to_frame_payload(request_id, request)?;
        atm_core::protocol::write_frame(
            &mut stream,
            &frame,
            "failed to write graft daemon request frame",
        )?;
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush graft daemon request frame")
                .with_source(source)
        })?;
        let response_frame = atm_core::protocol::read_frame(
            &mut stream,
            "failed to read graft daemon response frame",
            "graft daemon response frame exceeded the maximum supported size",
        )?
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon closed the local IPC connection before returning a graft response frame",
            )
            .with_recovery(
                "Retry the graft request after atm-daemon reaches serving state and inspect daemon logs if the problem persists.",
            )
        })?;
        let (response_id, response) =
            atm_core::protocol::response_from_frame_payload(response_frame)?;
        if response_id != request_id {
            return Err(AtmError::daemon_unavailable(format!(
                "graft daemon response request_id {} did not match request_id {}",
                response_id, request_id
            ))
            .with_recovery(
                "Align the embedding host, atm-graft, and atm-daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
            ));
        }
        Ok(response)
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

pub(crate) fn unexpected_response(command: &str, response: ResponseEnvelope) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
    .with_recovery(
        "Retry the graft operation once. If the mismatch persists, inspect daemon/client version alignment and retained daemon logs before retrying again.",
    )
}
