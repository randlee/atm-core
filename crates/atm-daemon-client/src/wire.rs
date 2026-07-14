use std::fmt;
use std::io::{Read, Write};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use atm_storage::AtmError;
use interprocess::local_socket::{GenericFilePath, Name, ToFsName};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePayload {
    pub request_id: RequestId,
    pub message_kind: MessageKind,
    pub flags: u16,
    pub bytes: Vec<u8>,
}

pub const MAX_DAEMON_FRAME_BYTES: usize = 1024 * 1024;
pub const ATM_FRAME_MAGIC: u32 = u32::from_be_bytes(*b"ATMD");
pub const ATM_FRAME_VERSION_V1: u16 = 1;
pub const ATM_FRAME_FLAGS_V1: u16 = 0;
pub const ATM_FRAME_HEADER_BYTES: usize = 22;

#[cfg(test)]
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    pub fn new(request_id: u64) -> Result<Self, AtmError> {
        let request_id = NonZeroU64::new(request_id).ok_or_else(|| {
            AtmError::validation("ATM daemon protocol request_id must be non-zero").with_recovery(
                "Retry with a client and daemon build that populate non-zero ATM daemon request ids.",
            )
        })?;
        Ok(Self(request_id))
    }

    pub const fn into_inner(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageKind {
    SendComposeRequest = 0x0001,
    SendAcknowledgeRequest = 0x0002,
    HeartbeatRequest = 0x0003,
    CompatibilityPreflightRequest = 0x0009,
    ListRequest = 0x0004,
    PeekRequest = 0x0005,
    ReceiveRequest = 0x0006,
    ClearRequest = 0x0007,
    DoctorRequest = 0x0008,
    SendSentResponse = 0x1001,
    SendAcknowledgedResponse = 0x1002,
    HeartbeatResponse = 0x1003,
    CompatibilityVerdictResponse = 0x1009,
    ListResponse = 0x1004,
    PeekResponse = 0x1005,
    ReceiveResponse = 0x1006,
    ClearResponse = 0x1007,
    DoctorResponse = 0x1008,
    ErrorResponse = 0x1fff,
}

impl MessageKind {
    pub const fn code(self) -> u16 {
        self as u16
    }

    pub const fn is_request(self) -> bool {
        matches!(
            self,
            Self::SendComposeRequest
                | Self::SendAcknowledgeRequest
                | Self::HeartbeatRequest
                | Self::CompatibilityPreflightRequest
                | Self::ListRequest
                | Self::PeekRequest
                | Self::ReceiveRequest
                | Self::ClearRequest
                | Self::DoctorRequest
        )
    }

    pub const fn is_response(self) -> bool {
        !self.is_request()
    }
}

impl TryFrom<u16> for MessageKind {
    type Error = AtmError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0x0001 => Ok(Self::SendComposeRequest),
            0x0002 => Ok(Self::SendAcknowledgeRequest),
            0x0003 => Ok(Self::HeartbeatRequest),
            0x0009 => Ok(Self::CompatibilityPreflightRequest),
            0x0004 => Ok(Self::ListRequest),
            0x0005 => Ok(Self::PeekRequest),
            0x0006 => Ok(Self::ReceiveRequest),
            0x0007 => Ok(Self::ClearRequest),
            0x0008 => Ok(Self::DoctorRequest),
            0x1001 => Ok(Self::SendSentResponse),
            0x1002 => Ok(Self::SendAcknowledgedResponse),
            0x1003 => Ok(Self::HeartbeatResponse),
            0x1009 => Ok(Self::CompatibilityVerdictResponse),
            0x1004 => Ok(Self::ListResponse),
            0x1005 => Ok(Self::PeekResponse),
            0x1006 => Ok(Self::ReceiveResponse),
            0x1007 => Ok(Self::ClearResponse),
            0x1008 => Ok(Self::DoctorResponse),
            0x1fff => Ok(Self::ErrorResponse),
            other => Err(AtmError::validation(format!(
                "unsupported ATM daemon message kind 0x{other:04x}"
            ))
            .with_recovery(
                "Align the CLI and daemon builds so both sides use the same local IPC message-kind contract before retrying.",
            )),
        }
    }
}

#[cfg(test)]
pub fn next_request_id() -> RequestId {
    loop {
        let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(request_id) = NonZeroU64::new(request_id) {
            return RequestId(request_id);
        }
    }
}

