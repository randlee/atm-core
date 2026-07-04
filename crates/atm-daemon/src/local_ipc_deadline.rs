use std::time::Duration;

use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeadlineSupport {
    Applied,
    Unsupported,
}

#[derive(Debug)]
pub(crate) struct OwnedLocalIpcDeadlineConfig {
    pub(crate) deadline: Duration,
    pub(crate) support: DeadlineSupport,
    pub(crate) worker_name: &'static str,
    pub(crate) timeout_error: AtmError,
    pub(crate) disconnect_error: AtmError,
    pub(crate) spawn_error_message: &'static str,
    pub(crate) spawn_error_recovery: &'static str,
}

pub(crate) fn apply_optional_deadline(
    result: std::io::Result<()>,
    message: &'static str,
    recovery: &'static str,
) -> Result<DeadlineSupport, AtmError> {
    match result {
        Ok(()) => Ok(DeadlineSupport::Applied),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => {
            Ok(DeadlineSupport::Unsupported)
        }
        Err(source) => Err(AtmError::daemon_unavailable(message)
            .with_recovery(recovery)
            .with_source(source)),
    }
}

pub(crate) fn run_owned_local_ipc_with_deadline<T, F>(
    mut stream: LocalSocketStream,
    config: OwnedLocalIpcDeadlineConfig,
    operation: F,
) -> Result<(LocalSocketStream, T), AtmError>
where
    T: Send + 'static,
    F: FnOnce(&mut LocalSocketStream) -> Result<T, AtmError> + Send + 'static,
{
    match config.support {
        DeadlineSupport::Applied => {
            let result = operation(&mut stream)?;
            Ok((stream, result))
        }
        DeadlineSupport::Unsupported => {
            let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name(config.worker_name.to_owned())
                .spawn(move || {
                    let result = operation(&mut stream);
                    let _ = result_tx.send((stream, result));
                })
                .map_err(|source| {
                    AtmError::daemon_unavailable(config.spawn_error_message)
                        .with_recovery(config.spawn_error_recovery)
                        .with_source(source)
                })?;
            match result_rx.recv_timeout(config.deadline) {
                Ok((stream, Ok(result))) => Ok((stream, result)),
                Ok((_, Err(error))) => Err(error),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(config.timeout_error),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err(config.disconnect_error)
                }
            }
        }
    }
}
