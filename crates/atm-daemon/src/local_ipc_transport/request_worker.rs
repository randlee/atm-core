use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atm_core::boundary::{AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::protocol::{
    JsonAtmProtocolCodec, ProtocolErrorEnvelope, RequestEnvelope, RequestId, ResponseEnvelope,
};
use atm_daemon_client::{RequestId as DaemonRequestId, graft_rpc};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream;

use crate::GraftRequestDispatcher;
use crate::active_connection_registry::{ActiveConnectionRegistry, TrackedDispatchHandle};

#[cfg(test)]
use super::PreparedRuntimeServer;
use super::{MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE, write_shutdown_response};

type DispatchResultRx = std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>;
type DispatchCompletionRx = std::sync::mpsc::Receiver<()>;
type DispatchWorkerHandle = std::thread::JoinHandle<()>;
type DispatchWorker = (DispatchResultRx, DispatchCompletionRx, DispatchWorkerHandle);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestExecutionRisk {
    ReadOnly,
    SideEffecting,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeadlineSupport {
    Applied,
    Unsupported,
}

#[cfg(windows)]
const WINDOWS_READ_HELPER_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(100);

enum ReadRequestFrameResult {
    EndOfStream,
    Frame {
        stream: LocalSocketStream,
        frame: RawRequestFrame,
    },
    #[cfg(windows)]
    TimedOut,
}

struct RawRequestFrame {
    request_id: u64,
    message_kind_code: u16,
    flags: u16,
    bytes: Vec<u8>,
}

pub(super) fn handle_connection(
    mut stream: LocalSocketStream,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    graft_dispatcher: Option<Arc<dyn GraftRequestDispatcher + Send + Sync>>,
    force_shutdown: &AtomicBool,
    registry: Arc<ActiveConnectionRegistry>,
    codec: JsonAtmProtocolCodec,
) -> Result<(), AtmError> {
    if force_shutdown.load(Ordering::SeqCst) {
        return write_shutdown_response(&mut stream, &codec).map(|_| ());
    }
    let read_deadline_support = configure_request_deadlines(&stream)?;

    let frame = match read_request_frame_with_deadline(
        stream,
        force_shutdown,
        read_deadline_support,
    )? {
        ReadRequestFrameResult::EndOfStream => return Ok(()),
        ReadRequestFrameResult::Frame {
            stream: resumed_stream,
            frame,
        } => {
            stream = resumed_stream;
            frame
        }
        #[cfg(windows)]
        ReadRequestFrameResult::TimedOut if force_shutdown.load(Ordering::SeqCst) => {
            tracing::info!(
                subsystem = "local_ipc",
                action = "request_read",
                outcome = "forced_shutdown",
                "daemon forced shutdown interrupted a Windows same-host request read before a complete frame arrived"
            );
            return Ok(());
        }
        #[cfg(windows)]
        ReadRequestFrameResult::TimedOut => {
            tracing::warn!(
                subsystem = "local_ipc",
                action = "request_read",
                outcome = "deadline_exceeded",
                deadline_ms = REQUEST_DEADLINE.as_millis() as u64,
                "daemon local IPC request read exceeded the runtime deadline; closing the stalled connection"
            );
            return Ok(());
        }
    };
    tracing::debug!(
        max_daemon_frame_bytes = atm_core::protocol::MAX_DAEMON_FRAME_BYTES,
        "daemon request frame accepted under configured size cap"
    );
    if graft_rpc::is_graft_message_kind_code(frame.message_kind_code) {
        let graft_dispatcher = graft_dispatcher.ok_or_else(|| {
            AtmError::daemon_unavailable(
                "graft local IPC request reached a daemon transport without a graft dispatcher",
            )
            .with_recovery(
                "Restart atm-daemon through RuntimeComposition::start() so graft-only IPC routes through the daemon-private graft dispatcher.",
            )
        })?;
        let request_id = DaemonRequestId::new(frame.request_id)?;
        let (_, request) = graft_rpc::request_from_raw_parts(
            request_id,
            frame.message_kind_code,
            frame.flags,
            frame.bytes,
        )?;
        if let graft_rpc::RequestEnvelope::AdvisoryStream(request) = request {
            return dispatch_advisory_stream(
                &mut stream,
                graft_dispatcher.as_ref(),
                force_shutdown,
                request_id,
                request,
            );
        }
        let response = dispatch_graft_request(request_id, request, graft_dispatcher, &registry)?;
        write_graft_response(&mut stream, request_id, response)?;
        registry.reap_finished_dispatches()?;
        return Ok(());
    }
    let frame = atm_core::protocol::FramePayload {
        request_id: RequestId::new(frame.request_id)?,
        message_kind: atm_core::protocol::MessageKind::try_from(frame.message_kind_code)?,
        flags: frame.flags,
        bytes: frame.bytes,
    };
    let (request_id, request) = codec.request_from_frame(frame)?;

    let response = dispatch_request(request_id, request, dispatcher, &registry)?;
    write_response(&mut stream, &codec, request_id, response)?;
    registry.reap_finished_dispatches()?;
    Ok(())
}

fn read_request_frame_with_deadline(
    mut stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    read_deadline_support: DeadlineSupport,
) -> Result<ReadRequestFrameResult, AtmError> {
    #[cfg(windows)]
    if read_deadline_support == DeadlineSupport::Unsupported {
        return read_request_frame_with_helper(stream, force_shutdown);
    }
    #[cfg(not(windows))]
    let _ = (force_shutdown, read_deadline_support);

    match read_request_frame(&mut stream)? {
        None => Ok(ReadRequestFrameResult::EndOfStream),
        Some(frame) => Ok(ReadRequestFrameResult::Frame { stream, frame }),
    }
}

#[cfg(windows)]
fn read_request_frame_with_helper(
    mut stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
) -> Result<ReadRequestFrameResult, AtmError> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("local-ipc-read-helper".to_string())
        .spawn(move || {
            let result = read_request_frame(&mut stream);
            let _ = result_tx.send((stream, result));
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn Windows named-pipe request read helper")
                .with_recovery(
                    "Restart the daemon; the same-host Windows request read helper could not be created.",
                )
                .with_source(source)
        })?;

    let started = std::time::Instant::now();
    loop {
        let remaining = REQUEST_DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() || force_shutdown.load(Ordering::SeqCst) {
            return Ok(ReadRequestFrameResult::TimedOut);
        }
        let poll = std::cmp::min(remaining, WINDOWS_READ_HELPER_POLL_INTERVAL);
        match result_rx.recv_timeout(poll) {
            Ok((stream, Ok(Some(frame)))) => {
                return Ok(ReadRequestFrameResult::Frame { stream, frame });
            }
            Ok((_, Ok(None))) => return Ok(ReadRequestFrameResult::EndOfStream),
            Ok((_, Err(error))) => return Err(error),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AtmError::daemon_unavailable(
                    "Windows named-pipe request read helper disconnected unexpectedly",
                )
                .with_recovery(
                    "Restart the daemon; the same-host Windows request read helper stopped before it could return a frame result.",
                ));
            }
        }
    }
}

