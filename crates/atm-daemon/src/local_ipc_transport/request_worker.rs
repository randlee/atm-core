use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atm_core::boundary::{AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{ConnectionFailureClassification, DaemonConnectionFailureFields};
use atm_core::protocol::{
    FramePayload, JsonAtmProtocolCodec, RequestEnvelope, RequestId, ResponseEnvelope,
};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream;

use crate::SubsystemObservability;
use crate::active_connection_registry::{
    ActiveConnectionRegistry, DispatchReapSummary, TrackedDispatchHandle,
};

#[cfg(test)]
use super::PreparedRuntimeServer;
use super::{
    DISPATCH_PANIC_RECOVERED_MESSAGE, MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE,
    write_shutdown_response,
};

const SAME_HOST_HEADER_READ_DEADLINE: std::time::Duration = std::time::Duration::from_secs(1);
const SAME_HOST_PAYLOAD_READ_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);
const SAME_HOST_RESPONSE_WRITE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);

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
        frame: atm_core::protocol::FramePayload,
    },
    #[cfg(windows)]
    TimedOut,
}

pub(super) fn handle_connection(
    mut stream: LocalSocketStream,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    force_shutdown: &AtomicBool,
    registry: Arc<ActiveConnectionRegistry>,
    codec: JsonAtmProtocolCodec,
    observability: &SubsystemObservability,
) -> Result<(), AtmError> {
    if force_shutdown.load(Ordering::SeqCst) {
        return write_shutdown_response(&mut stream, &codec).map(|_| ());
    }
    let frame = match read_connection_frame(stream, force_shutdown, observability)? {
        ReadRequestFrameResult::EndOfStream => return Ok(()),
        ReadRequestFrameResult::Frame {
            stream: resumed_stream,
            frame,
        } => {
            stream = resumed_stream;
            frame
        }
        #[cfg(windows)]
        ReadRequestFrameResult::TimedOut => return Ok(()),
    };
    tracing::debug!(
        max_daemon_frame_bytes = atm_core::protocol::MAX_DAEMON_FRAME_BYTES,
        "daemon request frame accepted under configured size cap"
    );
    let (request_id, request) = decode_request_frame(&codec, frame, observability)?;
    let response = dispatch_request(request_id, request, dispatcher, &registry, observability)?;
    if let Err(error) = write_response(&mut stream, &codec, request_id, response) {
        let classification = classify_connection_failure(&error);
        if classification == ConnectionFailureClassification::ExpectedPeerDisconnect {
            observability.emit_event_or_warn(
                observability
                    .event(
                        "connection_worker",
                        classification.as_str(),
                        "same-host peer disconnected before the daemon response frame completed",
                    )
                    .with_connection_failure(DaemonConnectionFailureFields {
                        code: error.code,
                        request_id: Some(request_id),
                        classification,
                    })
                    .with_transport_context("response_write"),
            );
            return Ok(());
        }
        emit_connection_failure_event(observability, &error, Some(request_id), "response_write");
        return Err(error);
    }
    emit_dispatch_panic_recovery(observability, registry.reap_finished_dispatches()?);
    Ok(())
}

fn emit_dispatch_panic_recovery(
    observability: &SubsystemObservability,
    summary: DispatchReapSummary,
) {
    if summary.recovered_panics == 0 {
        return;
    }
    observability.emit_or_warn(
        "dispatch_worker",
        "panic_recovered",
        DISPATCH_PANIC_RECOVERED_MESSAGE,
    );
}

