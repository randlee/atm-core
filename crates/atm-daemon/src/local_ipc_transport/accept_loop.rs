use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use atm_core::api::ApiRouter;
use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;

use super::{
    AcceptLoopOutcome, ActiveConnectionRegistry, CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
    LifecycleControlSourceAdapter, ServeLoopSignals, ShutdownBeacon, ShutdownResponseOutcome,
    SubsystemObservability, TERMINATE_REJECTION_GRACE_DEADLINE, handle_connection,
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

pub(super) fn handle_shutdown_probe(
    stream: &mut LocalSocketStream,
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
    endpoint_path: &Path,
    terminate_probe_pending: &mut bool,
) -> Result<AcceptLoopOutcome, AtmError> {
    record_shutdown_signal(lifecycle_control, shutdown_beacon);
    match write_shutdown_response(stream)? {
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

pub(super) fn spawn_connection_worker<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    stream: LocalSocketStream,
    dispatcher: &Arc<dyn ApiRouter + Send + Sync>,
    force_shutdown: &Arc<AtomicBool>,
    registry: &Arc<ActiveConnectionRegistry>,
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
                    &observability,
                )
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    #[cfg(test)]
                    eprintln!("daemon local IPC connection handling failed: {error}");
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
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn local IPC connection worker")


        })
}
