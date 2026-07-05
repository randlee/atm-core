use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atm_core::boundary::{AdvisoryStreamSink, AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::protocol::{
    JsonAtmProtocolCodec, ProtocolErrorEnvelope, RequestEnvelope, RequestId, ResponseEnvelope,
};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream;

use crate::active_connection_registry::{ActiveConnectionRegistry, TrackedDispatchHandle};
use crate::local_ipc_deadline::{
    DeadlineSupport, LocalIpcDeadlineSupport, OwnedLocalIpcDeadlineConfig, apply_optional_deadline,
    run_owned_local_ipc_with_deadline,
};

#[cfg(test)]
use super::PreparedRuntimeServer;
use super::{
    MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE, ShutdownResponseDeadlineMode,
    write_shutdown_response,
};

type DispatchResultRx = std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>;
type DispatchCompletionRx = std::sync::mpsc::Receiver<()>;
type DispatchWorkerHandle = std::thread::JoinHandle<()>;
type DispatchWorker = (DispatchResultRx, DispatchCompletionRx, DispatchWorkerHandle);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestExecutionRisk {
    ReadOnly,
    SideEffecting,
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

#[cfg(windows)]
enum NonblockingReadRequestFrameResult {
    EndOfStream,
    Frame(atm_core::protocol::FramePayload),
    TimedOut,
}

struct AdvisoryStreamSinkContext<'a> {
    codec: &'a JsonAtmProtocolCodec,
    request_id: RequestId,
    force_shutdown: &'a AtomicBool,
    registry: &'a Arc<ActiveConnectionRegistry>,
    write_deadline_support: DeadlineSupport,
}

pub(super) fn handle_connection(
    mut stream: LocalSocketStream,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    force_shutdown: &AtomicBool,
    registry: Arc<ActiveConnectionRegistry>,
    codec: JsonAtmProtocolCodec,
) -> Result<(), AtmError> {
    if force_shutdown.load(Ordering::SeqCst) {
        return write_shutdown_response(
            stream,
            &registry,
            &codec,
            ShutdownResponseDeadlineMode::Strict,
        )
        .map(|_| ());
    }
    let deadline_support = configure_request_deadlines(&stream)?;

    let Some((resumed_stream, frame)) =
        read_request_frame_or_terminate(stream, force_shutdown, &registry, deadline_support.read)?
    else {
        return Ok(());
    };
    stream = resumed_stream;
    tracing::debug!(
        max_daemon_frame_bytes = atm_core::protocol::MAX_DAEMON_FRAME_BYTES,
        "daemon request frame accepted under configured size cap"
    );
    let (request_id, request) = codec.request_from_frame(frame)?;
    if let RequestEnvelope::AdvisoryStream(request) = request {
        return handle_advisory_stream_request(
            stream,
            request,
            dispatcher.as_ref(),
            AdvisoryStreamSinkContext {
                codec: &codec,
                request_id,
                force_shutdown,
                registry: &registry,
                write_deadline_support: deadline_support.write,
            },
        );
    }

    let response = dispatch_request(request_id, request, dispatcher, &registry)?;
    write_response(
        stream,
        &codec,
        &registry,
        deadline_support.write,
        request_id,
        response,
    )?;
    registry.reap_finished_dispatches()?;
    Ok(())
}

fn read_request_frame_or_terminate(
    stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    registry: &Arc<ActiveConnectionRegistry>,
    read_deadline_support: DeadlineSupport,
) -> Result<Option<(LocalSocketStream, atm_core::protocol::FramePayload)>, AtmError> {
    match read_request_frame_with_deadline(stream, force_shutdown, registry, read_deadline_support)?
    {
        ReadRequestFrameResult::EndOfStream => Ok(None),
        ReadRequestFrameResult::Frame {
            stream: resumed_stream,
            frame,
        } => Ok(Some((resumed_stream, frame))),
        #[cfg(windows)]
        ReadRequestFrameResult::TimedOut if force_shutdown.load(Ordering::SeqCst) => {
            tracing::info!(
                subsystem = "local_ipc",
                action = "request_read",
                outcome = "forced_shutdown",
                "daemon forced shutdown interrupted a Windows same-host request read before a complete frame arrived"
            );
            Ok(None)
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
            Ok(None)
        }
    }
}

fn handle_advisory_stream_request(
    stream: LocalSocketStream,
    request: atm_core::AdvisoryStreamRequest,
    dispatcher: &dyn RequestDispatcher,
    sink_context: AdvisoryStreamSinkContext<'_>,
) -> Result<(), AtmError> {
    let mut sink = LocalIpcAdvisoryStreamSink {
        stream: Some(stream),
        codec: sink_context.codec,
        request_id: sink_context.request_id,
        force_shutdown: sink_context.force_shutdown,
        registry: sink_context.registry,
        write_deadline_support: sink_context.write_deadline_support,
    };
    dispatcher.dispatch_advisory_stream(request, &mut sink)
}

fn read_request_frame_with_deadline(
    mut stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    registry: &Arc<ActiveConnectionRegistry>,
    read_deadline_support: DeadlineSupport,
) -> Result<ReadRequestFrameResult, AtmError> {
    #[cfg(windows)]
    if read_deadline_support == DeadlineSupport::Unsupported {
        return read_request_frame_with_helper(stream, force_shutdown, registry);
    }
    #[cfg(not(windows))]
    let _ = (force_shutdown, registry, read_deadline_support);

    match read_request_frame(&mut stream)? {
        None => Ok(ReadRequestFrameResult::EndOfStream),
        Some(frame) => Ok(ReadRequestFrameResult::Frame { stream, frame }),
    }
}

#[cfg(windows)]
fn read_request_frame_with_helper(
    mut stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    _registry: &Arc<ActiveConnectionRegistry>,
) -> Result<ReadRequestFrameResult, AtmError> {
    stream.set_nonblocking(true).map_err(|source| {
        AtmError::daemon_unavailable(
            "failed to place the Windows same-host request pipe into nonblocking mode",
        )
        .with_recovery(
            "Restart the daemon; the same-host Windows request pipe could not enter the bounded nonblocking read path.",
        )
        .with_source(source)
    })?;

    let result = poll_request_frame_nonblocking(&mut stream, force_shutdown);
    match result {
        Ok(NonblockingReadRequestFrameResult::Frame(frame)) => {
            stream.set_nonblocking(false).map_err(|source| {
                AtmError::daemon_unavailable(
                    "failed to restore blocking mode on the Windows same-host request pipe",
                )
                .with_recovery(
                    "Restart the daemon; the same-host Windows request pipe could not return to blocking mode after a bounded read.",
                )
                .with_source(source)
            })?;
            Ok(ReadRequestFrameResult::Frame { stream, frame })
        }
        Ok(NonblockingReadRequestFrameResult::EndOfStream) => {
            Ok(ReadRequestFrameResult::EndOfStream)
        }
        Ok(NonblockingReadRequestFrameResult::TimedOut) => Ok(ReadRequestFrameResult::TimedOut),
        Err(error) => Err(error),
    }
}

fn read_request_frame(
    stream: &mut LocalSocketStream,
) -> Result<Option<atm_core::protocol::FramePayload>, AtmError> {
    atm_core::protocol::read_frame(
        stream,
        "failed to read daemon request frame",
        "daemon request frame exceeded the maximum supported size",
    )
}

#[cfg(windows)]
fn poll_request_frame_nonblocking(
    stream: &mut LocalSocketStream,
    force_shutdown: &AtomicBool,
) -> Result<NonblockingReadRequestFrameResult, AtmError> {
    let started = std::time::Instant::now();
    let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
    let header_read = poll_request_bytes(stream, force_shutdown, started, &mut header)?;
    if header_read == 0 {
        return Ok(NonblockingReadRequestFrameResult::EndOfStream);
    }
    let (request_id, message_kind, flags, payload_length) = decode_polled_frame_header(header)?;
    let mut payload = vec![0u8; payload_length];
    let payload_read = poll_request_bytes(stream, force_shutdown, started, &mut payload)?;
    if payload_read != payload.len() {
        return Ok(NonblockingReadRequestFrameResult::TimedOut);
    }
    Ok(NonblockingReadRequestFrameResult::Frame(
        atm_core::protocol::FramePayload {
            request_id,
            message_kind,
            flags,
            bytes: payload,
        },
    ))
}

#[cfg(windows)]
fn poll_request_bytes(
    stream: &mut LocalSocketStream,
    force_shutdown: &AtomicBool,
    started: std::time::Instant,
    buffer: &mut [u8],
) -> Result<usize, AtmError> {
    let mut offset = 0usize;
    while offset < buffer.len() {
        match std::io::Read::read(stream, &mut buffer[offset..]) {
            Ok(0) if offset == 0 => return Ok(0),
            Ok(0) => {
                return Err(daemon_request_read_error(std::io::Error::from(
                    std::io::ErrorKind::UnexpectedEof,
                )));
            }
            Ok(read) => {
                offset += read;
            }
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                if force_shutdown.load(Ordering::SeqCst) || started.elapsed() >= REQUEST_DEADLINE {
                    return Ok(offset);
                }
                let remaining = REQUEST_DEADLINE.saturating_sub(started.elapsed());
                std::thread::sleep(std::cmp::min(remaining, WINDOWS_READ_HELPER_POLL_INTERVAL));
            }
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(source) => return Err(daemon_request_read_error(source)),
        }
    }
    Ok(offset)
}

