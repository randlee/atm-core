use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use atm_core::boundary::{AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{ConnectionFailureClassification, DaemonConnectionFailureFields};
use atm_core::protocol::{JsonAtmProtocolCodec, ProtocolErrorEnvelope, ResponseEnvelope};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use super::{
    AcceptLoopOutcome, ActiveConnectionRegistry, CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
    LifecycleControlSourceAdapter, MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE, ServeLoopSignals,
    ShutdownBeacon, ShutdownResponseOutcome, SubsystemObservability,
    TERMINATE_REJECTION_GRACE_DEADLINE, TERMINATE_REJECTION_REQUEST_ID, handle_connection,
    record_serve_error, record_shutdown_signal, schedule_delayed_listener_wake, wake_listener,
    write_shutdown_response,
};

pub(super) fn take_accept_error(
    signals: &ServeLoopSignals,
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
) -> Result<Option<AtmError>, AtmError> {
    match signals.take_accept_error()? {
        Some(error) => Ok(Some(record_serve_error(
            lifecycle_control,
            shutdown_beacon,
            error,
        ))),
        None => Ok(None),
    }
}

pub(super) fn maybe_reload_runtime_view<ReloadRuntimeView>(
    signals: &ServeLoopSignals,
    reload_runtime_view: &ReloadRuntimeView,
    observability: &SubsystemObservability,
) -> bool
where
    ReloadRuntimeView: Fn() -> Result<(), AtmError>,
{
    if !signals.take_reload() {
        return false;
    }
    match reload_runtime_view() {
        Ok(()) => {
            observability.emit_or_warn(
                "reload_runtime_view",
                "ok",
                "bounded lifecycle-control-triggered config or roster reload applied",
            );
            tracing::info!("bounded lifecycle-control-triggered config/roster reload applied");
        }
        Err(error) => tracing::warn!(
            subsystem = "local_ipc_transport",
            action = "reload_runtime_view",
            outcome = "rejected",
            error_code = %error.code,
            error_message = %error.message,
            "bounded lifecycle-control-triggered config/roster reload rejected; last-known-good serving config retained"
        ),
    }
    true
}

pub(super) fn handle_shutdown_probe(
    stream: &mut LocalSocketStream,
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
    codec: &JsonAtmProtocolCodec,
    endpoint_path: &Path,
    terminate_probe_pending: &mut bool,
) -> Result<AcceptLoopOutcome, AtmError> {
    record_shutdown_signal(lifecycle_control, shutdown_beacon);
    match write_shutdown_response(stream, codec)? {
        ShutdownResponseOutcome::RejectedRequest => Ok(AcceptLoopOutcome::Break(None)),
        ShutdownResponseOutcome::NoFrame if *terminate_probe_pending => {
            Ok(AcceptLoopOutcome::Break(None))
        }
        ShutdownResponseOutcome::NoFrame => {
            *terminate_probe_pending = true;
            if let Err(error) = schedule_delayed_listener_wake(
                endpoint_path.to_path_buf(),
                TERMINATE_REJECTION_GRACE_DEADLINE,
            ) {
                tracing::warn!(
                    subsystem = "local_ipc_transport",
                    action = "shutdown_probe_wake",
                    outcome = "failed",
                    deadline_ms = TERMINATE_REJECTION_GRACE_DEADLINE.as_millis(),
                    path = %endpoint_path.display(),
                    %error,
                    "failed to schedule delayed listener wake during shutdown probe"
                );
                let _ = wake_listener(endpoint_path);
                return Ok(AcceptLoopOutcome::Break(None));
            }
            Ok(AcceptLoopOutcome::Continue)
        }
    }
}

pub(super) fn reject_connection_when_capped(
    stream: &mut LocalSocketStream,
    codec: &JsonAtmProtocolCodec,
    active_connections: usize,
    observability: &SubsystemObservability,
) -> Result<bool, AtmError> {
    if active_connections < MAX_CONCURRENT_CONNECTIONS {
        return Ok(false);
    }
    observability.emit_event_or_warn(
        observability
            .event(
                "connection_admission",
                "saturated",
                format!(
                    "daemon rejected a same-host connection because the {}-connection cap was already exhausted",
                    MAX_CONCURRENT_CONNECTIONS
                ),
            )
            .with_connection_failure(DaemonConnectionFailureFields {
                code: AtmErrorCode::DaemonConnectionSaturated,
                request_id: atm_core::protocol::RequestId::new(TERMINATE_REJECTION_REQUEST_ID)
                    .expect("nonzero request id"),
                classification: ConnectionFailureClassification::TransportFailure,
            })
            .with_transport_context("connection_cap")
            .with_extra_string_field("active_connections", active_connections.to_string()),
    );
    let response = ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
        &AtmError::new_with_code(
            AtmErrorCode::DaemonConnectionSaturated,
            atm_storage::AtmErrorKind::DaemonUnavailable,
            "daemon connection cap exceeded (max 64 concurrent accepts)",
        )
            .with_recovery(
                "Wait for in-flight ATM commands to complete before retrying, or reduce concurrent atm invocations.",
            ),
    ));
    let frame = codec.response_to_frame(
        atm_core::protocol::RequestId::new(TERMINATE_REJECTION_REQUEST_ID)?,
        response,
    )?;
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
    let _ = atm_core::protocol::write_frame(
        stream,
        &frame,
        "failed to write daemon rejection response frame",
    );
    let _ = stream.flush();
    Ok(true)
}

pub(super) fn spawn_connection_worker<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    stream: LocalSocketStream,
    dispatcher: &Arc<dyn RequestDispatcher + Send + Sync>,
    force_shutdown: &Arc<AtomicBool>,
    registry: &Arc<ActiveConnectionRegistry>,
    codec: JsonAtmProtocolCodec,
    observability: &SubsystemObservability,
) -> Result<(), AtmError> {
    let active = registry.register();
    let dispatcher = Arc::clone(dispatcher);
    let force_shutdown = Arc::clone(force_shutdown);
    let registry = Arc::clone(registry);
    let observability = observability.clone();
    thread::Builder::new()
        .name("local-ipc-connection-worker".to_string())
        .spawn_scoped(scope, move || {
            let _active = active;
            let result = catch_unwind(AssertUnwindSafe(|| {
                handle_connection(
                    stream,
                    dispatcher,
                    force_shutdown.as_ref(),
                    registry,
                    codec,
                    &observability,
                )
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    #[cfg(test)]
                    eprintln!("daemon local IPC connection handling failed: {error}");
                    super::request_worker::emit_connection_failure_event(
                        &observability,
                        &error,
                        atm_core::protocol::RequestId::new(TERMINATE_REJECTION_REQUEST_ID)
                            .expect("nonzero request id"),
                        "connection_worker",
                    );
                    tracing::warn!(
                        subsystem = "local_ipc_transport",
                        action = "connection_worker",
                        outcome = "classified_failure",
                        %error,
                        "daemon local IPC connection handling failed"
                    );
                }
                Err(_) => {
                    #[cfg(test)]
                    eprintln!("{CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE}");
                    observability.emit_or_warn(
                        "connection_worker",
                        "panic",
                        CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
                    );
                    tracing::warn!(
                        subsystem = "local_ipc_transport",
                        action = "connection_worker",
                        outcome = "panic",
                        "daemon local IPC connection worker panicked; the transport thread recovered and continued shutdown accounting"
                    );
                }
            }
        })
        .map(|_| ())
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn local IPC connection worker")
                .with_recovery(
                    "Restart the daemon after confirming the host can spawn same-host connection workers.",
                )
                .with_source(source)
        })
}