pub fn write_frame(
    writer: &mut impl Write,
    frame: &FramePayload,
    write_error: &'static str,
) -> Result<(), AtmError> {
    if frame.flags != ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame flags 0x{:04x} for version {}",
            frame.flags, ATM_FRAME_VERSION_V1
        ))
        .with_recovery(
            "Retry with a supported ATM daemon client/server build that uses protocol version 1 flags.",
        ));
    }
    if frame.bytes.len() > MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "daemon frame exceeded the maximum supported size",
        )
        .with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }
    let mut header = [0u8; ATM_FRAME_HEADER_BYTES];
    header[0..4].copy_from_slice(&ATM_FRAME_MAGIC.to_be_bytes());
    header[4..6].copy_from_slice(&ATM_FRAME_VERSION_V1.to_be_bytes());
    header[6..8].copy_from_slice(&frame.message_kind.code().to_be_bytes());
    header[8..10].copy_from_slice(&frame.flags.to_be_bytes());
    header[10..18].copy_from_slice(&frame.request_id.into_inner().to_be_bytes());
    header[18..22].copy_from_slice(&(frame.bytes.len() as u32).to_be_bytes());
    writer
        .write_all(&header)
        .and_then(|_| writer.write_all(&frame.bytes))
        .map_err(|source| AtmError::daemon_unavailable(write_error).with_source(source))
}

pub fn read_frame(
    reader: &mut impl Read,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<Option<FramePayload>, AtmError> {
    let Some(header) = read_frame_header(reader, read_error)? else {
        return Ok(None);
    };

    let magic = u32::from_be_bytes(header[0..4].try_into().expect("magic"));
    if magic != ATM_FRAME_MAGIC {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame magic 0x{magic:08x}"
        ))
        .with_recovery(
            "Retry with an ATM client and daemon build that both speak the documented ATM daemon protocol.",
        ));
    }

    let version = u16::from_be_bytes(header[4..6].try_into().expect("version"));
    if version != ATM_FRAME_VERSION_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame version {version}"
        ))
        .with_recovery(
            "Align the CLI and daemon builds so both sides use the same ATM daemon protocol version before retrying.",
        ));
    }

    let message_kind =
        MessageKind::try_from(u16::from_be_bytes(header[6..8].try_into().expect("kind")))?;
    let flags = u16::from_be_bytes(header[8..10].try_into().expect("flags"));
    if flags != ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported ATM daemon frame flags 0x{flags:04x} for version {version}"
        ))
        .with_recovery(
            "Retry with a supported ATM daemon client/server build that uses the version-1 flag contract.",
        ));
    }
    let request_id = RequestId::new(u64::from_be_bytes(
        header[10..18].try_into().expect("request id"),
    ))?;
    let payload_length = u32::from_be_bytes(header[18..22].try_into().expect("payload")) as usize;
    if payload_length > MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
            "Reduce the daemon request/response payload size before retrying the ATM command.",
        ));
    }

    let mut bytes = vec![0u8; payload_length];
    reader
        .read_exact(&mut bytes)
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
    Ok(Some(FramePayload {
        request_id,
        message_kind,
        flags,
        bytes,
    }))
}

fn read_frame_header(
    reader: &mut impl Read,
    read_error: &'static str,
) -> Result<Option<[u8; ATM_FRAME_HEADER_BYTES]>, AtmError> {
    let mut header = [0u8; ATM_FRAME_HEADER_BYTES];
    match reader.read_exact(&mut header) {
        Ok(()) => Ok(Some(header)),
        Err(source) if source.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(source) => Err(AtmError::daemon_unavailable(read_error).with_source(source)),
    }
}

pub fn daemon_local_ipc_name_from_path(endpoint_path: &Path) -> Result<Name<'static>, AtmError> {
    let normalized = platform_local_ipc_endpoint_path(endpoint_path.to_path_buf());
    normalized
        .into_os_string()
        .to_fs_name::<GenericFilePath>()
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to map daemon local IPC endpoint {} to a supported platform-local IPC name",
                endpoint_path.display()
            ))
            .with_source(source)
            .with_recovery(
                "Set ATM_DAEMON_SOCKET to a valid daemon local IPC endpoint and retry the ATM command.",
            )
        })
}

