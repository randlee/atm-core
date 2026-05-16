use super::*;
#[cfg(unix)]
use std::fs;
use std::time::Instant;

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
                %shutdown_error,
                %serve_error,
                "daemon shutdown encountered an additional error after a serve error"
            );
        } else {
            tracing::warn!(
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
    stream: &mut LocalSocketStream,
    codec: &JsonAtmProtocolCodec,
) -> Result<ShutdownResponseOutcome, AtmError> {
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
    let Some(frame) = atm_core::protocol::read_frame(
        stream,
        "failed to read daemon request frame during shutdown rejection",
        "daemon request frame exceeded the maximum supported size during shutdown rejection",
    )?
    else {
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
    atm_core::protocol::write_frame(
        stream,
        &frame,
        "failed to write daemon shutdown rejection response frame",
    )?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon shutdown rejection response frame")
            .with_recovery(
                "Retry the ATM command after the daemon restarts; the shutdown rejection response could not be delivered cleanly.",
            )
            .with_source(source)
    })?;
    Ok(ShutdownResponseOutcome::RejectedRequest)
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
