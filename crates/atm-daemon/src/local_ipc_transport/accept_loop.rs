use std::path::Path;

use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;

use super::{
    AcceptLoopOutcome, LifecycleControlSourceAdapter, ServeLoopSignals, ShutdownBeacon,
    ShutdownResponseOutcome, TERMINATE_REJECTION_GRACE_DEADLINE, record_serve_error,
    record_shutdown_signal, schedule_delayed_listener_wake, wake_listener, write_shutdown_response,
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