fn read_request_frame(stream: &mut LocalSocketStream) -> Result<Option<RawRequestFrame>, AtmError> {
    let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
    let read = stream.read(&mut header[..1]).map_err(|source| {
        AtmError::daemon_unavailable("failed to read daemon request frame header")
            .with_source(source)
    })?;
    if read == 0 {
        return Ok(None);
    }
    stream.read_exact(&mut header[1..]).map_err(|source| {
        AtmError::daemon_unavailable("failed to read daemon request frame header")
            .with_source(source)
    })?;
    let magic = u32::from_be_bytes(header[0..4].try_into().expect("magic"));
    if magic != atm_core::protocol::ATM_FRAME_MAGIC {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame magic 0x{magic:08x}"
        ))
        .with_recovery(
            "Retry with an ATM client and daemon build that both speak the documented ATM daemon protocol.",
        ));
    }
    let version = u16::from_be_bytes(header[4..6].try_into().expect("version"));
    if version != atm_core::protocol::ATM_FRAME_VERSION_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame version {version}"
        ))
        .with_recovery(
            "Align the CLI, atm-graft, and daemon builds so both sides use the same ATM daemon protocol version before retrying.",
        ));
    }
    let flags = u16::from_be_bytes(header[8..10].try_into().expect("flags"));
    let request_id = u64::from_be_bytes(header[10..18].try_into().expect("request id"));
    let payload_length = u32::from_be_bytes(header[18..22].try_into().expect("payload")) as usize;
    if payload_length > atm_core::protocol::MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "daemon request frame exceeded the maximum supported size",
        )
        .with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }
    let mut bytes = vec![0u8; payload_length];
    stream.read_exact(&mut bytes).map_err(|source| {
        AtmError::daemon_unavailable("failed to read daemon request frame").with_source(source)
    })?;
    Ok(Some(RawRequestFrame {
        request_id,
        message_kind_code: u16::from_be_bytes(header[6..8].try_into().expect("kind")),
        flags,
        bytes,
    }))
}

