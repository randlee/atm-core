//! Local HTTP record, deadline, and response helpers for the daemon client.

use std::fs;
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_storage::AtmError;

use crate::LocalIpcDeadlineSupport;

pub(crate) fn write_local_http_request(
    writer: &mut TcpStream,
    request: &RequestEnvelope,
    record_path: &Path,
) -> Result<(), AtmError> {
    let record = load_local_http_record(record_path)?;
    let capability = record.capability()?.to_base64url();
    atm_core::api::write_http_request_with_headers(
        writer,
        request,
        &[(
            atm_core::local_http::LOCAL_CAPABILITY_HEADER,
            capability.as_str(),
        )],
    )
}

pub(crate) fn load_local_http_record(
    record_path: &Path,
) -> Result<atm_core::local_http::LocalHttpEndpointRecord, AtmError> {
    let contents = fs::read(record_path).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!(
                "failed to read local HTTP endpoint record {}: {source}",
                record_path.display()
            ),
            source,
        )
    })?;
    let record: atm_core::local_http::LocalHttpEndpointRecord = serde_json::from_slice(&contents)
        .map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!(
                "failed to parse local HTTP endpoint record {}: {source}",
                record_path.display()
            ),
            source,
        )
    })?;
    record.capability()?;
    let owner_instance_id =
        atm_core::local_http::owner_instance_id_for_local_http_record(record_path)?;
    if record.daemon_instance_id != owner_instance_id {
        return Err(AtmError::daemon_unavailable(
            "local HTTP endpoint record belongs to a different daemon instance",
        ));
    }
    Ok(record)
}

pub(crate) fn set_stream_write_timeout(
    stream: &TcpStream,
    timeout: Option<Duration>,
) -> std::io::Result<()> {
    stream.set_write_timeout(timeout)
}

pub(crate) fn set_stream_read_timeout(
    stream: &TcpStream,
    timeout: Option<Duration>,
) -> std::io::Result<()> {
    stream.set_read_timeout(timeout)
}

pub(crate) fn read_http_response_with_deadline(
    mut stream: TcpStream,
    request: &RequestEnvelope,
    request_deadline: Duration,
    recv_deadline_support: LocalIpcDeadlineSupport,
) -> Result<ResponseEnvelope, AtmError> {
    if recv_deadline_support == LocalIpcDeadlineSupport::Unsupported {
        return read_http_response_with_helper(stream, request.clone(), request_deadline);
    }
    atm_core::api::read_http_response_with_frame_reader(
        &mut atm_core::api::HttpFrameReader::new(),
        &mut stream,
        request,
    )
}

fn read_http_response_with_helper(
    stream: TcpStream,
    request: RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let mut reader_stream = stream.try_clone().map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            "failed to clone daemon HTTP response stream for deadline enforcement",
            source,
        )
    })?;
    let helper = thread::Builder::new()
        .name("local-ipc-http-response-read-helper".to_string())
        .spawn(move || {
            let result = atm_core::api::read_http_response_with_frame_reader(
                &mut atm_core::api::HttpFrameReader::new(),
                &mut reader_stream,
                &request,
            );
            if result_tx.send(result).is_err() {
                tracing::debug!("daemon HTTP response reader timed out before helper completion");
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to spawn daemon HTTP response read helper",
                source,
            )
        })?;
    match result_rx.recv_timeout(request_deadline) {
        Ok(result) => {
            join_response_read_helper(helper)?;
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancel_response_read_helper(&stream, helper)?;
            Err(AtmError::daemon_unavailable(
                "timed out reading daemon HTTP response",
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            join_response_read_helper(helper)?;
            Err(AtmError::daemon_unavailable(
                "daemon HTTP response read helper disconnected unexpectedly",
            ))
        }
    }
}

fn cancel_response_read_helper(
    stream: &TcpStream,
    helper: thread::JoinHandle<()>,
) -> Result<(), AtmError> {
    let shutdown = stream.shutdown(Shutdown::Both);
    let joined = join_response_read_helper(helper);
    if let Err(source) = shutdown {
        return Err(AtmError::daemon_unavailable_with_cause(
            "failed to cancel timed-out daemon HTTP response read",
            source,
        ));
    }
    joined
}

fn join_response_read_helper(helper: thread::JoinHandle<()>) -> Result<(), AtmError> {
    helper
        .join()
        .map_err(|_panic| AtmError::daemon_unavailable("daemon HTTP response read helper panicked"))
}

pub(crate) fn apply_local_ipc_deadline(
    result: std::io::Result<()>,
    message: &'static str,
) -> Result<LocalIpcDeadlineSupport, AtmError> {
    match result {
        Ok(()) => Ok(LocalIpcDeadlineSupport::Applied),
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => {
            Ok(LocalIpcDeadlineSupport::Unsupported)
        }
        Err(source) => Err(AtmError::daemon_unavailable_with_cause(message, source)),
    }
}
