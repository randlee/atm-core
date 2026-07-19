use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::prelude::*;

const LISTENER_WAKE_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const REQUEST_DEADLINE: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeadlineSupport {
    Applied,
    Unsupported,
}

pub(crate) fn schedule_delayed_listener_wake(
    endpoint_path: PathBuf,
    delay: Duration,
) -> Result<(), AtmError> {
    // Fire-and-forget: this helper only opens one local connection to unblock accept(), so
    // shutdown does not need to retain or join the wake thread after it has been scheduled.
    // The `_wake_handle` binding makes that intentional detach explicit at the call site.
    let _wake_handle = std::thread::Builder::new()
        .name("delayed-listener-wake".to_owned())
        .spawn(move || {
            std::thread::sleep(delay);
            let _ = wake_listener(&endpoint_path);
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn delayed listener wake helper")
                .with_recovery(
                    "Retry the ATM command after atm-daemon restarts; the shutdown wake helper could not be scheduled.",
                )
                .with_source(source)
        })?;
    Ok(())
}

pub(crate) fn wake_listener(endpoint_path: &Path) -> Result<(), AtmError> {
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("listener-wake-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(name));
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon listener wake worker")
                .with_recovery(
                    "Restart the daemon; the same-host listener wake path could not create its bounded connect helper.",
                )
                .with_source(source)
        })?;
    let mut stream = match result_rx.recv_timeout(LISTENER_WAKE_CONNECT_DEADLINE) {
        Ok(Ok(stream)) => stream,
        Ok(Err(source)) => {
            return Err(AtmError::daemon_unavailable(format!(
                "failed to wake daemon local IPC listener at {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Restart the daemon; the local IPC listener could not be nudged out of accept cleanly during shutdown.",
            )
            .with_source(source));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            return Err(AtmError::daemon_lifecycle_wedge(format!(
                "timed out waking daemon local IPC listener at {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Restart the daemon; the listener wake connection exceeded the bounded shutdown budget.",
            ));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon listener wake worker disconnected unexpectedly for {}",
                endpoint_path.display()
            ))
            .with_recovery(
                "Restart the daemon; the local IPC wake path aborted before it could connect to the listener.",
            ));
        }
    };
    let send_deadline_support = apply_listener_wake_deadline(
        stream.set_send_timeout(Some(REQUEST_DEADLINE)),
        "failed to apply daemon listener wake timeout",
    )?;
    if send_deadline_support == DeadlineSupport::Unsupported {
        // The wake path only needs the connect itself to unblock accept(). On Windows named
        // pipes, explicit flush can block until the peer drains the send buffer, so once the
        // timeout API is reported as unsupported we drop the empty wake connection immediately
        // instead of reintroducing an unbounded wait while shutting the daemon down.
        return Ok(());
    }
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon listener wake signal")
            .with_recovery(
                "Restart the daemon; the local IPC wake signal could not be flushed to the blocked listener.",
            )
            .with_source(source)
    })?;
    Ok(())
}

fn apply_listener_wake_deadline(
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
                "Restart the daemon; the shutdown wake connection could not apply its bounded send deadline.",
            )
            .with_source(source)),
    }
}