#[cfg(windows)]
fn platform_local_ipc_endpoint_path(path: PathBuf) -> PathBuf {
    const WINDOWS_PIPE_PREFIX: &str = r"\\.\pipe\";

    let raw = path.to_string_lossy();
    if raw.starts_with(WINDOWS_PIPE_PREFIX) {
        return path;
    }

    let mut hash = 0xcbf29ce484222325u64;
    for byte in raw.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    let mut leaf = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "atm-daemon".to_string());
    leaf.retain(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if leaf.is_empty() {
        leaf = "atm-daemon".to_string();
    }

    PathBuf::from(format!(r"\\.\pipe\atm-{}-{hash:016x}", leaf))
}

#[cfg(not(windows))]
fn platform_local_ipc_endpoint_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
pub(crate) use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};

#[cfg(test)]
pub(crate) fn request_to_frame_payload(
    request_id: RequestId,
    request: RequestEnvelope,
) -> Result<FramePayload, AtmError> {
    Ok(FramePayload {
        request_id,
        message_kind: message_kind_for_request(&request),
        flags: ATM_FRAME_FLAGS_V1,
        bytes: serde_json::to_vec(&request).map_err(AtmError::from)?,
    })
}

#[cfg(test)]
pub(crate) fn request_from_frame_payload(
    frame: FramePayload,
) -> Result<(RequestId, RequestEnvelope), AtmError> {
    if !frame.message_kind.is_request() {
        return Err(AtmError::validation(format!(
            "ATM daemon request decoder received non-request message kind 0x{:04x}",
            frame.message_kind.code()
        ))
        .with_recovery(
            "Align the CLI and daemon builds so both sides agree on request and response packet roles before retrying.",
        ));
    }
    let request = serde_json::from_slice(&frame.bytes).map_err(AtmError::from)?;
    Ok((frame.request_id, request))
}

#[cfg(test)]
pub(crate) fn response_to_frame_payload(
    request_id: RequestId,
    response: ResponseEnvelope,
) -> Result<FramePayload, AtmError> {
    Ok(FramePayload {
        request_id,
        message_kind: message_kind_for_response(&response),
        flags: ATM_FRAME_FLAGS_V1,
        bytes: serde_json::to_vec(&response).map_err(AtmError::from)?,
    })
}

#[cfg(test)]
pub(crate) fn response_from_frame_payload(
    frame: FramePayload,
) -> Result<(RequestId, ResponseEnvelope), AtmError> {
    if !frame.message_kind.is_response() {
        return Err(AtmError::validation(format!(
            "ATM daemon response decoder received non-response message kind 0x{:04x}",
            frame.message_kind.code()
        ))
        .with_recovery(
            "Align the CLI and daemon builds so both sides agree on request and response packet roles before retrying.",
        ));
    }
    let response = serde_json::from_slice(&frame.bytes).map_err(AtmError::from)?;
    Ok((frame.request_id, response))
}

#[cfg(test)]
fn message_kind_for_request(request: &RequestEnvelope) -> MessageKind {
    use atm_core::protocol::RequestEnvelope::*;
    match request {
        Send(atm_core::protocol::SendRequestEnvelope::Compose(_)) => {
            MessageKind::SendComposeRequest
        }
        Send(atm_core::protocol::SendRequestEnvelope::Acknowledge(_)) => {
            MessageKind::SendAcknowledgeRequest
        }
        CompatibilityPreflight(_) => MessageKind::CompatibilityPreflightRequest,
        Heartbeat(_) => MessageKind::HeartbeatRequest,
        List(_) => MessageKind::ListRequest,
        Peek(_) => MessageKind::PeekRequest,
        Receive(_) => MessageKind::ReceiveRequest,
        Clear(_) => MessageKind::ClearRequest,
        Doctor(_) => MessageKind::DoctorRequest,
    }
}

#[cfg(test)]
fn message_kind_for_response(response: &ResponseEnvelope) -> MessageKind {
    use atm_core::protocol::ResponseEnvelope::*;
    match response {
        Send(atm_core::protocol::SendResponseEnvelope::Sent(_)) => MessageKind::SendSentResponse,
        Send(atm_core::protocol::SendResponseEnvelope::Acknowledged(_)) => {
            MessageKind::SendAcknowledgedResponse
        }
        CompatibilityVerdict(_) => MessageKind::CompatibilityVerdictResponse,
        Heartbeat(_) => MessageKind::HeartbeatResponse,
        List(_) => MessageKind::ListResponse,
        Peek(_) => MessageKind::PeekResponse,
        Receive(_) => MessageKind::ReceiveResponse,
        Clear(_) => MessageKind::ClearResponse,
        Doctor(_) => MessageKind::DoctorResponse,
        Error(_) => MessageKind::ErrorResponse,
    }
}
