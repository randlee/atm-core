use super::*;
#[cfg(unix)]
use std::fs;
use std::time::Instant;

use crate::local_ipc_deadline::{
    DeadlineSupport, ReadFrameDeadlineOutcome, read_frame_with_optional_deadline,
    write_frame_with_optional_deadline,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShutdownResponseDeadlineMode {
    Strict,
    ProbeConnection,
}

pub(super) fn finalize_serve_loop<BeginShutdown, FinalizeShutdown>(
    begin_shutdown: &BeginShutdown,
    finalize_shutdown: &FinalizeShutdown,
    context: ServeShutdownContext<'_>,
    lifecycle_waiter: std::thread::ScopedJoinHandle<'_, ()>,
) -> Option<AtmError>
where
    BeginShutdown: Fn() -> Result<(), AtmError>,
    FinalizeShutdown: Fn(),
{
    let mut shutdown_error = begin_shutdown().err();
    let shutdown_started = Instant::now();
    if let Err(error) = drain_active_connections_for_shutdown(
        context.registry.as_ref(),
        context.force_shutdown,
        context.graceful_drain_deadline,
        context.force_cancel_deadline,
        shutdown_started,
        TRACKED_DISPATCH_JOIN_DEADLINE,
    ) {
        append_shutdown_error(&mut shutdown_error, "drain_error", error);
    }
    let _ = context.lifecycle_control.notify_state_change();
    if let Err(error) = wake_listener(context.endpoint_guard.endpoint_path()) {
        tracing::debug!(%error, "daemon local IPC listener wake was unnecessary during shutdown");
    }
    if lifecycle_waiter.join().is_err() {
        append_shutdown_error(
            &mut shutdown_error,
            "lifecycle_waiter_error",
            AtmError::daemon_lifecycle_wedge(
                "daemon lifecycle waiter panicked during transport shutdown",
            )
            .with_recovery(
                "Restart the daemon; the same-host lifecycle waiter crashed while the runtime was transitioning out of serving state.",
            ),
        );
    }
    if let Err(error) = context.lifecycle_control.shutdown_worker_with_timeout() {
        append_shutdown_error(&mut shutdown_error, "lifecycle_worker_error", error);
    }
    if let Err(error) = context.endpoint_guard.unpublish() {
        append_shutdown_error(&mut shutdown_error, "endpoint_cleanup_error", error);
    }
    finalize_shutdown();
    shutdown_error
}

pub(super) fn finish_serve_shutdown(
    serve_error: Option<AtmError>,
    shutdown_error: Option<AtmError>,
) -> Result<(), AtmError> {
    if let Some(serve_error) = serve_error {
        if let Some(shutdown_error) = shutdown_error {
            tracing::warn!(
                subsystem = "ipc_shutdown",
                action = "finish_serve_shutdown",
                outcome = "serve_and_shutdown_error",
                %shutdown_error,
                %serve_error,
                "daemon shutdown encountered an additional error after a serve error"
            );
        } else {
            tracing::warn!(
                subsystem = "ipc_shutdown",
                action = "finish_serve_shutdown",
                outcome = "serve_error",
                %serve_error,
                "daemon serve loop exited with an error after shutdown finalization"
            );
        }
        return Err(serve_error);
    }
    shutdown_error.map_or(Ok(()), Err)
}

fn append_shutdown_error(shutdown_error: &mut Option<AtmError>, field_name: &str, error: AtmError) {
    if let Some(existing) = shutdown_error.as_ref() {
        tracing::warn!(
            subsystem = "ipc_shutdown",
            action = "append_shutdown_error",
            outcome = "additional_error",
            begin_shutdown_error = %existing,
            error_field = field_name,
            additional_error = %error,
            "daemon shutdown encountered an additional error after an earlier shutdown-start error"
        );
    } else {
        *shutdown_error = Some(error);
    }
}

pub(super) fn record_serve_error(
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
    error: AtmError,
) -> AtmError {
    record_shutdown_signal(lifecycle_control, shutdown_beacon);
    error
}

pub(super) fn record_shutdown_signal(
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
) {
    shutdown_beacon.trip();
    let _ = lifecycle_control.notify_state_change();
}

#[cfg(unix)]
pub(super) fn prepare_local_ipc_endpoint(
    endpoint_path: &Path,
) -> Result<LocalIpcEndpointPreparation, AtmError> {
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon local IPC directory at {}",
                parent.display()
            ))
            .with_recovery(
                "Grant write access to the daemon socket parent directory or choose a writable ATM_HOME before retrying.",
            )
            .with_source(source)
        })?;
    }
    remove_stale_endpoint(endpoint_path)?;
    Ok(LocalIpcEndpointPreparation::FilesystemEndpointPrepared)
}