fn read_connection_frame(
    stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    observability: &SubsystemObservability,
) -> Result<ReadRequestFrameResult, AtmError> {
    let read_deadline_support = configure_request_deadlines(&stream).inspect_err(|error| {
        emit_connection_failure_event(observability, error, None, "request_deadline_config");
    })?;
    match read_request_frame_with_deadline(stream, force_shutdown, read_deadline_support)
        .inspect_err(|error| {
            emit_connection_failure_event(observability, error, None, "request_read");
        })? {
        ReadRequestFrameResult::EndOfStream => Ok(ReadRequestFrameResult::EndOfStream),
        ReadRequestFrameResult::Frame { stream, frame } => {
            Ok(ReadRequestFrameResult::Frame { stream, frame })
        }
        #[cfg(windows)]
        ReadRequestFrameResult::TimedOut if force_shutdown.load(Ordering::SeqCst) => {
            tracing::info!(
                subsystem = "local_ipc",
                action = "request_read",
                outcome = "forced_shutdown",
                "daemon forced shutdown interrupted a Windows same-host request read before a complete frame arrived"
            );
            Ok(ReadRequestFrameResult::TimedOut)
        }
        #[cfg(windows)]
        ReadRequestFrameResult::TimedOut => {
            observability.emit_event_or_warn(
                observability
                    .event(
                        "connection_worker",
                        ConnectionFailureClassification::TransportFailure.as_str(),
                        "daemon local IPC request read exceeded the runtime deadline; closing the stalled connection",
                    )
                    .with_connection_failure(DaemonConnectionFailureFields {
                        code: AtmErrorCode::DaemonUnavailable,
                        request_id: None,
                        classification: ConnectionFailureClassification::TransportFailure,
                    })
                    .with_transport_context("request_read"),
            );
            tracing::warn!(
                subsystem = "local_ipc",
                action = "request_read",
                outcome = "deadline_exceeded",
                deadline_ms = REQUEST_DEADLINE.as_millis() as u64,
                "daemon local IPC request read exceeded the runtime deadline; closing the stalled connection"
            );
            Ok(ReadRequestFrameResult::TimedOut)
        }
    }
}

fn decode_request_frame(
    codec: &JsonAtmProtocolCodec,
    frame: FramePayload,
    observability: &SubsystemObservability,
) -> Result<(RequestId, RequestEnvelope), AtmError> {
    let request_id = frame.request_id;
    codec.request_from_frame(frame).inspect_err(|error| {
        emit_connection_failure_event(observability, error, Some(request_id), "request_decode");
    })
}

pub(super) fn classify_connection_failure(error: &AtmError) -> ConnectionFailureClassification {
    if error.code == AtmErrorCode::MessageValidationFailed {
        return ConnectionFailureClassification::MalformedRequest;
    }
    let haystacks = [error.message.to_ascii_lowercase()];
    if haystacks.iter().any(|value| {
        value.contains("broken pipe")
            || value.contains("connection reset")
            || value.contains("connection aborted")
            || value.contains("unexpected eof")
            || value.contains("end of file")
    }) {
        return ConnectionFailureClassification::ExpectedPeerDisconnect;
    }
    if matches!(
        error.code,
        AtmErrorCode::DaemonUnavailable | AtmErrorCode::WaitTimeout
    ) {
        return ConnectionFailureClassification::TransportFailure;
    }
    ConnectionFailureClassification::RequestFailure
}

pub(super) fn emit_connection_failure_event(
    observability: &SubsystemObservability,
    error: &AtmError,
    request_id: Option<RequestId>,
    transport_context: &'static str,
) {
    let classification = classify_connection_failure(error);
    observability.emit_event_or_warn(
        observability
            .event(
                "connection_worker",
                classification.as_str(),
                error.message.clone(),
            )
            .with_connection_failure(DaemonConnectionFailureFields {
                code: error.code,
                request_id,
                classification,
            })
            .with_transport_context(transport_context),
    );
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
                ));
            }
        }
    }
}

fn read_request_frame(
    stream: &mut LocalSocketStream,
) -> Result<Option<atm_core::protocol::FramePayload>, AtmError> {
    let Some(header_bytes) =
        atm_core::protocol::read_frame_header(stream, "failed to read daemon request frame")?
    else {
        return Ok(None);
    };
    let header = atm_core::protocol::decode_frame_header(
        header_bytes,
        "daemon request frame exceeded the maximum supported size",
    )?;
    apply_optional_deadline(
        stream.set_recv_timeout(Some(SAME_HOST_PAYLOAD_READ_DEADLINE)),
        "failed to apply daemon request payload deadline",
    )?;
    Ok(Some(atm_core::protocol::read_frame_payload(
        stream,
        header,
        "failed to read daemon request frame",
    )?))
}

fn configure_request_deadlines(stream: &LocalSocketStream) -> Result<DeadlineSupport, AtmError> {
    let read_deadline_support = apply_optional_deadline(
        stream.set_recv_timeout(Some(SAME_HOST_HEADER_READ_DEADLINE)),
        "failed to apply daemon request header deadline",
    )?;
    apply_optional_deadline(
        stream.set_send_timeout(Some(SAME_HOST_RESPONSE_WRITE_DEADLINE)),
        "failed to apply daemon response write deadline",
    )?;
    Ok(read_deadline_support)
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
        Err(_source) => Err(AtmError::daemon_unavailable(message)),
    }
}