fn configure_request_deadlines(stream: &LocalSocketStream) -> Result<DeadlineSupport, AtmError> {
    let read_deadline_support = apply_optional_deadline(
        stream.set_recv_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon request read deadline",
    )?;
    apply_optional_deadline(
        stream.set_send_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon response write deadline",
    )?;
    Ok(read_deadline_support)
}

fn dispatch_advisory_stream(
    stream: &mut LocalSocketStream,
    dispatcher: &dyn GraftRequestDispatcher,
    force_shutdown: &AtomicBool,
    request_id: DaemonRequestId,
    request: atm_daemon_client::graft_rpc::AdvisoryStreamRequest,
) -> Result<(), AtmError> {
    apply_deadline_contract(
        stream.set_send_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon advisory-stream write deadline",
    )?;
    let mut sink = LocalIpcAdvisoryStreamSink {
        stream,
        request_id,
        force_shutdown,
    };
    dispatcher.dispatch_graft_stream(request, &mut sink)
}

fn apply_deadline_contract(
    result: std::io::Result<()>,
    message: &'static str,
) -> Result<(), AtmError> {
    match apply_optional_deadline(result, message)? {
        DeadlineSupport::Applied | DeadlineSupport::Unsupported => Ok(()),
    }
}

fn apply_optional_deadline(
    result: std::io::Result<()>,
    message: &'static str,
) -> Result<DeadlineSupport, AtmError> {
    match result {
        Ok(()) => Ok(DeadlineSupport::Applied),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => {
            Ok(DeadlineSupport::Unsupported)
        }
        Err(source) => Err(AtmError::daemon_unavailable(message)
            .with_recovery(
                "Restart the daemon; the same-host request socket could not apply its bounded deadline.",
            )
            .with_source(source)),
    }
}

fn dispatch_request(
    request_id: RequestId,
    request: RequestEnvelope,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
) -> Result<ResponseEnvelope, AtmError> {
    let execution_risk = request_execution_risk(&request);
    let (result_rx, completion_rx, dispatch_handle) =
        spawn_dispatch_worker(request, dispatcher, Arc::clone(registry))?;
    registry.push_dispatch_handle(
        TrackedDispatchHandle {
            completion_rx,
            join_handle: dispatch_handle,
        },
        MAX_CONCURRENT_CONNECTIONS,
    )?;
    Ok(await_dispatch_response(
        request_id,
        execution_risk,
        result_rx,
    ))
}

type GraftDispatchResultRx =
    std::sync::mpsc::Receiver<Result<graft_rpc::ResponseEnvelope, AtmError>>;
type GraftDispatchWorker = (
    GraftDispatchResultRx,
    DispatchCompletionRx,
    DispatchWorkerHandle,
);

fn dispatch_graft_request(
    request_id: DaemonRequestId,
    request: graft_rpc::RequestEnvelope,
    dispatcher: Arc<dyn GraftRequestDispatcher + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
) -> Result<graft_rpc::ResponseEnvelope, AtmError> {
    let execution_risk = request_execution_risk_graft(&request);
    let (result_rx, completion_rx, dispatch_handle) =
        spawn_graft_dispatch_worker(request, dispatcher, Arc::clone(registry))?;
    registry.push_dispatch_handle(
        TrackedDispatchHandle {
            completion_rx,
            join_handle: dispatch_handle,
        },
        MAX_CONCURRENT_CONNECTIONS,
    )?;
    Ok(await_graft_dispatch_response(
        request_id,
        execution_risk,
        result_rx,
    ))
}

fn spawn_graft_dispatch_worker(
    request: graft_rpc::RequestEnvelope,
    dispatcher: Arc<dyn GraftRequestDispatcher + Send + Sync>,
    dispatch_registry: Arc<ActiveConnectionRegistry>,
) -> Result<GraftDispatchWorker, AtmError> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    let dispatch_handle = std::thread::Builder::new()
        .name("local-ipc-graft-dispatch".to_string())
        .spawn(move || {
            let _dispatch_work = dispatch_registry.register_dispatch_work();
            let _ = result_tx.send(dispatcher.dispatch_graft(request));
            let _ = completion_tx.send(());
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn graft local IPC dispatch worker")
                .with_recovery(
                    "Retry the graft request after atm-daemon restarts; the same-host graft request worker could not be created.",
                )
                .with_source(source)
        })?;
    Ok((result_rx, completion_rx, dispatch_handle))
}

