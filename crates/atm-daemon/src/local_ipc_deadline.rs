use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::protocol::FramePayload;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

const NONBLOCKING_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
    TimedOut,
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

pub(crate) fn write_frame_with_optional_deadline(
    mut stream: LocalSocketStream,
    deadline: Duration,
    support: DeadlineSupport,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
    frame: &FramePayload,
    error_messages: (&'static str, &'static str),
    timeout_error: AtmError,
) -> Result<(LocalSocketStream, ()), AtmError> {
    let (write_error, flush_error) = error_messages;
    match support {
        DeadlineSupport::Applied => {
            atm_core::protocol::write_frame(&mut stream, frame, write_error)?;
            std::io::Write::flush(&mut stream)
                .map_err(|source| AtmError::daemon_unavailable(flush_error).with_source(source))?;
            Ok((stream, ()))
        }
        DeadlineSupport::Unsupported => write_frame_nonblocking(
            stream,
            deadline,
            stop_requested,
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
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
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
        DeadlineSupport::Unsupported => {
            read_frame_nonblocking(stream, deadline, stop_requested, read_error, oversize_error)
        }
    }
}

fn write_frame_nonblocking(
    mut stream: LocalSocketStream,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
    frame: &FramePayload,
    write_error: &'static str,
    flush_error: &'static str,
    timeout_error: AtmError,
) -> Result<(LocalSocketStream, ()), AtmError> {
    let bytes = encode_frame_bytes(frame, write_error)?;
    with_nonblocking_mode(&mut stream, |stream| {
        let started = std::time::Instant::now();
        let written = poll_write_bytes_with(
            started,
            deadline,
            stop_requested,
            &bytes,
            NONBLOCKING_POLL_INTERVAL,
            |slice| std::io::Write::write(stream, slice),
        )
        .map_err(|source| AtmError::daemon_unavailable(write_error).with_source(source))?;
        if written != bytes.len() {
            return Err(clone_timeout_error(&timeout_error));
        }
        let flushed = poll_unit_until_complete(
            started,
            deadline,
            stop_requested,
            NONBLOCKING_POLL_INTERVAL,
            || std::io::Write::flush(stream),
        )
        .map_err(|source| AtmError::daemon_unavailable(flush_error).with_source(source))?;
        if !flushed {
            return Err(clone_timeout_error(&timeout_error));
        }
        Ok(())
    })?;
    Ok((stream, ()))
}

fn read_frame_nonblocking(
    mut stream: LocalSocketStream,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<(LocalSocketStream, ReadFrameDeadlineOutcome), AtmError> {
    let result = with_nonblocking_mode(&mut stream, |stream| {
        let started = std::time::Instant::now();
        let mut header = [0u8; atm_core::protocol::ATM_FRAME_HEADER_BYTES];
        let header_read = poll_read_bytes_with(
            started,
            deadline,
            stop_requested,
            &mut header,
            NONBLOCKING_POLL_INTERVAL,
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
            NONBLOCKING_POLL_INTERVAL,
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

fn with_nonblocking_mode<T>(
    stream: &mut LocalSocketStream,
    operation: impl FnOnce(&mut LocalSocketStream) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    stream.set_nonblocking(true).map_err(|source| {
        AtmError::daemon_unavailable("failed to place the same-host IPC stream into nonblocking mode")
        .with_recovery(
            "Restart the daemon; the same-host IPC stream could not enter the bounded nonblocking fallback path.",
        )
        .with_source(source)
    })?;
    let result = operation(stream);
    if let Err(source) = stream.set_nonblocking(false) {
        let error = AtmError::daemon_unavailable(
            "failed to restore blocking mode on the same-host IPC stream",
        )
        .with_recovery(
            "Restart the daemon; the same-host IPC stream could not return to blocking mode after the bounded fallback path.",
        )
        .with_source(source);
        tracing::warn!(
            %error,
            "same-host IPC stream failed to restore blocking mode after bounded nonblocking fallback"
        );
    }
    result
}

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
    let mut offset = 0usize;
    while offset < buffer.len() {
        match read_fn(&mut buffer[offset..]) {
            Ok(0) if offset == 0 => return Ok(0),
            Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
            Ok(read) => offset += read,
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                if stop_requested_or_deadline_elapsed(started, deadline, stop_requested) {
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

fn poll_write_bytes_with<WriteFn>(
    started: std::time::Instant,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
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
                if stop_requested_or_deadline_elapsed(started, deadline, stop_requested) {
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

fn poll_unit_until_complete<Op>(
    started: std::time::Instant,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
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
                if stop_requested_or_deadline_elapsed(started, deadline, stop_requested) {
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

fn stop_requested_or_deadline_elapsed(
    started: std::time::Instant,
    deadline: Duration,
    stop_requested: Option<&std::sync::atomic::AtomicBool>,
) -> bool {
    use std::sync::atomic::Ordering;

    stop_requested
        .map(|flag| flag.load(Ordering::SeqCst))
        .unwrap_or(false)
        || started.elapsed() >= deadline
}

fn encode_frame_bytes(
    frame: &FramePayload,
    write_error: &'static str,
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

    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::time::Instant;

    use interprocess::local_socket::ListenerOptions;
    use interprocess::local_socket::traits::Listener as _;
    use tempfile::TempDir;

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

    #[test]
    fn read_frame_with_optional_deadline_times_out_after_partial_header_stall() {
        let (client, mut server) = connected_stream_pair();
        let frame = FramePayload {
            request_id: atm_core::protocol::RequestId::new(1).expect("request id"),
            message_kind: atm_core::protocol::MessageKind::DoctorRequest,
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

    #[test]
    fn poll_read_bytes_with_exits_early_when_stop_requested_flips() {
        let stop_requested = AtomicBool::new(false);
        let started = Instant::now();
        let mut first_block = true;
        let mut buffer = [0u8; 8];
        let read = poll_read_bytes_with(
            started,
            Duration::from_secs(1),
            Some(&stop_requested),
            &mut buffer,
            Duration::from_millis(1),
            |_slice| {
                if first_block {
                    first_block = false;
                    stop_requested.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
            },
        )
        .expect("poll read exits cleanly");
        assert_eq!(read, 0);
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "poll read should stop well before the full deadline"
        );
    }

    #[test]
    fn poll_write_bytes_with_times_out_when_receiver_never_drains() {
        let started = Instant::now();
        let written = poll_write_bytes_with(
            started,
            Duration::from_millis(20),
            None,
            &[1, 2, 3, 4],
            Duration::from_millis(1),
            |_slice| Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)),
        )
        .expect("poll write returns bounded timeout result");
        assert_eq!(written, 0);
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "poll write timeout should remain bounded"
        );
    }
}