#[cfg(windows)]
fn decode_polled_frame_header(
    header: [u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES],
) -> Result<(RequestId, atm_core::protocol::MessageKind, u16, usize), AtmError> {
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
            "Align the CLI and daemon builds so both sides use the same ATM daemon protocol version before retrying.",
        ));
    }

    let message_kind = atm_core::protocol::MessageKind::try_from(u16::from_be_bytes(
        header[6..8].try_into().expect("kind"),
    ))?;
    let flags = u16::from_be_bytes(header[8..10].try_into().expect("flags"));
    if flags != atm_core::protocol::ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame flags 0x{flags:04x} for version {version}"
        ))
        .with_recovery(
            "Retry with a supported ATM daemon client/server build that uses the version-1 flag contract.",
        ));
    }

    let request_id = RequestId::new(u64::from_be_bytes(
        header[10..18].try_into().expect("request id"),
    ))?;
    let payload_length = u32::from_be_bytes(header[18..22].try_into().expect("payload")) as usize;
    if payload_length > atm_core::protocol::MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "daemon request frame exceeded the maximum supported size",
        )
        .with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }
    Ok((request_id, message_kind, flags, payload_length))
}

#[cfg(windows)]
fn daemon_request_read_error(source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to read daemon request frame").with_source(source)
}