fn await_graft_dispatch_response(
    request_id: DaemonRequestId,
    execution_risk: RequestExecutionRisk,
    result_rx: std::sync::mpsc::Receiver<Result<graft_rpc::ResponseEnvelope, AtmError>>,
) -> graft_rpc::ResponseEnvelope {
    match result_rx.recv_timeout(REQUEST_DEADLINE) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => graft_rpc::ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(&error)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!(
                subsystem = "local_ipc",
                action = "graft_dispatch",
                outcome = "deadline_exceeded",
                request_id = %request_id,
                deadline_ms = REQUEST_DEADLINE.as_millis(),
                "daemon graft request dispatcher exceeded the runtime deadline"
            );
            dispatch_timeout_response_graft(execution_risk)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => graft_rpc::ResponseEnvelope::Error(
            ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable(
                    "daemon graft request dispatcher stopped before returning a response",
                )
                .with_recovery(
                    "Retry the graft request after the daemon finishes recovering the request runtime.",
                ),
            ),
        ),
    }
}

fn spawn_dispatch_worker(
    request: RequestEnvelope,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    dispatch_registry: Arc<ActiveConnectionRegistry>,
) -> Result<DispatchWorker, AtmError> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    // The same-host dispatch worker is intentionally allowed to run to natural
    // completion after a caller-side timeout. Requests may have already crossed
    // durable or side-effecting boundaries, so forcing cancellation at
    // REQUEST_DEADLINE would create a more ambiguous contract than the current
    // "response timed out; work may still complete in the background" surface.
    // The completion channel plus tracked join handle keep that bounded worker
    // visible to shutdown/reap logic instead of leaking silently.
    let dispatch_handle = std::thread::Builder::new()
        .name("local-ipc-dispatch".to_string())
        .spawn(move || {
            let _dispatch_work = dispatch_registry.register_dispatch_work();
            let _ = result_tx.send(dispatcher.dispatch(request));
            let _ = completion_tx.send(());
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon local IPC dispatch worker")
                .with_recovery(
                    "Retry the ATM command after atm-daemon restarts; the same-host request worker could not be created.",
                )
                .with_source(source)
        })?;
    Ok((result_rx, completion_rx, dispatch_handle))
}

fn await_dispatch_response(
    request_id: RequestId,
    execution_risk: RequestExecutionRisk,
    result_rx: std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>,
) -> ResponseEnvelope {
    match result_rx.recv_timeout(REQUEST_DEADLINE) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(&error)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!(
                subsystem = "local_ipc",
                action = "dispatch",
                outcome = "deadline_exceeded",
                request_id = %request_id,
                deadline_ms = REQUEST_DEADLINE.as_millis(),
                "daemon request dispatcher exceeded the runtime deadline"
            );
            dispatch_timeout_response(execution_risk)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable(
                    "daemon request dispatcher stopped before returning a response",
                )
                .with_recovery(
                    "Retry the ATM command after the daemon finishes recovering the request runtime.",
                ),
            ))
        }
    }
}

fn request_execution_risk(request: &RequestEnvelope) -> RequestExecutionRisk {
    match request {
        RequestEnvelope::List(_) | RequestEnvelope::Receive(_) | RequestEnvelope::Doctor(_) => {
            RequestExecutionRisk::ReadOnly
        }
        RequestEnvelope::Send(_) | RequestEnvelope::Heartbeat(_) | RequestEnvelope::Clear(_) => {
            RequestExecutionRisk::SideEffecting
        }
    }
}

fn request_execution_risk_graft(request: &graft_rpc::RequestEnvelope) -> RequestExecutionRisk {
    match request {
        graft_rpc::RequestEnvelope::AdvisoryFetch(_) => RequestExecutionRisk::ReadOnly,
        graft_rpc::RequestEnvelope::AdvisoryRegister(_)
        | graft_rpc::RequestEnvelope::AdvisoryUnregister(_)
        | graft_rpc::RequestEnvelope::AdvisoryDrain(_)
        | graft_rpc::RequestEnvelope::AdvisoryStream(_) => RequestExecutionRisk::SideEffecting,
    }
}

fn dispatch_timeout_response(execution_risk: RequestExecutionRisk) -> ResponseEnvelope {
    let error = match execution_risk {
        RequestExecutionRisk::ReadOnly => AtmError::daemon_unavailable(
            "daemon request exceeded the 3s runtime deadline; retry the read-only ATM command after the same-host daemon catches up",
        ),
        RequestExecutionRisk::SideEffecting => AtmError::daemon_may_have_executed(
            "daemon request exceeded the 3s runtime deadline after side-effecting work may have started",
        ),
    };
    ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(&error))
}

