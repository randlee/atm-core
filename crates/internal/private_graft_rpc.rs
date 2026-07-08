#![allow(
    dead_code,
    reason = "AD.14 keeps this graft advisory/session wire model only as a private transitional implementation until AD.15 and AD.16 delete the remaining consumers"
)]
#![allow(
    clippy::enum_variant_names,
    reason = "The retained advisory packet family keeps its historical variant names only until AD.15 and AD.16 delete this private transitional module"
)]

//! Transitional private graft advisory/session wire model retained only until
//! AD.15 and AD.16 delete the daemon advisory runtime and the graft receiver
//! session protocol. This module is intentionally not exported by
//! `atm-daemon-client`.

use std::fmt;
use std::io::{Read, Write};
use std::num::NonZeroUsize;
use std::str::FromStr;

use atm_core::error::AtmError;
use atm_core::protocol::ProtocolErrorEnvelope;
use atm_core::schema::AtmMessageId;
use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize};

use crate::wire::{
    ATM_FRAME_FLAGS_V1, ATM_FRAME_HEADER_BYTES, ATM_FRAME_MAGIC, ATM_FRAME_VERSION_V1,
    MAX_DAEMON_FRAME_BYTES, RequestId,
};

pub const MAX_ADVISORY_SESSION_ID_BYTES: usize = 128;
pub const MAX_ADVISORY_BATCH_LIMIT: usize = 256;
pub const MAX_ADVISORY_MESSAGE_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageKind {
    AdvisoryRegisterRequest = 0x0008,
    AdvisoryUnregisterRequest = 0x0009,
    AdvisoryFetchRequest = 0x000a,
    AdvisoryDrainRequest = 0x000b,
    AdvisoryStreamRequest = 0x000c,
    AdvisoryRegisterResponse = 0x1008,
    AdvisoryUnregisterResponse = 0x1009,
    AdvisoryFetchResponse = 0x100a,
    AdvisoryDrainResponse = 0x100b,
    AdvisoryStreamResponse = 0x100c,
    ErrorResponse = 0x1ffe,
}

impl MessageKind {
    pub const fn code(self) -> u16 {
        self as u16
    }

    pub const fn is_request(self) -> bool {
        matches!(
            self,
            Self::AdvisoryRegisterRequest
                | Self::AdvisoryUnregisterRequest
                | Self::AdvisoryFetchRequest
                | Self::AdvisoryDrainRequest
                | Self::AdvisoryStreamRequest
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
            0x0008 => Ok(Self::AdvisoryRegisterRequest),
            0x0009 => Ok(Self::AdvisoryUnregisterRequest),
            0x000a => Ok(Self::AdvisoryFetchRequest),
            0x000b => Ok(Self::AdvisoryDrainRequest),
            0x000c => Ok(Self::AdvisoryStreamRequest),
            0x1008 => Ok(Self::AdvisoryRegisterResponse),
            0x1009 => Ok(Self::AdvisoryUnregisterResponse),
            0x100a => Ok(Self::AdvisoryFetchResponse),
            0x100b => Ok(Self::AdvisoryDrainResponse),
            0x100c => Ok(Self::AdvisoryStreamResponse),
            0x1ffe => Ok(Self::ErrorResponse),
            other => Err(AtmError::validation(format!(
                "unsupported graft local IPC message kind 0x{other:04x}"
            ))
            .with_recovery(
                "Align atm-graft and atm-daemon builds so both sides use the same graft-local IPC contract before retrying.",
            )),
        }
    }
}

