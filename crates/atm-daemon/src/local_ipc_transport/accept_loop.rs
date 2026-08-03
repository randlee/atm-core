use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;

use super::{
    AcceptLoopOutcome, LifecycleControlSourceAdapter, ServeLoopSignals, ShutdownBeacon,
    ShutdownResponseOutcome, record_serve_error, record_shutdown_signal, write_shutdown_response,
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
) -> Result<AcceptLoopOutcome, AtmError> {
    record_shutdown_signal(lifecycle_control, shutdown_beacon);
    match write_shutdown_response(stream)? {
        ShutdownResponseOutcome::RejectedRequest => Ok(AcceptLoopOutcome::Break(None)),
        ShutdownResponseOutcome::NoFrame => Ok(AcceptLoopOutcome::Break(None)),
    }
}
