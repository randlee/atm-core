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

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

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

enum ReadRequestFrameOutcome {
    EndOfStream,
    Frame(atm_core::protocol::FramePayload),
    TimedOut,
}

pub(super) fn handle_connection(
    mut stream: LocalSocketStream,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    force_shutdown: &AtomicBool,
    registry: Arc<ActiveConnectionRegistry>,
    codec: JsonAtmProtocolCodec,
) -> Result<(), AtmError> {
    if force_shutdown.load(Ordering::SeqCst) {
        return write_shutdown_response(&mut stream, &codec).map(|_| ());
    }
    let read_deadline_support = configure_request_deadlines(&stream)?;
    let _read_deadline_guard = RequestReadDeadlineGuard::arm(&stream, read_deadline_support)?;

    let frame = match read_request_frame(&mut stream)? {
        ReadRequestFrameOutcome::EndOfStream => return Ok(()),
        ReadRequestFrameOutcome::Frame(frame) => frame,
        ReadRequestFrameOutcome::TimedOut => {
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
    let (request_id, request) = codec.request_from_frame(frame)?;
    if let RequestEnvelope::AdvisoryStream(request) = request {
        return dispatch_advisory_stream(
            &mut stream,
            dispatcher.as_ref(),
            force_shutdown,
            &codec,
            request_id,
            request,
        );
    }

    let response = dispatch_request(request_id, request, dispatcher, &registry)?;
    write_response(&mut stream, &codec, request_id, response)?;
    registry.reap_finished_dispatches()?;
    Ok(())
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

fn read_request_frame(stream: &mut LocalSocketStream) -> Result<ReadRequestFrameOutcome, AtmError> {
    match atm_core::protocol::read_frame(
        stream,
        "failed to read daemon request frame",
        "daemon request frame exceeded the maximum supported size",
    ) {
        Ok(Some(frame)) => Ok(ReadRequestFrameOutcome::Frame(frame)),
        Ok(None) => Ok(ReadRequestFrameOutcome::EndOfStream),
        Err(error) if is_windows_named_pipe_deadline_abort(&error) => {
            Ok(ReadRequestFrameOutcome::TimedOut)
        }
        Err(error) => Err(error),
    }
}

fn dispatch_advisory_stream(
    stream: &mut LocalSocketStream,
    dispatcher: &dyn RequestDispatcher,
    force_shutdown: &AtomicBool,
    codec: &JsonAtmProtocolCodec,
    request_id: RequestId,
    request: atm_core::AdvisoryStreamRequest,
) -> Result<(), AtmError> {
    apply_deadline_contract(
        stream.set_send_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon advisory-stream write deadline",
    )?;
    let mut sink = LocalIpcAdvisoryStreamSink {
        stream,
        codec,
        request_id,
        force_shutdown,
    };
    dispatcher.dispatch_advisory_stream(request, &mut sink)
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

struct RequestReadDeadlineGuard {
    #[cfg(windows)]
    watchdog: Option<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    cancel_tx: Option<std::sync::mpsc::Sender<()>>,
}

impl RequestReadDeadlineGuard {
    fn arm(
        stream: &LocalSocketStream,
        read_deadline_support: DeadlineSupport,
    ) -> Result<Self, AtmError> {
        #[cfg(not(windows))]
        {
            let _ = stream;
            let _ = read_deadline_support;
            Ok(Self {})
        }

        #[cfg(windows)]
        {
            if read_deadline_support != DeadlineSupport::Unsupported {
                return Ok(Self {
                    watchdog: None,
                    cancel_tx: None,
                });
            }

            let handle = stream.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
            let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
            let watchdog = std::thread::Builder::new()
                .name("local-ipc-read-watchdog".to_string())
                .spawn(move || match cancel_rx.recv_timeout(REQUEST_DEADLINE) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if let Err(error) = cancel_windows_named_pipe_io(handle) {
                            tracing::warn!(
                                %error,
                                subsystem = "local_ipc",
                                action = "request_read_watchdog_cancel",
                                deadline_ms = REQUEST_DEADLINE.as_millis() as u64,
                                "failed to cancel a stalled Windows named-pipe request read after the bounded runtime deadline"
                            );
                        }
                    }
                })
                .map_err(|source| {
                    AtmError::daemon_unavailable(
                        "failed to spawn Windows named-pipe request read watchdog",
                    )
                    .with_recovery(
                        "Restart the daemon; the same-host Windows request watchdog could not be created.",
                    )
                    .with_source(source)
                })?;
            Ok(Self {
                watchdog: Some(watchdog),
                cancel_tx: Some(cancel_tx),
            })
        }
    }
}

impl Drop for RequestReadDeadlineGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if let Some(cancel_tx) = self.cancel_tx.take() {
                let _ = cancel_tx.send(());
            }
            if let Some(watchdog) = self.watchdog.take()
                && watchdog.join().is_err()
            {
                tracing::warn!(
                    subsystem = "local_ipc",
                    action = "request_read_watchdog_join",
                    "Windows named-pipe request read watchdog panicked during teardown"
                );
            }
        }
    }
}

#[cfg(windows)]
fn cancel_windows_named_pipe_io(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> std::io::Result<()> {
    use std::ptr;

    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::System::IO::CancelIoEx;

    let cancelled = unsafe { CancelIoEx(handle, ptr::null_mut()) };
    if cancelled != 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
        return Ok(());
    }
    Err(error)
}

#[cfg(not(windows))]
fn is_windows_named_pipe_deadline_abort(_error: &AtmError) -> bool {
    false
}

#[cfg(windows)]
fn is_windows_named_pipe_deadline_abort(error: &AtmError) -> bool {
    use windows_sys::Win32::Foundation::ERROR_OPERATION_ABORTED;

    error
        .source
        .as_ref()
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .is_some_and(|source| {
            source.kind() == std::io::ErrorKind::Interrupted
                || source.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32)
        })
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
    codec: &'a JsonAtmProtocolCodec,
    request_id: RequestId,
    force_shutdown: &'a AtomicBool,
}

impl AdvisoryStreamSink for LocalIpcAdvisoryStreamSink<'_> {
    fn emit(&mut self, response: ResponseEnvelope) -> Result<(), AtmError> {
        let frame = self.codec.response_to_frame(self.request_id, response)?;
        atm_core::protocol::write_frame(
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
