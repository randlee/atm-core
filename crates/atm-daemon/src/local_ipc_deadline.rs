use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::protocol::FramePayload;
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(windows)]
use interprocess::local_socket::traits::Stream as _;

#[cfg(windows)]
const WINDOWS_NONBLOCKING_POLL_INTERVAL: Duration = Duration::from_millis(100);

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

pub(crate) enum ReadFrameDeadlineOutcome {
    EndOfStream,
    Frame(FramePayload),
    #[cfg(windows)]
    TimedOut,
}

#[derive(Debug)]
#[cfg(any(test, not(windows)))]
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

#[cfg(any(test, not(windows)))]
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

pub(crate) fn write_frame_with_optional_deadline(
    mut stream: LocalSocketStream,
    deadline: Duration,
    support: DeadlineSupport,
    frame: &FramePayload,
    write_error: &'static str,
    flush_error: &'static str,
    timeout_error: AtmError,
) -> Result<(LocalSocketStream, ()), AtmError> {
    match support {
        DeadlineSupport::Applied => {
            atm_core::protocol::write_frame(&mut stream, frame, write_error)?;
            std::io::Write::flush(&mut stream)
                .map_err(|source| AtmError::daemon_unavailable(flush_error).with_source(source))?;
            Ok((stream, ()))
        }
        #[cfg(windows)]
        DeadlineSupport::Unsupported => write_frame_nonblocking(
            stream,
            deadline,
            frame,
            write_error,
            flush_error,
            timeout_error,
        ),
        #[cfg(not(windows))]
        DeadlineSupport::Unsupported => write_frame_with_helper_deadline(
            stream,
            deadline,
            frame,
            write_error,
            flush_error,
            timeout_error,
        ),
    }
}

pub(crate) fn read_frame_with_optional_deadline(
    mut stream: LocalSocketStream,
    deadline: Duration,
    support: DeadlineSupport,
    #[cfg(windows)] stop_requested: Option<&std::sync::atomic::AtomicBool>,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<(LocalSocketStream, ReadFrameDeadlineOutcome), AtmError> {
    match support {
        DeadlineSupport::Applied => {
            let frame = atm_core::protocol::read_frame(&mut stream, read_error, oversize_error)?;
            Ok((
                stream,
                match frame {
                    Some(frame) => ReadFrameDeadlineOutcome::Frame(frame),
                    None => ReadFrameDeadlineOutcome::EndOfStream,
                },
            ))
        }
        #[cfg(windows)]
        DeadlineSupport::Unsupported => {
            read_frame_nonblocking(stream, deadline, stop_requested, read_error, oversize_error)
        }
        #[cfg(not(windows))]
        DeadlineSupport::Unsupported => {
            read_frame_with_helper_deadline(stream, deadline, read_error, oversize_error)
        }
    }
}

#[cfg(not(windows))]
fn write_frame_with_helper_deadline(
    stream: LocalSocketStream,
    deadline: Duration,
    frame: &FramePayload,
    write_error: &'static str,
    flush_error: &'static str,
    timeout_error: AtmError,
) -> Result<(LocalSocketStream, ()), AtmError> {
    let frame = frame.clone();
    run_owned_local_ipc_with_deadline(
        stream,
        OwnedLocalIpcDeadlineConfig {
            deadline,
            support: DeadlineSupport::Unsupported,
            worker_name: "local-ipc-write-deadline-helper",
            timeout_error,
            disconnect_error: AtmError::daemon_unavailable(
                "daemon local IPC write helper disconnected before the bounded write completed",
            )
            .with_recovery(
                "Retry the ATM command after the daemon restarts; the same-host fallback write worker stopped unexpectedly.",
            ),
            spawn_error_message: "failed to spawn daemon local IPC write helper",
            spawn_error_recovery:
                "Retry the ATM command after the daemon restarts; the same-host fallback write worker could not be created.",
            background_work_registry: None,
        },
        move |stream| {
            atm_core::protocol::write_frame(stream, &frame, write_error)?;
            std::io::Write::flush(stream)
                .map_err(|source| AtmError::daemon_unavailable(flush_error).with_source(source))?;
            Ok(())
        },
    )
}