fn dispatch_timeout_response_graft(
    execution_risk: RequestExecutionRisk,
) -> graft_rpc::ResponseEnvelope {
    let error = match execution_risk {
        RequestExecutionRisk::ReadOnly => AtmError::daemon_unavailable(
            "daemon graft request exceeded the 3s runtime deadline; retry the read-only graft command after the same-host daemon catches up",
        ),
        RequestExecutionRisk::SideEffecting => AtmError::daemon_may_have_executed(
            "daemon graft request exceeded the 3s runtime deadline after side-effecting work may have started",
        ),
    };
    graft_rpc::ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(&error))
}

fn write_response(
    stream: &mut LocalSocketStream,
    codec: &JsonAtmProtocolCodec,
    request_id: RequestId,
    response: ResponseEnvelope,
) -> Result<(), AtmError> {
    let frame = codec.response_to_frame(request_id, response)?;
    atm_core::protocol::write_frame(stream, &frame, "failed to write daemon response frame")?;
    std::io::Write::flush(stream).map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon response frame")
            .with_recovery(
                "Retry the ATM command after the daemon finishes recovering the same-host request runtime.",
            )
            .with_source(source)
    })
}

fn write_graft_response(
    stream: &mut LocalSocketStream,
    request_id: DaemonRequestId,
    response: graft_rpc::ResponseEnvelope,
) -> Result<(), AtmError> {
    let frame = graft_rpc::response_to_frame_payload(request_id, response)?;
    graft_rpc::write_frame(
        stream,
        &frame,
        "failed to write graft daemon response frame",
    )?;
    std::io::Write::flush(stream).map_err(|source| {
        AtmError::daemon_unavailable("failed to flush graft daemon response frame")
            .with_recovery(
                "Retry the graft request after the daemon finishes recovering the same-host graft runtime.",
            )
            .with_source(source)
    })
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(super) fn install_injected_accept_error_for_test(
    runtime: &mut PreparedRuntimeServer,
    signal: std::sync::mpsc::SyncSender<()>,
) {
    runtime.accept_error_inject = Some(signal);
}

struct LocalIpcAdvisoryStreamSink<'a> {
    stream: &'a mut LocalSocketStream,
    request_id: DaemonRequestId,
    force_shutdown: &'a AtomicBool,
}

impl crate::GraftStreamSink for LocalIpcAdvisoryStreamSink<'_> {
    fn emit(&mut self, response: graft_rpc::ResponseEnvelope) -> Result<(), AtmError> {
        let frame = graft_rpc::response_to_frame_payload(self.request_id, response)?;
        graft_rpc::write_frame(
            self.stream,
            &frame,
            "failed to write daemon advisory-stream response frame",
        )?;
        std::io::Write::flush(&mut self.stream).map_err(|source| {
            AtmError::daemon_unavailable("failed to flush daemon advisory-stream response frame")
                .with_recovery(
                    "Retry graft activation after atm-daemon returns to a healthy serving state.",
                )
                .with_source(source)
        })
    }

    fn stop_requested(&self) -> bool {
        self.force_shutdown.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::{RequestExecutionRisk, dispatch_timeout_response, request_execution_risk};
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::DoctorQuery;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::protocol::{ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope};

    #[test]
    fn side_effecting_timeout_returns_may_have_executed_code() {
        let response = dispatch_timeout_response(RequestExecutionRisk::SideEffecting);
        let ResponseEnvelope::Error(ProtocolErrorEnvelope { code, .. }) = response else {
            panic!("expected error envelope");
        };
        assert_eq!(code, AtmErrorCode::DaemonMayHaveExecuted);
    }

    #[test]
    fn read_only_timeout_returns_retryable_daemon_unavailable_code() {
        let response = dispatch_timeout_response(RequestExecutionRisk::ReadOnly);
        let ResponseEnvelope::Error(ProtocolErrorEnvelope { code, .. }) = response else {
            panic!("expected error envelope");
        };
        assert_eq!(code, AtmErrorCode::DaemonUnavailable);
    }

    #[test]
    fn request_execution_risk_classifies_clear_as_side_effecting() {
        let tmp = std::env::temp_dir();
        let request = RequestEnvelope::Clear(ClearQuery {
            home_dir: tmp.clone(),
            current_dir: tmp,
            caller_identity: atm_core::test_support::TEST_SENDER.parse().expect("caller"),
            caller_team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            target_address: None,
            older_than: None,
            idle_only: false,
            dry_run: false,
        });
        assert_eq!(
            request_execution_risk(&request),
            RequestExecutionRisk::SideEffecting
        );
    }

    #[test]
    fn request_execution_risk_classifies_doctor_as_read_only() {
        let tmp = std::env::temp_dir();
        let request = RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tmp.clone(),
            current_dir: tmp,
            team_override: None,
        });
        assert_eq!(
            request_execution_risk(&request),
            RequestExecutionRisk::ReadOnly
        );
    }
}
