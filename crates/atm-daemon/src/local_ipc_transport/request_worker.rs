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
    DeadlineSupport, LocalIpcDeadlineSupport, apply_optional_deadline,
    write_frame_with_optional_deadline,
};
#[cfg(windows)]
use crate::local_ipc_deadline::{ReadFrameDeadlineOutcome, read_frame_with_optional_deadline};

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

enum ReadRequestFrameResult {
    EndOfStream,
    Frame {
        stream: LocalSocketStream,
        frame: atm_core::protocol::FramePayload,
    },
    #[cfg(windows)]
    TimedOut,
}

struct AdvisoryStreamSinkContext<'a> {
    codec: &'a JsonAtmProtocolCodec,
    request_id: RequestId,
    force_shutdown: &'a AtomicBool,
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
    stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    _registry: &Arc<ActiveConnectionRegistry>,
) -> Result<ReadRequestFrameResult, AtmError> {
    let (stream, outcome) = read_frame_with_optional_deadline(
        stream,
        REQUEST_DEADLINE,
        DeadlineSupport::Unsupported,
        Some(force_shutdown),
        "failed to read daemon request frame",
        "daemon request frame exceeded the maximum supported size",
    )?;
    match outcome {
        ReadFrameDeadlineOutcome::EndOfStream => Ok(ReadRequestFrameResult::EndOfStream),
        ReadFrameDeadlineOutcome::Frame(frame) => {
            Ok(ReadRequestFrameResult::Frame { stream, frame })
        }
        ReadFrameDeadlineOutcome::TimedOut => Ok(ReadRequestFrameResult::TimedOut),
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
    _registry: &Arc<ActiveConnectionRegistry>,
    write_deadline_support: DeadlineSupport,
    request_id: RequestId,
    response: ResponseEnvelope,
) -> Result<(), AtmError> {
    let frame = codec.response_to_frame(request_id, response)?;
    let _ = write_frame_with_optional_deadline(
        stream,
        REQUEST_DEADLINE,
        write_deadline_support,
        &frame,
        "failed to write daemon response frame",
        "failed to flush daemon response frame",
        response_write_timeout_error(),
    )?;
    Ok(())
}

fn response_write_timeout_error() -> AtmError {
    AtmError::daemon_unavailable(
        "daemon response write exceeded the runtime deadline; closing the stalled same-host connection",
    )
    .with_recovery(
        "Retry the ATM command after the daemon finishes recovering the same-host request runtime.",
    )
}

struct LocalIpcAdvisoryStreamSink<'a> {
    stream: Option<LocalSocketStream>,
    codec: &'a JsonAtmProtocolCodec,
    request_id: RequestId,
    force_shutdown: &'a AtomicBool,
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
        let (stream, ()) = write_frame_with_optional_deadline(
            stream,
            REQUEST_DEADLINE,
            self.write_deadline_support,
            &frame,
            "failed to write daemon advisory-stream response frame",
            "failed to flush daemon advisory-stream response frame",
            AtmError::daemon_unavailable(
                "daemon advisory-stream response write exceeded the runtime deadline",
            )
            .with_recovery(
                "Retry graft activation after atm-daemon returns to a healthy serving state.",
            ),
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