#[cfg(not(windows))]
fn read_frame_with_helper_deadline(
    stream: LocalSocketStream,
    deadline: Duration,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<(LocalSocketStream, ReadFrameDeadlineOutcome), AtmError> {
    run_owned_local_ipc_with_deadline(
        stream,
        OwnedLocalIpcDeadlineConfig {
            deadline,
            support: DeadlineSupport::Unsupported,
            worker_name: "local-ipc-read-deadline-helper",
            timeout_error: AtmError::daemon_unavailable(
                "daemon local IPC read exceeded the runtime deadline",
            )
            .with_recovery(
                "Retry the ATM command after the daemon restarts; the same-host fallback read worker did not complete within the bounded deadline.",
            ),
            disconnect_error: AtmError::daemon_unavailable(
                "daemon local IPC read helper disconnected before the bounded read completed",
            )
            .with_recovery(
                "Retry the ATM command after the daemon restarts; the same-host fallback read worker stopped unexpectedly.",
            ),
            spawn_error_message: "failed to spawn daemon local IPC read helper",
            spawn_error_recovery:
                "Retry the ATM command after the daemon restarts; the same-host fallback read worker could not be created.",
            background_work_registry: None,
        },
        move |stream| {
            Ok(match atm_core::protocol::read_frame(stream, read_error, oversize_error)? {
                Some(frame) => ReadFrameDeadlineOutcome::Frame(frame),
                None => ReadFrameDeadlineOutcome::EndOfStream,
            })
        },
    )
}

#[cfg(windows)]
fn write_frame_nonblocking(
    mut stream: LocalSocketStream,
    deadline: Duration,
    frame: &FramePayload,
    write_error: &'static str,
    flush_error: &'static str,
    timeout_error: AtmError,
) -> Result<(LocalSocketStream, ()), AtmError> {
    let bytes = encode_frame_bytes(frame, write_error)?;
    with_windows_nonblocking(&mut stream, |stream| {
        let started = std::time::Instant::now();
        let written = poll_write_bytes_with(
            started,
            deadline,
            &bytes,
            WINDOWS_NONBLOCKING_POLL_INTERVAL,
            |slice| std::io::Write::write(stream, slice),
        )
        .map_err(|source| AtmError::daemon_unavailable(write_error).with_source(source))?;
        if written != bytes.len() {
            return Err(clone_timeout_error(&timeout_error));
        }
        let flushed =
            poll_unit_until_complete(started, deadline, WINDOWS_NONBLOCKING_POLL_INTERVAL, || {
                std::io::Write::flush(stream)
            })
            .map_err(|source| AtmError::daemon_unavailable(flush_error).with_source(source))?;
        if !flushed {
            return Err(clone_timeout_error(&timeout_error));
        }
        Ok(())
    })?;
    Ok((stream, ()))
}

#[cfg(windows)]
fn read_frame_nonblocking(
    mut stream: LocalSocketStream,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<(LocalSocketStream, ReadFrameDeadlineOutcome), AtmError> {
    let result = with_windows_nonblocking(&mut stream, |stream| {
        let started = std::time::Instant::now();
        let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
        let header_read = poll_read_bytes_with(
            started,
            deadline,
            stop_requested,
            &mut header,
            WINDOWS_NONBLOCKING_POLL_INTERVAL,
            |buffer| std::io::Read::read(stream, buffer),
        )
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
        if header_read == 0 {
            return Ok(ReadFrameDeadlineOutcome::EndOfStream);
        }
        if header_read != header.len() {
            return Ok(ReadFrameDeadlineOutcome::TimedOut);
        }
        let (request_id, message_kind, flags, payload_length) =
            decode_frame_header(header, oversize_error)?;
        let mut payload = vec![0u8; payload_length];
        let payload_read = poll_read_bytes_with(
            started,
            deadline,
            stop_requested,
            &mut payload,
            WINDOWS_NONBLOCKING_POLL_INTERVAL,
            |buffer| std::io::Read::read(stream, buffer),
        )
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
        if payload_read != payload.len() {
            return Ok(ReadFrameDeadlineOutcome::TimedOut);
        }
        Ok(ReadFrameDeadlineOutcome::Frame(FramePayload {
            request_id,
            message_kind,
            flags,
            bytes: payload,
        }))
    })?;
    Ok((stream, result))
}

#[cfg(windows)]
fn with_windows_nonblocking<T>(
    stream: &mut LocalSocketStream,
    operation: impl FnOnce(&mut LocalSocketStream) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    stream.set_nonblocking(true).map_err(|source| {
        AtmError::daemon_unavailable(
            "failed to place the Windows same-host pipe into nonblocking mode",
        )
        .with_recovery(
            "Restart the daemon; the Windows same-host pipe could not enter the bounded nonblocking fallback path.",
        )
        .with_source(source)
    })?;
    let result = operation(stream);
    stream.set_nonblocking(false).map_err(|source| {
        AtmError::daemon_unavailable(
            "failed to restore blocking mode on the Windows same-host pipe",
        )
        .with_recovery(
            "Restart the daemon; the Windows same-host pipe could not return to blocking mode after the bounded fallback path.",
        )
        .with_source(source)
    })?;
    result
}

#[cfg(windows)]
fn poll_read_bytes_with<ReadFn>(
    started: std::time::Instant,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
    buffer: &mut [u8],
    poll_interval: Duration,
    mut read_fn: ReadFn,
) -> std::io::Result<usize>
where
    ReadFn: FnMut(&mut [u8]) -> std::io::Result<usize>,
{
    use std::sync::atomic::Ordering;

    let mut offset = 0usize;
    while offset < buffer.len() {
        match read_fn(&mut buffer[offset..]) {
            Ok(0) if offset == 0 => return Ok(0),
            Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
            Ok(read) => offset += read,
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                if stop_requested
                    .map(|flag| flag.load(Ordering::SeqCst))
                    .unwrap_or(false)
                    || started.elapsed() >= deadline
                {
                    return Ok(offset);
                }
                let remaining = deadline.saturating_sub(started.elapsed());
                std::thread::sleep(std::cmp::min(remaining, poll_interval));
            }
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(source) => return Err(source),
        }
    }
    Ok(offset)
}

#[cfg(windows)]
fn poll_write_bytes_with<WriteFn>(
    started: std::time::Instant,
    deadline: Duration,
    buffer: &[u8],
    poll_interval: Duration,
    mut write_fn: WriteFn,
) -> std::io::Result<usize>
where
    WriteFn: FnMut(&[u8]) -> std::io::Result<usize>,
{
    let mut offset = 0usize;
    while offset < buffer.len() {
        match write_fn(&buffer[offset..]) {
            Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::WriteZero)),
            Ok(written) => offset += written,
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= deadline {
                    return Ok(offset);
                }
                let remaining = deadline.saturating_sub(started.elapsed());
                std::thread::sleep(std::cmp::min(remaining, poll_interval));
            }
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(source) => return Err(source),
        }
    }
    Ok(offset)
}