#[cfg(not(unix))]
pub(super) fn prepare_local_ipc_endpoint(
    _endpoint_path: &Path,
) -> Result<LocalIpcEndpointPreparation, AtmError> {
    Ok(LocalIpcEndpointPreparation::NonFilesystemEndpointPrepared)
}

#[cfg(unix)]
pub(super) fn remove_stale_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if !endpoint_path.exists() {
        return Ok(());
    }
    match fs::remove_file(endpoint_path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AtmError::daemon_unavailable(format!(
            "failed to remove stale daemon local IPC endpoint at {}",
            endpoint_path.display()
        ))
        .with_recovery(
            "Stop the conflicting daemon or remove the stale same-host socket path before restarting atm-daemon.",
        )
        .with_source(source)),
    }
}

pub(super) fn write_shutdown_response(
    mut stream: LocalSocketStream,
    _registry: &Arc<ActiveConnectionRegistry>,
    force_shutdown: &AtomicBool,
    codec: &JsonAtmProtocolCodec,
    deadline_mode: ShutdownResponseDeadlineMode,
) -> Result<ShutdownResponseOutcome, AtmError> {
    let (read_deadline_support, write_deadline_support) =
        shutdown_rejection_deadline_support(&stream, deadline_mode)?;
    let (resumed_stream, frame) = read_shutdown_rejection_frame(stream, read_deadline_support)?;
    stream = resumed_stream;
    let Some(frame) = frame else {
        return Ok(ShutdownResponseOutcome::NoFrame);
    };
    let Ok((request_id, _request)) = codec.request_from_frame(frame) else {
        return Ok(ShutdownResponseOutcome::NoFrame);
    };
    let response = ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
        &AtmError::daemon_unavailable("daemon is shutting down and not accepting new requests")
            .with_recovery("Retry the ATM command after the daemon restarts."),
    ));
    let frame = codec.response_to_frame(request_id, response)?;
    let _ = write_shutdown_rejection_frame(stream, write_deadline_support, force_shutdown, frame)?;
    Ok(ShutdownResponseOutcome::RejectedRequest)
}

fn shutdown_rejection_deadline_support(
    stream: &LocalSocketStream,
    deadline_mode: ShutdownResponseDeadlineMode,
) -> Result<(DeadlineSupport, DeadlineSupport), AtmError> {
    let (read, write) = match deadline_mode {
        ShutdownResponseDeadlineMode::Strict => {
            let read = shutdown_deadline_support(
                stream.set_recv_timeout(Some(REQUEST_DEADLINE)),
                "failed to apply daemon shutdown rejection read deadline",
                "Restart the daemon; the shutdown rejection socket could not apply its bounded read deadline.",
                "daemon shutdown rejection read deadline was unavailable; using helper-thread fallback",
            )?;
            let write = shutdown_deadline_support(
                stream.set_send_timeout(Some(REQUEST_DEADLINE)),
                "failed to apply daemon shutdown rejection write deadline",
                "Restart the daemon; the shutdown rejection socket could not apply its bounded write deadline.",
                "daemon shutdown rejection write deadline was unavailable; using helper-thread fallback",
            )?;
            (read, write)
        }
        ShutdownResponseDeadlineMode::ProbeConnection => {
            // The accept-loop shutdown probe also consumes the daemon's own wake connections,
            // which intentionally carry no request frame and can reach this path after the peer
            // has already dropped its side. Some supported Unix stacks reject timeout setters on
            // that empty probe stream with InvalidInput even though the shared bounded helper is
            // still the correct contract for "read at most one frame / write at most one
            // rejection". Use the helper path directly for probe-only connections so we do not
            // misclassify the daemon's internal accept wake-up as a fatal socket configuration
            // failure.
            (DeadlineSupport::Unsupported, DeadlineSupport::Unsupported)
        }
    };
    Ok((read, write))
}