fn configure_request_deadlines(
    stream: &LocalSocketStream,
) -> Result<LocalIpcDeadlineSupport, AtmError> {
    let read = apply_optional_deadline(
        stream.set_recv_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon request read deadline",
        "Restart the daemon; the same-host request socket could not apply its bounded read deadline.",
    )?;
    let write = apply_optional_deadline(
        stream.set_send_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon response write deadline",
        "Restart the daemon; the same-host request socket could not apply its bounded write deadline.",
    )?;
    Ok(LocalIpcDeadlineSupport { read, write })
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
        RequestEnvelope::List(_)
        | RequestEnvelope::Receive(_)
        | RequestEnvelope::Doctor(_)
        | RequestEnvelope::AdvisoryFetch(_) => RequestExecutionRisk::ReadOnly,
        RequestEnvelope::Send(_)
        | RequestEnvelope::Heartbeat(_)
        | RequestEnvelope::Clear(_)
        | RequestEnvelope::AdvisoryRegister(_)
        | RequestEnvelope::AdvisoryUnregister(_)
        | RequestEnvelope::AdvisoryDrain(_)
        | RequestEnvelope::AdvisoryStream(_) => RequestExecutionRisk::SideEffecting,
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

fn write_response(
    stream: LocalSocketStream,
    codec: &JsonAtmProtocolCodec,
    registry: &Arc<ActiveConnectionRegistry>,
    write_deadline_support: DeadlineSupport,
    request_id: RequestId,
    response: ResponseEnvelope,
) -> Result<(), AtmError> {
    let frame = codec.response_to_frame(request_id, response)?;
    let _ = run_owned_local_ipc_with_deadline(
        stream,
        response_write_deadline_config(Arc::clone(registry), write_deadline_support),
        move |stream| write_response_frame(stream, &frame),
    )?;
    Ok(())
}

fn response_write_deadline_config(
    background_work_registry: Arc<ActiveConnectionRegistry>,
    write_deadline_support: DeadlineSupport,
) -> OwnedLocalIpcDeadlineConfig {
    OwnedLocalIpcDeadlineConfig {
        deadline: REQUEST_DEADLINE,
        support: write_deadline_support,
        worker_name: "local-ipc-response-write-helper",
        timeout_error: AtmError::daemon_unavailable(
            "daemon response write exceeded the runtime deadline; closing the stalled same-host connection",
        )
        .with_recovery(
            "Retry the ATM command after the daemon finishes recovering the same-host request runtime.",
        ),
        disconnect_error: AtmError::daemon_unavailable(
            "daemon response write helper disconnected before returning a result",
        )
        .with_recovery(
            "Retry the ATM command after the daemon finishes recovering the same-host request runtime.",
        ),
        spawn_error_message: "failed to spawn daemon response write helper",
        spawn_error_recovery:
            "Restart the daemon; the same-host response write helper could not be created.",
        background_work_registry: Some(background_work_registry),
    }
}

fn write_response_frame(
    stream: &mut LocalSocketStream,
    frame: &atm_core::protocol::FramePayload,
) -> Result<(), AtmError> {
    atm_core::protocol::write_frame(stream, frame, "failed to write daemon response frame")?;
    std::io::Write::flush(stream).map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon response frame")
            .with_recovery(
                "Retry the ATM command after the daemon finishes recovering the same-host request runtime.",
            )
            .with_source(source)
    })
}