#[cfg(windows)]
fn poll_unit_until_complete<Op>(
    started: std::time::Instant,
    deadline: Duration,
    poll_interval: Duration,
    mut op: Op,
) -> std::io::Result<bool>
where
    Op: FnMut() -> std::io::Result<()>,
{
    loop {
        match op() {
            Ok(()) => return Ok(true),
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= deadline {
                    return Ok(false);
                }
                let remaining = deadline.saturating_sub(started.elapsed());
                std::thread::sleep(std::cmp::min(remaining, poll_interval));
            }
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(source) => return Err(source),
        }
    }
}

#[cfg(windows)]
fn encode_frame_bytes(
    frame: &FramePayload,
    _write_error: &'static str,
) -> Result<Vec<u8>, AtmError> {
    if frame.flags != atm_core::protocol::ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame flags 0x{:04x} for version {}",
            frame.flags,
            atm_core::protocol::ATM_FRAME_VERSION_V1
        ))
        .with_recovery(
            "Retry with a supported ATM daemon client/server build that uses protocol version 1 flags.",
        ));
    }
    if frame.bytes.len() > atm_core::protocol::MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "daemon frame exceeded the maximum supported size",
        )
        .with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }
    let mut bytes =
        Vec::with_capacity(atm_core::protocol::ATM_FRAME_HEADER_BYTES + frame.bytes.len());
    let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
    header[0..4].copy_from_slice(&atm_core::protocol::ATM_FRAME_MAGIC.to_be_bytes());
    header[4..6].copy_from_slice(&atm_core::protocol::ATM_FRAME_VERSION_V1.to_be_bytes());
    header[6..8].copy_from_slice(&frame.message_kind.code().to_be_bytes());
    header[8..10].copy_from_slice(&frame.flags.to_be_bytes());
    header[10..18].copy_from_slice(&frame.request_id.into_inner().to_be_bytes());
    header[18..22].copy_from_slice(&(frame.bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&frame.bytes);
    if bytes.is_empty() {
        return Err(AtmError::daemon_unavailable(write_error));
    }
    Ok(bytes)
}