pub const fn is_graft_message_kind_code(value: u16) -> bool {
    matches!(
        value,
        0x0008
            | 0x0009
            | 0x000a
            | 0x000b
            | 0x000c
            | 0x1008
            | 0x1009
            | 0x100a
            | 0x100b
            | 0x100c
            | 0x1ffe
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePayload {
    pub request_id: RequestId,
    pub message_kind: MessageKind,
    pub flags: u16,
    pub bytes: Vec<u8>,
}

#[allow(
    clippy::enum_variant_names,
    reason = "The retained advisory packet family keeps its historical variant names only until AD.15 and AD.16 delete this private transitional module"
)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RequestEnvelope {
    AdvisoryRegister(AdvisorySessionRegistrationRequest),
    AdvisoryUnregister(AdvisorySessionUnregistrationRequest),
    AdvisoryFetch(AdvisoryFetchRequest),
    AdvisoryDrain(AdvisoryDrainRequest),
    AdvisoryStream(AdvisoryStreamRequest),
}

#[allow(
    clippy::enum_variant_names,
    reason = "The retained advisory packet family keeps its historical variant names only until AD.15 and AD.16 delete this private transitional module"
)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResponseEnvelope {
    AdvisoryRegister(AdvisorySessionRegistrationResponse),
    AdvisoryUnregister(AdvisorySessionUnregistrationResponse),
    AdvisoryFetch(AdvisoryFetchResponse),
    AdvisoryDrain(AdvisoryDrainResponse),
    AdvisoryStream(AdvisoryStreamResponse),
    Error(ProtocolErrorEnvelope),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdvisorySessionState {
    Inactive,
    Connecting,
    Registered,
    Disconnected,
    Closed,
    CloseFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySession {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: AdvisorySessionId,
    pub state: AdvisorySessionState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AdvisorySessionId(String);

impl AdvisorySessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("advisory session id must not be blank").with_recovery(
                    "Populate a stable non-empty advisory session id before calling the graft-local IPC runtime.",
                ),
            );
        }
        if value.len() > MAX_ADVISORY_SESSION_ID_BYTES {
            return Err(AtmError::validation(format!(
                "advisory session id must be at most {MAX_ADVISORY_SESSION_ID_BYTES} bytes"
            ))
            .with_recovery(
                "Shorten the advisory session id before calling the graft-local IPC runtime.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AdvisorySessionId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for AdvisorySessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for AdvisorySessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AdvisoryBatchLimit(NonZeroUsize);

impl AdvisoryBatchLimit {
    pub fn new(value: usize) -> Result<Self, AtmError> {
        let value = NonZeroUsize::new(value).ok_or_else(|| {
            AtmError::validation("advisory batch limit must be greater than zero").with_recovery(
                "Use a positive advisory batch limit before calling the graft-local IPC queue surface.",
            )
        })?;
        if value.get() > MAX_ADVISORY_BATCH_LIMIT {
            return Err(AtmError::validation(format!(
                "advisory batch limit must not exceed {MAX_ADVISORY_BATCH_LIMIT}"
            ))
            .with_recovery(
                "Lower the advisory batch limit to the documented queue ceiling before calling the graft-local IPC queue surface.",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionRegistrationRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: AdvisorySessionId,
    pub pid: u32,
    pub started_at: IsoTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionRegistrationResponse {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: AdvisorySessionId,
    pub registered_at: IsoTimestamp,
    pub queue_capacity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryStreamRequest {
    pub registration: AdvisorySessionRegistrationRequest,
    pub limit: AdvisoryBatchLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryStreamResponse {
    pub session_id: AdvisorySessionId,
    pub nudges: Vec<AdvisoryEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionUnregistrationRequest {
    pub session_id: AdvisorySessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionUnregistrationResponse {
    pub session_id: AdvisorySessionId,
    pub closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryEvent {
    pub message_id: AtmMessageId,
    pub from: AgentName,
    pub message: AdvisoryMessage,
    pub received_at: IsoTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct AdvisoryMessage(String);

impl AdvisoryMessage {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.len() > MAX_ADVISORY_MESSAGE_BYTES {
            return Err(AtmError::validation(format!(
                "advisory message exceeds the {MAX_ADVISORY_MESSAGE_BYTES}-byte limit"
            ))
            .with_recovery(
                "Shorten the advisory payload before crossing the graft-local IPC boundary.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for AdvisoryMessage {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for AdvisoryMessage {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AdvisoryMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AdvisoryMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for AdvisoryMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdvisoryMessage")
            .field("bytes", &self.0.len())
            .field("redacted", &true)
            .finish()
    }
}

impl PartialEq<&str> for AdvisoryMessage {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryFetchRequest {
    pub session_id: AdvisorySessionId,
    pub limit: AdvisoryBatchLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryFetchResponse {
    pub session_id: AdvisorySessionId,
    pub nudges: Vec<AdvisoryEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryDrainRequest {
    pub session_id: AdvisorySessionId,
    pub limit: AdvisoryBatchLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryDrainResponse {
    pub session_id: AdvisorySessionId,
    pub nudges: Vec<AdvisoryEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

pub fn request_to_frame_payload(
    request_id: RequestId,
    request: RequestEnvelope,
) -> Result<FramePayload, AtmError> {
    Ok(FramePayload {
        request_id,
        message_kind: request_message_kind(&request),
        flags: ATM_FRAME_FLAGS_V1,
        bytes: serde_json::to_vec(&request).map_err(AtmError::from)?,
    })
}

pub fn request_from_raw_parts(
    request_id: RequestId,
    message_kind_code: u16,
    flags: u16,
    bytes: Vec<u8>,
) -> Result<(RequestId, RequestEnvelope), AtmError> {
    let message_kind = MessageKind::try_from(message_kind_code)?;
    if !message_kind.is_request() {
        return Err(AtmError::validation(format!(
            "graft request decoder received non-request message kind 0x{message_kind_code:04x}"
        ))
        .with_recovery(
            "Align atm-graft and atm-daemon builds so both sides agree on graft request and response packet roles before retrying.",
        ));
    }
    if flags != ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported graft local IPC frame flags 0x{flags:04x}"
        ))
        .with_recovery(
            "Align atm-graft and atm-daemon builds so both sides use the same version-1 graft-local IPC flag contract before retrying.",
        ));
    }
    let request = serde_json::from_slice(&bytes).map_err(AtmError::from)?;
    Ok((request_id, request))
}

pub fn response_to_frame_payload(
    request_id: RequestId,
    response: ResponseEnvelope,
) -> Result<FramePayload, AtmError> {
    Ok(FramePayload {
        request_id,
        message_kind: response_message_kind(&response),
        flags: ATM_FRAME_FLAGS_V1,
        bytes: serde_json::to_vec(&response).map_err(AtmError::from)?,
    })
}

pub fn response_from_raw_parts(
    request_id: RequestId,
    message_kind_code: u16,
    flags: u16,
    bytes: Vec<u8>,
) -> Result<(RequestId, ResponseEnvelope), AtmError> {
    let message_kind = MessageKind::try_from(message_kind_code)?;
    if !message_kind.is_response() {
        return Err(AtmError::validation(format!(
            "graft response decoder received non-response message kind 0x{message_kind_code:04x}"
        ))
        .with_recovery(
            "Align atm-graft and atm-daemon builds so both sides agree on graft request and response packet roles before retrying.",
        ));
    }
    if flags != ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported graft local IPC frame flags 0x{flags:04x}"
        ))
        .with_recovery(
            "Align atm-graft and atm-daemon builds so both sides use the same version-1 graft-local IPC flag contract before retrying.",
        ));
    }
    let response = serde_json::from_slice(&bytes).map_err(AtmError::from)?;
    Ok((request_id, response))
}

pub fn write_frame(
    writer: &mut impl Write,
    frame: &FramePayload,
    write_error: &'static str,
) -> Result<(), AtmError> {
    if frame.flags != ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported graft local IPC frame flags 0x{:04x} for version {}",
            frame.flags, ATM_FRAME_VERSION_V1
        ))
        .with_recovery(
            "Retry with a supported atm-graft and atm-daemon build that uses protocol version 1 flags.",
        ));
    }
    if frame.bytes.len() > MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "graft local IPC frame exceeded the maximum supported size",
        )
        .with_recovery(
            "Reduce the graft local IPC request/response payload size before retrying.",
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
            "unsupported graft local IPC frame magic 0x{magic:08x}"
        ))
        .with_recovery(
            "Retry with an atm-graft and atm-daemon build that both speak the documented graft-local IPC contract.",
        ));
    }

    let version = u16::from_be_bytes(header[4..6].try_into().expect("version"));
    if version != ATM_FRAME_VERSION_V1 {
        return Err(AtmError::validation(format!(
            "unsupported graft local IPC frame version {version}"
        ))
        .with_recovery(
            "Align atm-graft and atm-daemon builds so both sides use the same graft-local IPC version before retrying.",
        ));
    }

    let message_kind =
        MessageKind::try_from(u16::from_be_bytes(header[6..8].try_into().expect("kind")))?;
    let flags = u16::from_be_bytes(header[8..10].try_into().expect("flags"));
    if flags != ATM_FRAME_FLAGS_V1 {
        return Err(AtmError::validation(format!(
            "unsupported graft local IPC frame flags 0x{flags:04x} for version {version}"
        ))
        .with_recovery(
            "Retry with a supported atm-graft and atm-daemon build that uses the version-1 graft-local IPC flag contract.",
        ));
    }
    let request_id = RequestId::new(u64::from_be_bytes(
        header[10..18].try_into().expect("request id"),
    ))?;
    let payload_length = u32::from_be_bytes(header[18..22].try_into().expect("payload")) as usize;
    if payload_length > MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
            "Reduce the graft local IPC request/response payload size before retrying.",
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
    let read = reader
        .read(&mut header[..1])
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
    if read == 0 {
        return Ok(None);
    }
    reader
        .read_exact(&mut header[1..])
        .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
    Ok(Some(header))
}

fn request_message_kind(request: &RequestEnvelope) -> MessageKind {
    match request {
        RequestEnvelope::AdvisoryRegister(_) => MessageKind::AdvisoryRegisterRequest,
        RequestEnvelope::AdvisoryUnregister(_) => MessageKind::AdvisoryUnregisterRequest,
        RequestEnvelope::AdvisoryFetch(_) => MessageKind::AdvisoryFetchRequest,
        RequestEnvelope::AdvisoryDrain(_) => MessageKind::AdvisoryDrainRequest,
        RequestEnvelope::AdvisoryStream(_) => MessageKind::AdvisoryStreamRequest,
    }
}

fn response_message_kind(response: &ResponseEnvelope) -> MessageKind {
    match response {
        ResponseEnvelope::AdvisoryRegister(_) => MessageKind::AdvisoryRegisterResponse,
        ResponseEnvelope::AdvisoryUnregister(_) => MessageKind::AdvisoryUnregisterResponse,
        ResponseEnvelope::AdvisoryFetch(_) => MessageKind::AdvisoryFetchResponse,
        ResponseEnvelope::AdvisoryDrain(_) => MessageKind::AdvisoryDrainResponse,
        ResponseEnvelope::AdvisoryStream(_) => MessageKind::AdvisoryStreamResponse,
        ResponseEnvelope::Error(_) => MessageKind::ErrorResponse,
    }
}
