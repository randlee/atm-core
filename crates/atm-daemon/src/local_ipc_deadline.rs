use std::time::Duration;

use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeadlineSupport {
    Applied,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalIpcDeadlineSupport {
    pub(crate) read: DeadlineSupport,
    pub(crate) write: DeadlineSupport,
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
    pub(crate) background_work_registry:
        Option<std::sync::Arc<crate::active_connection_registry::ActiveConnectionRegistry>>,
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
            // The underlying local IPC read/write call cannot be force-cancelled once the OS
            // reports deadline APIs as unsupported. We therefore detach the blocking worker but
            // register it in the daemon's active-work registry when available so shutdown drain
            // and same-host work accounting can still observe the stalled helper and fail within
            // the documented bounded deadline instead of leaking it silently.
            let background_work_registry = config.background_work_registry.clone();
            std::thread::Builder::new()
                .name(config.worker_name.to_owned())
                .spawn(move || {
                    let _background_work = background_work_registry
                        .map(|registry| registry.register_background_work());
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::time::Instant;

    use interprocess::local_socket::ListenerOptions;
    use interprocess::local_socket::traits::{Listener as _, Stream as _};
    use tempfile::TempDir;

    use crate::active_connection_registry::ActiveConnectionRegistry;
    use crate::local_ipc_connection::drain_active_connections_for_shutdown;

    fn connected_stream() -> LocalSocketStream {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint_path = tempdir.path().join("deadline.sock");
        let name = atm_core::protocol::daemon_local_ipc_name_from_path(&endpoint_path)
            .expect("ipc name")
            .into_owned();
        let listener = ListenerOptions::new()
            .name(name.clone())
            .create_sync()
            .expect("listener");
        let (accepted_tx, accepted_rx) = mpsc::sync_channel(1);
        let accept_thread = std::thread::Builder::new()
            .name("local-ipc-deadline-test-accept".to_string())
            .spawn(move || {
                let stream = listener.accept().expect("accept");
                accepted_tx.send(stream).expect("send accepted stream");
            })
            .expect("spawn accept thread");
        let client = LocalSocketStream::connect(name).expect("connect");
        let _accepted = accepted_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("accepted stream");
        accept_thread.join().expect("join accept thread");
        client
    }

    fn unsupported_config(deadline: Duration) -> OwnedLocalIpcDeadlineConfig {
        OwnedLocalIpcDeadlineConfig {
            deadline,
            support: DeadlineSupport::Unsupported,
            worker_name: "local-ipc-deadline-test-helper",
            timeout_error: AtmError::daemon_unavailable("deadline helper timed out"),
            disconnect_error: AtmError::daemon_unavailable("deadline helper disconnected"),
            spawn_error_message: "failed to spawn deadline helper",
            spawn_error_recovery: "retry the local IPC deadline test helper",
            background_work_registry: None,
        }
    }

    #[test]
    fn unsupported_helper_returns_result_before_deadline() {
        let stream = connected_stream();
        let (_stream, result) = run_owned_local_ipc_with_deadline(
            stream,
            unsupported_config(Duration::from_millis(50)),
            |_stream| Ok::<_, AtmError>(42usize),
        )
        .expect("helper result");
        assert_eq!(result, 42);
    }

    #[test]
    fn unsupported_helper_returns_timeout_error_after_deadline() {
        let (_release_tx, release_rx) = mpsc::sync_channel::<()>(1);
        let stream = connected_stream();
        let error = run_owned_local_ipc_with_deadline(
            stream,
            unsupported_config(Duration::from_millis(10)),
            move |_stream| {
                let _ = release_rx.recv_timeout(Duration::from_millis(50));
                Ok::<_, AtmError>(())
            },
        )
        .expect_err("timeout");
        assert!(error.message.contains("deadline helper timed out"));
    }

    #[test]
    fn unsupported_helper_returns_disconnect_error_when_worker_panics() {
        let stream = connected_stream();
        let error = run_owned_local_ipc_with_deadline::<(), _>(
            stream,
            unsupported_config(Duration::from_millis(50)),
            |_stream| panic!("intentional helper panic for disconnect branch"),
        )
        .expect_err("disconnect");
        assert!(error.message.contains("deadline helper disconnected"));
    }

    #[test]
    fn tracked_unsupported_helper_keeps_shutdown_drain_bounded() {
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let force_shutdown = AtomicBool::new(false);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let stream = connected_stream();
        let error = run_owned_local_ipc_with_deadline(
            stream,
            OwnedLocalIpcDeadlineConfig {
                background_work_registry: Some(Arc::clone(&registry)),
                ..unsupported_config(Duration::from_millis(10))
            },
            move |_stream| {
                started_tx.send(()).expect("signal helper start");
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
                Ok::<_, AtmError>(())
            },
        )
        .expect_err("helper timeout");
        assert!(error.message.contains("deadline helper timed out"));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("helper started");

        let shutdown_error = drain_active_connections_for_shutdown(
            registry.as_ref(),
            &force_shutdown,
            Duration::from_millis(5),
            Duration::from_millis(20),
            Instant::now(),
            Duration::from_millis(5),
        )
        .expect_err("shutdown drain should stay bounded by the forced-cancel deadline");
        assert!(force_shutdown.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            shutdown_error
                .message
                .contains("forced cancel deadline elapsed"),
            "unexpected error: {shutdown_error:?}"
        );

        let _ = release_tx.send(());
    }
}