#[cfg(windows)]
fn decode_frame_header(
    header: [u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES],
    oversize_error: &'static str,
) -> Result<
    (
        atm_core::protocol::RequestId,
        atm_core::protocol::MessageKind,
        u16,
        usize,
    ),
    AtmError,
> {
    let magic = u32::from_be_bytes(header[0..4].try_into().expect("magic"));
    if magic != atm_core::protocol::ATM_FRAME_MAGIC {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame magic 0x{magic:08x}"
        ))
        .with_recovery(
            "Retry with an ATM client and daemon build that both speak the documented ATM daemon protocol.",
        ));
    }

    let version = u16::from_be_bytes(header[4..6].try_into().expect("version"));
    if version != atm_core::protocol::ATM_FRAME_VERSION_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame version {version}"
        ))
        .with_recovery(
            "Align the CLI and daemon builds so both sides use the same ATM daemon protocol version before retrying.",
        ));
    }

    let message_kind = atm_core::protocol::MessageKind::try_from(u16::from_be_bytes(
        header[6..8].try_into().expect("kind"),
    ))?;
    let flags = u16::from_be_bytes(header[8..10].try_into().expect("flags"));
    if flags != atm_core::protocol::ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame flags 0x{flags:04x} for version {version}"
        ))
        .with_recovery(
            "Retry with a supported ATM daemon client/server build that uses the version-1 flag contract.",
        ));
    }
    let request_id = atm_core::protocol::RequestId::new(u64::from_be_bytes(
        header[10..18].try_into().expect("request id"),
    ))?;
    let payload_length = u32::from_be_bytes(header[18..22].try_into().expect("payload")) as usize;
    if payload_length > atm_core::protocol::MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }
    Ok((request_id, message_kind, flags, payload_length))
}

#[cfg(windows)]
fn clone_timeout_error(template: &AtmError) -> AtmError {
    let mut error = AtmError::new_with_code(template.code, template.kind, template.message.clone());
    for recovery in &template.recovery {
        error = error.with_recovery(recovery.clone());
    }
    error
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

    fn connected_stream_pair() -> (LocalSocketStream, LocalSocketStream) {
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
        let accepted = accepted_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("accepted stream");
        accept_thread.join().expect("join accept thread");
        (client, accepted)
    }

    fn connected_stream() -> LocalSocketStream {
        connected_stream_pair().0
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

    #[cfg(windows)]
    #[test]
    fn poll_read_bytes_with_retries_interrupted_reads() {
        let started = Instant::now();
        let mut buffer = [0u8; 4];
        let mut call = 0usize;
        let read = poll_read_bytes_with(
            started,
            Duration::from_secs(1),
            None,
            &mut buffer,
            Duration::from_millis(0),
            |slice| {
                call += 1;
                match call {
                    1 | 3 => Err(std::io::Error::from(std::io::ErrorKind::Interrupted)),
                    2 => {
                        slice[..2].copy_from_slice(&[1, 2]);
                        Ok(2)
                    }
                    4 => {
                        slice[..2].copy_from_slice(&[3, 4]);
                        Ok(2)
                    }
                    _ => unreachable!("unexpected poll_read_bytes_with call {call}"),
                }
            },
        )
        .expect("poll read succeeds");
        assert_eq!(read, buffer.len());
        assert_eq!(buffer, [1, 2, 3, 4]);
    }

    #[cfg(windows)]
    #[test]
    fn read_frame_with_optional_deadline_times_out_after_partial_header_stall() {
        let (client, mut server) = connected_stream_pair();
        let frame = FramePayload {
            request_id: atm_core::protocol::RequestId::new(1).expect("request id"),
            message_kind: atm_core::protocol::MessageKind::Request,
            flags: atm_core::protocol::ATM_FRAME_FLAGS_V1,
            bytes: b"ping".to_vec(),
        };
        let encoded = encode_frame_bytes(&frame, "encode frame").expect("encode frame");
        let partial_header_len = atm_core::protocol::ATM_FRAME_HEADER_BYTES - 1;
        std::io::Write::write_all(&mut server, &encoded[..partial_header_len])
            .expect("write partial header");
        std::io::Write::flush(&mut server).expect("flush partial header");

        let (_stream, outcome) = read_frame_with_optional_deadline(
            client,
            Duration::from_millis(20),
            DeadlineSupport::Unsupported,
            None,
            "failed to read daemon request frame",
            "daemon request frame exceeded the maximum supported size",
        )
        .expect("bounded read outcome");

        assert!(matches!(outcome, ReadFrameDeadlineOutcome::TimedOut));
    }
}
