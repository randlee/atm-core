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

#[cfg(test)]
use super::PreparedRuntimeServer;
use super::{MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE, write_shutdown_response};

type DispatchResultRx = std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>;
type DispatchCompletionRx = std::sync::mpsc::Receiver<()>;
type DispatchWorkerHandle = std::thread::JoinHandle<()>;
type DispatchWorker = (DispatchResultRx, DispatchCompletionRx, DispatchWorkerHandle);

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
    configure_request_deadlines(&stream)?;

    let Some(frame) = read_request_frame(&mut stream)? else {
        return Ok(());
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

fn configure_request_deadlines(stream: &LocalSocketStream) -> Result<(), AtmError> {
    stream
        .set_recv_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon request read deadline")
                .with_recovery(
                    "Restart the daemon; the same-host request socket could not apply its bounded read deadline.",
                )
                .with_source(source)
        })?;
    stream
        .set_send_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon response write deadline")
                .with_recovery(
                    "Restart the daemon; the same-host request socket could not apply its bounded write deadline.",
                )
                .with_source(source)
        })
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

fn dispatch_advisory_stream(
    stream: &mut LocalSocketStream,
    dispatcher: &dyn RequestDispatcher,
    force_shutdown: &AtomicBool,
    codec: &JsonAtmProtocolCodec,
    request_id: RequestId,
    request: atm_core::AdvisoryStreamRequest,
) -> Result<(), AtmError> {
    stream.set_send_timeout(Some(REQUEST_DEADLINE)).map_err(|source| {
        AtmError::daemon_unavailable("failed to apply daemon advisory-stream write deadline")
            .with_recovery(
                "Restart the daemon; the same-host advisory stream socket could not apply its bounded write deadline.",
            )
            .with_source(source)
    })?;
    let mut sink = LocalIpcAdvisoryStreamSink {
        stream,
        codec,
        request_id,
        force_shutdown,
    };
    dispatcher.dispatch_advisory_stream(request, &mut sink)
}

fn dispatch_request(
    request_id: RequestId,
    request: RequestEnvelope,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
) -> Result<ResponseEnvelope, AtmError> {
    let (result_rx, completion_rx, dispatch_handle) =
        spawn_dispatch_worker(request, dispatcher, Arc::clone(registry))?;
    registry.push_dispatch_handle(
        TrackedDispatchHandle {
            completion_rx,
            join_handle: dispatch_handle,
        },
        MAX_CONCURRENT_CONNECTIONS,
    )?;
    Ok(await_dispatch_response(request_id, result_rx))
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
            ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable(
                    "daemon request exceeded the 3s runtime deadline; the operation may still complete in the background",
                )
                .with_recovery(
                    "Check the destination mailbox or service-side effects before retrying this ATM command.",
                ),
            ))
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