struct LocalIpcAdvisoryStreamSink<'a> {
    stream: Option<LocalSocketStream>,
    codec: &'a JsonAtmProtocolCodec,
    request_id: RequestId,
    force_shutdown: &'a AtomicBool,
    registry: &'a Arc<ActiveConnectionRegistry>,
    write_deadline_support: DeadlineSupport,
}

impl AdvisoryStreamSink for LocalIpcAdvisoryStreamSink<'_> {
    fn emit(&mut self, response: ResponseEnvelope) -> Result<(), AtmError> {
        let stream = self.stream.take().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon advisory-stream sink lost its local IPC connection state",
            )
            .with_recovery(
                "Retry graft activation after atm-daemon returns to a healthy serving state.",
            )
        })?;
        let frame = self.codec.response_to_frame(self.request_id, response)?;
        let (stream, ()) = run_owned_local_ipc_with_deadline(
            stream,
            OwnedLocalIpcDeadlineConfig {
                deadline: REQUEST_DEADLINE,
                support: self.write_deadline_support,
                worker_name: "local-ipc-advisory-write-helper",
                timeout_error: AtmError::daemon_unavailable(
                    "daemon advisory-stream response write exceeded the runtime deadline",
                )
                .with_recovery(
                    "Retry graft activation after atm-daemon returns to a healthy serving state.",
                ),
                disconnect_error: AtmError::daemon_unavailable(
                    "daemon advisory-stream write helper disconnected before returning a result",
                )
                .with_recovery(
                    "Retry graft activation after atm-daemon returns to a healthy serving state.",
                ),
                spawn_error_message: "failed to spawn daemon advisory-stream write helper",
                spawn_error_recovery: "Restart the daemon; the same-host advisory-stream write helper could not be created.",
                background_work_registry: Some(Arc::clone(self.registry)),
            },
            move |stream| {
                atm_core::protocol::write_frame(
                    stream,
                    &frame,
                    "failed to write daemon advisory-stream response frame",
                )?;
                std::io::Write::flush(stream).map_err(|source| {
                    AtmError::daemon_unavailable(
                        "failed to flush daemon advisory-stream response frame",
                    )
                    .with_recovery(
                        "Retry graft activation after atm-daemon returns to a healthy serving state.",
                    )
                    .with_source(source)
                })
            },
        )?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop_requested(&self) -> bool {
        self.force_shutdown.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(super) fn install_injected_accept_error_for_test(
    runtime: &mut PreparedRuntimeServer,
    signal: std::sync::mpsc::SyncSender<()>,
) {
    runtime.accept_error_inject = Some(signal);
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
            actor_override: None,
            target_address: None,
            team_override: None,
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