fn shutdown_deadline_support(
    result: std::io::Result<()>,
    message: &'static str,
    recovery: &'static str,
    debug_message: &'static str,
) -> Result<DeadlineSupport, AtmError> {
    #[cfg(not(windows))]
    let _ = debug_message;
    match result {
        Ok(()) => Ok(DeadlineSupport::Applied),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => {
            let error = AtmError::daemon_unavailable(message)
                .with_recovery(recovery)
                .with_source(source);
            tracing::debug!(%error, "{debug_message}");
            Ok(DeadlineSupport::Unsupported)
        }
        Err(source) => Err(AtmError::daemon_unavailable(message)
            .with_recovery(recovery)
            .with_source(source)),
    }
}

fn read_shutdown_rejection_frame(
    stream: LocalSocketStream,
    read_deadline_support: DeadlineSupport,
) -> Result<(LocalSocketStream, Option<atm_core::protocol::FramePayload>), AtmError> {
    let (stream, outcome) = read_frame_with_optional_deadline(
        stream,
        REQUEST_DEADLINE,
        read_deadline_support,
        None,
        "failed to read daemon request frame during shutdown rejection",
        "daemon request frame exceeded the maximum supported size during shutdown rejection",
    )?;
    match outcome {
        ReadFrameDeadlineOutcome::EndOfStream => Ok((stream, None)),
        ReadFrameDeadlineOutcome::Frame(frame) => Ok((stream, Some(frame))),
        ReadFrameDeadlineOutcome::TimedOut => Err(AtmError::daemon_unavailable(
            "daemon shutdown rejection request read exceeded the runtime deadline",
        )
        .with_recovery("Retry the ATM command after the daemon restarts.")),
    }
}

fn write_shutdown_rejection_frame(
    stream: LocalSocketStream,
    write_deadline_support: DeadlineSupport,
    force_shutdown: &AtomicBool,
    frame: atm_core::protocol::FramePayload,
) -> Result<(LocalSocketStream, ()), AtmError> {
    write_frame_with_optional_deadline(
        stream,
        REQUEST_DEADLINE,
        write_deadline_support,
        Some(force_shutdown),
        &frame,
        (
            "failed to write daemon shutdown rejection response frame",
            "failed to flush daemon shutdown rejection response frame",
        ),
        AtmError::daemon_unavailable(
            "daemon shutdown rejection response write exceeded the runtime deadline",
        )
        .with_recovery(
            "Retry the ATM command after the daemon restarts; the shutdown rejection response could not be delivered cleanly.",
        ),
    )
}

pub(super) fn emit_ready_signal_if_requested() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_READY_STDOUT").is_none() {
        return Ok(());
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "ATM_DAEMON_READY").map_err(|source| {
        AtmError::daemon_unavailable("failed to emit daemon ready signal")
            .with_recovery(
                "Restart the daemon after confirming the parent process still accepts the ready signal on stdout.",
            )
            .with_source(source)
    })?;
    stdout.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon ready signal")
            .with_recovery(
                "Restart the daemon after confirming the parent process still accepts the ready signal on stdout.",
            )
            .with_source(source)
    })?;
    Ok(())
}