fn dispatch_request(
    request_id: RequestId,
    request: RequestEnvelope,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
    observability: &SubsystemObservability,
) -> Result<ResponseEnvelope, AtmError> {
    let execution_risk = request_execution_risk(&request);
    let result_rx = (|| {
        let (result_rx, completion_rx, dispatch_handle) =
            spawn_dispatch_worker(request, dispatcher, Arc::clone(registry))?;
        registry.push_dispatch_handle(
            TrackedDispatchHandle {
                completion_rx,
                join_handle: dispatch_handle,
            },
            MAX_CONCURRENT_CONNECTIONS,
        )?;
        Ok::<DispatchResultRx, AtmError>(result_rx)
    })()
    .inspect_err(|error| {
        emit_connection_failure_event(observability, error, Some(request_id), "dispatch_request");
    })?;
    Ok(await_dispatch_response(
        request_id,
        execution_risk,
        result_rx,
    ))
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
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn daemon local IPC dispatch worker")
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
        Ok(Err(error)) => ResponseEnvelope::Error(error),
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
            ResponseEnvelope::Error(AtmError::daemon_unavailable(
                "daemon request dispatcher stopped before returning a response",
            ))
        }
    }
}

fn request_execution_risk(request: &RequestEnvelope) -> RequestExecutionRisk {
    match request {
        RequestEnvelope::List(_)
        | RequestEnvelope::Peek(_)
        | RequestEnvelope::Receive(_)
        | RequestEnvelope::Doctor(_) => RequestExecutionRisk::ReadOnly,
        RequestEnvelope::CompatibilityPreflight(_) => RequestExecutionRisk::ReadOnly,
        RequestEnvelope::Send(_) | RequestEnvelope::Heartbeat(_) | RequestEnvelope::Clear(_) => {
            RequestExecutionRisk::SideEffecting
        }
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
    ResponseEnvelope::Error(error)
}

fn write_response(
    stream: &mut LocalSocketStream,
    codec: &JsonAtmProtocolCodec,
    request_id: RequestId,
    response: ResponseEnvelope,
) -> Result<(), AtmError> {
    let frame = codec.response_to_frame(request_id, response)?;
    atm_core::protocol::write_frame(stream, &frame, "failed to write daemon response frame")?;
    std::io::Write::flush(stream)
        .map_err(|_source| AtmError::daemon_unavailable("failed to flush daemon response frame"))
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn install_injected_accept_error_for_test(
    runtime: &mut PreparedRuntimeServer,
    signal: std::sync::mpsc::SyncSender<()>,
) {
    runtime.accept_error_inject = Some(signal);
}

#[cfg(test)]
mod tests {
    use super::{
        RequestExecutionRisk, classify_connection_failure, dispatch_timeout_response,
        request_execution_risk,
    };
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::observability::ConnectionFailureClassification;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};

    #[test]
    fn side_effecting_timeout_returns_may_have_executed_code() {
        let response = dispatch_timeout_response(RequestExecutionRisk::SideEffecting);
        let ResponseEnvelope::Error(AtmError { code, .. }) = response else {
            panic!("expected error envelope");
        };
        assert_eq!(code, AtmErrorCode::DaemonMayHaveExecuted);
    }

    #[test]
    fn read_only_timeout_returns_retryable_daemon_unavailable_code() {
        let response = dispatch_timeout_response(RequestExecutionRisk::ReadOnly);
        let ResponseEnvelope::Error(AtmError { code, .. }) = response else {
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
            ..DoctorQuery::default()
        });
        assert_eq!(
            request_execution_risk(&request),
            RequestExecutionRisk::ReadOnly
        );
    }

    #[test]
    fn classify_connection_failure_marks_validation_errors_as_malformed_requests() {
        let error = AtmError::validation("bad frame");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::MalformedRequest
        );
    }

    #[test]
    fn classify_connection_failure_marks_disconnect_strings_as_expected_peer_disconnects() {
        let error = AtmError::daemon_unavailable("Broken pipe while writing response");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::ExpectedPeerDisconnect
        );
    }

    #[test]
    fn classify_connection_failure_marks_daemon_unavailable_as_transport_failure() {
        let error = AtmError::daemon_unavailable("socket timed out");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::TransportFailure
        );
    }

    #[test]
    fn classify_connection_failure_marks_other_errors_as_request_failure() {
        let error = AtmError::new(AtmErrorCode::MailboxReadFailed, "mailbox lookup failed");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::RequestFailure
        );
    }
}
