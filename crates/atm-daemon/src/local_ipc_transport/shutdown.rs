use super::*;
use std::fs;
use std::time::Instant;

pub(super) fn finalize_serve_loop<BeginShutdown>(
    begin_shutdown: &BeginShutdown,
    context: ServeShutdownContext<'_>,
    lifecycle_waiter: std::thread::ScopedJoinHandle<'_, ()>,
) -> Option<AtmError>
where
    BeginShutdown: Fn() -> Result<(), AtmError>,
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
            ),
        );
    }
    if let Err(error) = context.lifecycle_control.shutdown_worker_with_timeout() {
        append_shutdown_error(&mut shutdown_error, "lifecycle_worker_error", error);
    }
    if let Err(error) = context.endpoint_guard.unpublish() {
        append_shutdown_error(&mut shutdown_error, "endpoint_cleanup_error", error);
    }
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

pub(super) fn prepare_local_ipc_endpoint(
    endpoint_path: &Path,
) -> Result<LocalIpcEndpointPreparation, AtmError> {
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon local IPC directory at {}",
                parent.display()
            ))
        })?;
    }
    #[cfg(unix)]
    remove_stale_endpoint(endpoint_path)?;
    Ok(LocalIpcEndpointPreparation::FilesystemEndpointPrepared)
}

#[cfg(unix)]
pub(super) fn remove_stale_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if !endpoint_path.exists() {
        return Ok(());
    }
    match fs::remove_file(endpoint_path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_source) => Err(AtmError::daemon_unavailable(format!(
            "failed to remove stale daemon local IPC endpoint at {}",
            endpoint_path.display()
        ))),
    }
}

pub(super) fn write_shutdown_response(
    stream: &mut LocalSocketStream,
) -> Result<ShutdownResponseOutcome, AtmError> {
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
    if atm_core::api::read_http_request(stream)?.is_none() {
        return Ok(ShutdownResponseOutcome::NoFrame);
    }
    let response = ResponseEnvelope::Error(AtmError::daemon_unavailable(
        "daemon is shutting down and not accepting new requests",
    ));
    atm_core::api::write_http_response(stream, &response)?;
    Ok(ShutdownResponseOutcome::RejectedRequest)
}

pub(super) fn emit_ready_signal_if_requested() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_READY_STDOUT").is_none() {
        return Ok(());
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "ATM_DAEMON_READY")
        .map_err(|_source| AtmError::daemon_unavailable("failed to emit daemon ready signal"))?;
    stdout
        .flush()
        .map_err(|_source| AtmError::daemon_unavailable("failed to flush daemon ready signal"))?;
    Ok(())
}
