use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::prelude::*;

const LISTENER_WAKE_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const LISTENER_WAKE_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);

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
            let _ = wake_listener_until(&endpoint_path, REQUEST_DEADLINE);
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn delayed listener wake helper")
        })?;
    Ok(())
}

pub(crate) fn wake_listener_until(
    endpoint_path: &Path,
    retry_for: Duration,
) -> Result<(), AtmError> {
    let started = Instant::now();
    let mut last_error = match wake_listener(endpoint_path) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    loop {
        let elapsed = started.elapsed();
        if elapsed >= retry_for {
            return Err(last_error);
        }
        std::thread::sleep(std::cmp::min(
            LISTENER_WAKE_RETRY_INTERVAL,
            retry_for.saturating_sub(elapsed),
        ));
        match wake_listener(endpoint_path) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
    }
}

pub(crate) fn wake_listener(endpoint_path: &Path) -> Result<(), AtmError> {
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("listener-wake-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(name));
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn daemon listener wake worker")
        })?;
    let mut stream = match result_rx.recv_timeout(LISTENER_WAKE_CONNECT_DEADLINE) {
        Ok(Ok(stream)) => stream,
        Ok(Err(_source)) => {
            return Err(AtmError::daemon_unavailable(format!(
                "failed to wake daemon local IPC listener at {}",
                endpoint_path.display()
            )));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            return Err(AtmError::daemon_lifecycle_wedge(format!(
                "timed out waking daemon local IPC listener at {}",
                endpoint_path.display()
            )));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon listener wake worker disconnected unexpectedly for {}",
                endpoint_path.display()
            )));
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
    stream.flush().map_err(|_source| {
        AtmError::daemon_unavailable("failed to flush daemon listener wake signal")
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
        Err(_source) => Err(AtmError::daemon_unavailable(message)),
    }
}
