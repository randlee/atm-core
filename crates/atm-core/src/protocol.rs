//! Shared protocol DTOs for the core transport boundary family.

use std::env;
use std::fmt;
use std::io::Read;
use std::io::Write;
use std::num::NonZeroU64;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use interprocess::local_socket::{GenericFilePath, Name, ToFsName};
use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::clear::{ClearOutcome, ClearQuery};
use crate::doctor::{DoctorQuery, DoctorReport};
use crate::error::{AtmError, AtmErrorKind};
use crate::error_codes::AtmErrorCode;
use crate::graft::{
    AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryFetchRequest, AdvisoryFetchResponse,
    AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse,
    AdvisorySessionUnregistrationRequest, AdvisorySessionUnregistrationResponse,
    AdvisoryStreamRequest, AdvisoryStreamResponse,
};
use crate::home;
use crate::list::{ListOutcome, ListQuery};
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, IsoTimestamp, TeamName};

/// Shared protocol send-shaped request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendRequestEnvelope {
    Compose(SendRequest),
    Acknowledge(AckRequest),
}

/// Shared protocol send-shaped response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendResponseEnvelope {
    Sent(SendOutcome),
    Acknowledged(AckOutcome),
}

/// Shared protocol request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestEnvelope {
    Send(SendRequestEnvelope),
    Heartbeat(TeamMemberHeartbeatRequest),
    List(ListQuery),
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
    AdvisoryRegister(AdvisorySessionRegistrationRequest),
    AdvisoryUnregister(AdvisorySessionUnregistrationRequest),
    AdvisoryFetch(AdvisoryFetchRequest),
    AdvisoryDrain(AdvisoryDrainRequest),
    AdvisoryStream(AdvisoryStreamRequest),
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    Heartbeat(TeamMemberHeartbeatResponse),
    List(ListOutcome),
    Receive(Box<ReadOutcome>),
    Clear(ClearOutcome),
    Doctor(Box<DoctorReport>),
    AdvisoryRegister(AdvisorySessionRegistrationResponse),
    AdvisoryUnregister(AdvisorySessionUnregistrationResponse),
    AdvisoryFetch(AdvisoryFetchResponse),
    AdvisoryDrain(AdvisoryDrainResponse),
    AdvisoryStream(AdvisoryStreamResponse),
    Error(ProtocolErrorEnvelope),
}

/// Serialized daemon-side ATM error for protocol transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolErrorEnvelope {
    pub code: AtmErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovery: Vec<String>,
}

impl ProtocolErrorEnvelope {
    pub fn from_error(error: &AtmError) -> Self {
        Self {
            code: error.code,
            message: error.message.clone(),
            recovery: error.recovery.clone(),
        }
    }

    pub fn into_atm_error(self) -> AtmError {
        let mut error =
            AtmError::new_with_code(self.code, error_kind_for_code(self.code), self.message);
        for recovery in self.recovery {
            error = error.with_recovery(recovery);
        }
        error
    }
}

const fn error_kind_for_code(code: AtmErrorCode) -> AtmErrorKind {
    match code {
        AtmErrorCode::ConfigHomeUnavailable
        | AtmErrorCode::ConfigParseFailed
        | AtmErrorCode::ConfigRetiredHookMembersKey
        | AtmErrorCode::ConfigRetiredLegacyHookKeys
        | AtmErrorCode::ConfigTeamParseFailed
        | AtmErrorCode::ConfigTeamMissing => AtmErrorKind::Config,
        AtmErrorCode::IdentityUnavailable
        | AtmErrorCode::IdentityInvalid
        | AtmErrorCode::WarningIdentityDrift => AtmErrorKind::Identity,
        AtmErrorCode::IdentityConflict => AtmErrorKind::Identity,
        AtmErrorCode::DaemonUnavailable
        | AtmErrorCode::DaemonMayHaveExecuted
        | AtmErrorCode::DaemonLifecycleWedge
        | AtmErrorCode::DaemonLaunchGateRejected
        | AtmErrorCode::DaemonServingStateRejected
        | AtmErrorCode::DaemonStaleOwnerRecoveryFailed
        | AtmErrorCode::DaemonAutoStartFailed
        | AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered
        | AtmErrorCode::DaemonAdvisorySessionNotRegistered
        | AtmErrorCode::RemoteDeliveryOutcomeUnknown => AtmErrorKind::DaemonUnavailable,
        AtmErrorCode::AddressParseFailed => AtmErrorKind::Address,
        AtmErrorCode::TeamUnavailable | AtmErrorCode::TeamNotFound => AtmErrorKind::TeamNotFound,
        AtmErrorCode::AgentNotFound => AtmErrorKind::AgentNotFound,
        AtmErrorCode::MailboxReadFailed | AtmErrorCode::WarningMailboxRecordSkipped => {
            AtmErrorKind::MailboxRead
        }
        AtmErrorCode::MailboxWriteFailed => AtmErrorKind::MailboxWrite,
        AtmErrorCode::MailboxLockFailed
        | AtmErrorCode::MailboxLockReadOnlyFilesystem
        | AtmErrorCode::MailboxLockTimeout
        | AtmErrorCode::WarningStaleMailboxLock => AtmErrorKind::MailboxLock,
        AtmErrorCode::FilePolicyRejected | AtmErrorCode::FileReferenceRewriteFailed => {
            AtmErrorKind::FilePolicy
        }
        AtmErrorCode::InternalError => AtmErrorKind::Internal,
        AtmErrorCode::SerializationFailed => AtmErrorKind::Serialization,
        AtmErrorCode::WaitTimeout => AtmErrorKind::Timeout,
        AtmErrorCode::ObservabilityEmitFailed => AtmErrorKind::ObservabilityEmit,
        AtmErrorCode::ObservabilityQueryFailed => AtmErrorKind::ObservabilityQuery,
        AtmErrorCode::ObservabilityFollowFailed => AtmErrorKind::ObservabilityFollow,
        AtmErrorCode::ObservabilityHealthFailed
        | AtmErrorCode::ObservabilityHealthOk
        | AtmErrorCode::WarningObservabilityHealthDegraded => AtmErrorKind::ObservabilityHealth,
        AtmErrorCode::WarningSqliteHealthDegraded
        | AtmErrorCode::PostSendAdvisoryDeliveryFailed => AtmErrorKind::DaemonUnavailable,
        AtmErrorCode::ObservabilityBootstrapFailed => AtmErrorKind::ObservabilityBootstrap,
        AtmErrorCode::MessageValidationFailed
        | AtmErrorCode::MailboxRecoveredMessageSetTooLarge
        | AtmErrorCode::HelpTopicNotFound
        | AtmErrorCode::AckInvalidState
        | AtmErrorCode::ClearInvalidState
        | AtmErrorCode::WarningInvalidTeamMemberSkipped
        | AtmErrorCode::WarningMalformedAtmFieldIgnored
        | AtmErrorCode::WarningOriginInboxEntrySkipped
        | AtmErrorCode::WarningMissingTeamConfigFallback
        | AtmErrorCode::WarningSendAlertStateDegraded
        | AtmErrorCode::WarningRosterDrift
        | AtmErrorCode::WarningBaselineMemberMissing
        | AtmErrorCode::WarningRestoreInProgress
        | AtmErrorCode::WarningHookSkipped
        | AtmErrorCode::WarningHookExecutionFailed
        | AtmErrorCode::PostSendPaneMissing
        | AtmErrorCode::PostSendTmuxSendFailed
        | AtmErrorCode::PostSendGraftUnavailable
        | AtmErrorCode::TestFakeTransportInjectionFailed
        | AtmErrorCode::TeamInvalid
        | AtmErrorCode::CallerContextRequestInvalid => AtmErrorKind::Validation,
    }
}

/// Raw protocol frame payload plus the shared ATM frame header fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePayload {
    pub request_id: RequestId,
    pub message_kind: MessageKind,
    pub flags: u16,
    pub bytes: Vec<u8>,
}

/// Maximum encoded daemon request/response frame size.
pub const MAX_DAEMON_FRAME_BYTES: usize = 1024 * 1024;
pub const ATM_FRAME_MAGIC: u32 = u32::from_be_bytes(*b"ATMD");
pub const ATM_FRAME_VERSION_V1: u16 = 1;
pub const ATM_FRAME_FLAGS_V1: u16 = 0;
pub const ATM_FRAME_HEADER_BYTES: usize = 22;

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    pub fn new(request_id: u64) -> Result<Self, AtmError> {
        let request_id = NonZeroU64::new(request_id).ok_or_else(|| {
            AtmError::validation(
                "ATM daemon protocol request_id must be non-zero",
            )
            .with_recovery(
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
    ListRequest = 0x0004,
    ReceiveRequest = 0x0005,
    ClearRequest = 0x0006,
    DoctorRequest = 0x0007,
    AdvisoryRegisterRequest = 0x0008,
    AdvisoryUnregisterRequest = 0x0009,
    AdvisoryFetchRequest = 0x000a,
    AdvisoryDrainRequest = 0x000b,
    AdvisoryStreamRequest = 0x000c,
    SendSentResponse = 0x1001,
    SendAcknowledgedResponse = 0x1002,
    HeartbeatResponse = 0x1003,
    ListResponse = 0x1004,
    ReceiveResponse = 0x1005,
    ClearResponse = 0x1006,
    DoctorResponse = 0x1007,
    AdvisoryRegisterResponse = 0x1008,
    AdvisoryUnregisterResponse = 0x1009,
    AdvisoryFetchResponse = 0x100a,
    AdvisoryDrainResponse = 0x100b,
    AdvisoryStreamResponse = 0x100c,
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
                | Self::ListRequest
                | Self::ReceiveRequest
                | Self::ClearRequest
                | Self::DoctorRequest
                | Self::AdvisoryRegisterRequest
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
        let kind = match value {
            0x0001 => Self::SendComposeRequest,
            0x0002 => Self::SendAcknowledgeRequest,
            0x0003 => Self::HeartbeatRequest,
            0x0004 => Self::ListRequest,
            0x0005 => Self::ReceiveRequest,
            0x0006 => Self::ClearRequest,
            0x0007 => Self::DoctorRequest,
            0x0008 => Self::AdvisoryRegisterRequest,
            0x0009 => Self::AdvisoryUnregisterRequest,
            0x000a => Self::AdvisoryFetchRequest,
            0x000b => Self::AdvisoryDrainRequest,
            0x000c => Self::AdvisoryStreamRequest,
            0x1001 => Self::SendSentResponse,
            0x1002 => Self::SendAcknowledgedResponse,
            0x1003 => Self::HeartbeatResponse,
            0x1004 => Self::ListResponse,
            0x1005 => Self::ReceiveResponse,
            0x1006 => Self::ClearResponse,
            0x1007 => Self::DoctorResponse,
            0x1008 => Self::AdvisoryRegisterResponse,
            0x1009 => Self::AdvisoryUnregisterResponse,
            0x100a => Self::AdvisoryFetchResponse,
            0x100b => Self::AdvisoryDrainResponse,
            0x100c => Self::AdvisoryStreamResponse,
            0x1fff => Self::ErrorResponse,
            _ => {
                return Err(AtmError::validation(format!(
                    "unsupported ATM daemon frame message kind 0x{value:04x}"
                ))
                .with_recovery(
                    "Align the CLI and daemon builds so both sides speak the same ATM daemon protocol version before retrying.",
                ));
            }
        };
        Ok(kind)
    }
}

#[derive(Debug, Clone, Default)]
pub struct JsonAtmProtocolCodec;

impl crate::boundary::sealed::Sealed for JsonAtmProtocolCodec {}

impl crate::boundary::AtmProtocol for JsonAtmProtocolCodec {
    fn request_to_frame(
        &self,
        request_id: RequestId,
        request: RequestEnvelope,
    ) -> Result<FramePayload, AtmError> {
        request_to_frame_payload(request_id, request)
    }

    fn request_from_frame(
        &self,
        frame: FramePayload,
    ) -> Result<(RequestId, RequestEnvelope), AtmError> {
        request_from_frame_payload(frame)
    }

    fn response_to_frame(
        &self,
        request_id: RequestId,
        response: ResponseEnvelope,
    ) -> Result<FramePayload, AtmError> {
        response_to_frame_payload(request_id, response)
    }

    fn response_from_frame(
        &self,
        frame: FramePayload,
    ) -> Result<(RequestId, ResponseEnvelope), AtmError> {
        response_from_frame_payload(frame)
    }
}

pub fn next_request_id() -> RequestId {
    loop {
        let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(request_id) = NonZeroU64::new(request_id) {
            return RequestId(request_id);
        }
    }
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

pub fn request_from_frame_payload(
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
    let request = match frame.message_kind {
        MessageKind::SendComposeRequest
        | MessageKind::SendAcknowledgeRequest
        | MessageKind::ListRequest
        | MessageKind::ReceiveRequest
        | MessageKind::ClearRequest => {
            let value = serde_json::from_slice::<serde_json::Value>(&frame.bytes)
                .map_err(AtmError::from)?;
            validate_required_caller_context_fields(&value)?;
            serde_json::from_value(value).map_err(AtmError::from)?
        }
        _ => serde_json::from_slice(&frame.bytes).map_err(AtmError::from)?,
    };
    Ok((frame.request_id, request))
}

fn validate_required_caller_context_fields(value: &serde_json::Value) -> Result<(), AtmError> {
    let object = value.as_object().ok_or_else(|| {
        AtmError::caller_context_request_invalid(
            "daemon request payload must be a JSON object with caller_identity and caller_team",
        )
    })?;
    parse_required_caller_identity(object)?;
    parse_required_caller_team(object)?;
    Ok(())
}

fn parse_required_caller_identity(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<AgentName, AtmError> {
    let value = object.get("caller_identity").ok_or_else(|| {
        AtmError::caller_context_request_invalid("daemon request is missing caller_identity")
    })?;
    let raw = value.as_str().ok_or_else(|| {
        AtmError::caller_context_request_invalid("daemon request caller_identity must be a string")
    })?;
    raw.parse::<AgentName>().map_err(|error| {
        AtmError::caller_context_request_invalid(format!(
            "daemon request caller_identity is invalid: {}",
            error.message
        ))
    })
}

fn parse_required_caller_team(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<TeamName, AtmError> {
    let value = object.get("caller_team").ok_or_else(|| {
        AtmError::caller_context_request_invalid("daemon request is missing caller_team")
    })?;
    let raw = value.as_str().ok_or_else(|| {
        AtmError::caller_context_request_invalid("daemon request caller_team must be a string")
    })?;
    raw.parse::<TeamName>().map_err(|error| {
        AtmError::caller_context_request_invalid(format!(
            "daemon request caller_team is invalid: {}",
            error.message
        ))
    })
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

pub fn response_from_frame_payload(
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
        RequestEnvelope::Send(SendRequestEnvelope::Compose(_)) => MessageKind::SendComposeRequest,
        RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(_)) => {
            MessageKind::SendAcknowledgeRequest
        }
        RequestEnvelope::Heartbeat(_) => MessageKind::HeartbeatRequest,
        RequestEnvelope::List(_) => MessageKind::ListRequest,
        RequestEnvelope::Receive(_) => MessageKind::ReceiveRequest,
        RequestEnvelope::Clear(_) => MessageKind::ClearRequest,
        RequestEnvelope::Doctor(_) => MessageKind::DoctorRequest,
        RequestEnvelope::AdvisoryRegister(_) => MessageKind::AdvisoryRegisterRequest,
        RequestEnvelope::AdvisoryUnregister(_) => MessageKind::AdvisoryUnregisterRequest,
        RequestEnvelope::AdvisoryFetch(_) => MessageKind::AdvisoryFetchRequest,
        RequestEnvelope::AdvisoryDrain(_) => MessageKind::AdvisoryDrainRequest,
        RequestEnvelope::AdvisoryStream(_) => MessageKind::AdvisoryStreamRequest,
    }
}

fn response_message_kind(response: &ResponseEnvelope) -> MessageKind {
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_)) => MessageKind::SendSentResponse,
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(_)) => {
            MessageKind::SendAcknowledgedResponse
        }
        ResponseEnvelope::Heartbeat(_) => MessageKind::HeartbeatResponse,
        ResponseEnvelope::List(_) => MessageKind::ListResponse,
        ResponseEnvelope::Receive(_) => MessageKind::ReceiveResponse,
        ResponseEnvelope::Clear(_) => MessageKind::ClearResponse,
        ResponseEnvelope::Doctor(_) => MessageKind::DoctorResponse,
        ResponseEnvelope::AdvisoryRegister(_) => MessageKind::AdvisoryRegisterResponse,
        ResponseEnvelope::AdvisoryUnregister(_) => MessageKind::AdvisoryUnregisterResponse,
        ResponseEnvelope::AdvisoryFetch(_) => MessageKind::AdvisoryFetchResponse,
        ResponseEnvelope::AdvisoryDrain(_) => MessageKind::AdvisoryDrainResponse,
        ResponseEnvelope::AdvisoryStream(_) => MessageKind::AdvisoryStreamResponse,
        ResponseEnvelope::Error(_) => MessageKind::ErrorResponse,
    }
}

/// Read one daemon frame into memory while enforcing the shared size cap.
///
/// # Errors
///
/// Returns [`AtmError`] when the stream cannot be read or when the payload
/// exceeds [`MAX_DAEMON_FRAME_BYTES`].
pub fn read_bounded_stream(
    stream: &mut impl Read,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<Vec<u8>, AtmError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(read) > MAX_DAEMON_FRAME_BYTES {
            return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
                "Reduce the daemon request/response payload size before retrying the ATM command.",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// Resolve the active daemon socket path for the ATM request transport.
///
/// # Errors
///
/// Returns [`AtmError`] when the host-scoped ATM runtime root cannot be resolved.
pub fn daemon_socket_path() -> Result<PathBuf, AtmError> {
    if let Some(path) = env::var_os("ATM_DAEMON_SOCKET").filter(|value| !value.is_empty()) {
        return Ok(platform_local_ipc_endpoint_path(PathBuf::from(path)));
    }
    Ok(platform_local_ipc_endpoint_path(
        home::host_runtime_dir()?.join("atm-daemon.sock"),
    ))
}

/// Resolve the active local IPC name for the ATM request transport.
///
/// # Errors
///
/// Returns [`AtmError`] when the active endpoint cannot be mapped to a valid
/// platform-local IPC name.
pub fn daemon_local_ipc_name() -> Result<Name<'static>, AtmError> {
    daemon_local_ipc_name_from_path(&daemon_socket_path()?)
}

/// Convert one daemon endpoint path into the concrete platform-local IPC name
/// used by the same-host transport.
///
/// # Errors
///
/// Returns [`AtmError`] when the endpoint cannot be represented by the current
/// platform-local IPC implementation.
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

/// Shared notification event payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Delivery,
    #[deprecated(note = "Phase AD obsolete: historical reconcile/watch only")]
    ReconcileComplete,
}

impl fmt::Display for NotificationKind {
    #[allow(
        deprecated,
        reason = "Phase AD obsolete transport strings remain stable for historical reconcile/watch decoding and formatting support."
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Delivery => "delivery",
            Self::ReconcileComplete => "reconcile_complete",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationEvent {
    pub kind: NotificationKind,
    pub detail: String,
    pub team: Option<TeamName>,
    pub agent: Option<AgentName>,
}

/// Runtime heartbeat activity transported into the daemon status cache.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatActivity {
    ActiveToolUse,
    Idle,
    SessionEnded,
}

/// One daemon heartbeat request for one team member identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamMemberHeartbeatRequest {
    pub team: TeamName,
    pub member: AgentName,
    pub pid: u32,
    pub observed_at: IsoTimestamp,
    pub activity: HeartbeatActivity,
}

/// One daemon heartbeat response after runtime-state application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamMemberHeartbeatResponse {
    pub team: TeamName,
    pub member: AgentName,
    pub pid: u32,
    #[serde(default)]
    pub pid_changed: bool,
    pub state: RuntimeMemberState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<IsoTimestamp>,
}

/// Runtime-owned live-state projection for one known team member.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMemberState {
    Unknown,
    IdentityConflict,
    Offline,
    Idle,
    Active,
}

/// Process-level daemon liveness state used by doctor and status queries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLivenessState {
    Running,
    Unavailable,
}

/// Request-serving readiness state used by doctor and status queries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeReadinessState {
    Ready,
    Degraded,
    Unavailable,
}

/// Aggregate live-member counts carried in daemon runtime snapshots.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatusCounts {
    pub active_members: usize,
    pub idle_members: usize,
    pub offline_members: usize,
    pub unknown_members: usize,
}

/// Runtime status snapshot transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot {
    pub liveness: RuntimeLivenessState,
    pub readiness: RuntimeReadinessState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub singleton_owner_pid: Option<u32>,
    #[serde(default)]
    pub degraded_ingest: bool,
    #[serde(default)]
    pub member_counts: RuntimeStatusCounts,
}

/// Watch subscription request payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[deprecated(note = "Phase AD obsolete: historical reconcile/watch only")]
pub struct WatchSubscriptionRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Watch event batch transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[deprecated(note = "Phase AD obsolete: historical reconcile/watch only")]
pub struct WatchEventBatch {
    pub paths: Vec<PathBuf>,
}

/// Reconcile request transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[deprecated(note = "Phase AD obsolete: historical reconcile/watch only")]
pub struct ReconcileRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Reconcile outcome transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[deprecated(note = "Phase AD obsolete: historical reconcile/watch only")]
pub struct ReconcileResult {
    pub observed_paths: usize,
    pub imported_sources: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        HeartbeatActivity, ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope,
        RuntimeLivenessState, RuntimeMemberState, RuntimeReadinessState, RuntimeStatusCounts,
        RuntimeStatusSnapshot, TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    };
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    #[test]
    fn heartbeat_request_envelope_round_trips() {
        let observed_at = IsoTimestamp::now();
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: TeamName::from_validated("test-team"),
            member: AgentName::from_validated("test-agent"),
            pid: 4242,
            observed_at,
            activity: HeartbeatActivity::ActiveToolUse,
        });

        let encoded = serde_json::to_vec(&request).expect("encode heartbeat request");
        let decoded: RequestEnvelope =
            serde_json::from_slice(&encoded).expect("decode heartbeat request");

        match decoded {
            RequestEnvelope::Heartbeat(decoded) => {
                assert_eq!(decoded.team, TeamName::from_validated("test-team"));
                assert_eq!(decoded.member, AgentName::from_validated("test-agent"));
                assert_eq!(decoded.pid, 4242);
                assert_eq!(decoded.observed_at, observed_at);
                assert_eq!(decoded.activity, HeartbeatActivity::ActiveToolUse);
            }
            other => panic!("expected heartbeat request, got {other:?}"),
        }
    }

    #[test]
    fn heartbeat_response_envelope_round_trips() {
        let last_active_at = IsoTimestamp::now();
        let response = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
            team: TeamName::from_validated("test-team"),
            member: AgentName::from_validated("test-agent"),
            pid: 4242,
            pid_changed: true,
            state: RuntimeMemberState::Active,
            last_active_at: Some(last_active_at),
        });

        let encoded = serde_json::to_vec(&response).expect("encode heartbeat response");
        let decoded: ResponseEnvelope =
            serde_json::from_slice(&encoded).expect("decode heartbeat response");

        match decoded {
            ResponseEnvelope::Heartbeat(decoded) => {
                assert_eq!(decoded.team, TeamName::from_validated("test-team"));
                assert_eq!(decoded.member, AgentName::from_validated("test-agent"));
                assert_eq!(decoded.pid, 4242);
                assert!(decoded.pid_changed);
                assert_eq!(decoded.state, RuntimeMemberState::Active);
                assert_eq!(decoded.last_active_at, Some(last_active_at));
            }
            other => panic!("expected heartbeat response, got {other:?}"),
        }
    }

    #[test]
    fn runtime_status_snapshot_round_trips() {
        let snapshot = RuntimeStatusSnapshot {
            liveness: RuntimeLivenessState::Running,
            readiness: RuntimeReadinessState::Ready,
            detail: Some("runtime cache ready".to_string()),
            singleton_owner_pid: Some(777),
            degraded_ingest: false,
            member_counts: RuntimeStatusCounts {
                active_members: 2,
                idle_members: 1,
                offline_members: 1,
                unknown_members: 3,
            },
        };

        let encoded = serde_json::to_vec(&snapshot).expect("encode runtime snapshot");
        let decoded: RuntimeStatusSnapshot =
            serde_json::from_slice(&encoded).expect("decode runtime snapshot");

        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn protocol_error_envelope_preserves_remote_delivery_outcome_unknown_recovery() {
        let error = AtmError::remote_delivery_outcome_unknown(
            "remote peer delivery outcome is unknown and replay persistence failed",
        )
        .with_source(
            AtmError::daemon_unavailable("remote replay store is not configured").with_recovery(
                "Restore the host-scoped ATM durable replay store before retrying remote delivery so atm-daemon can resume unknown peer handoffs safely.",
            ),
        );
        let envelope = ProtocolErrorEnvelope::from_error(&error);
        let round_trip = envelope.into_atm_error();

        assert_eq!(round_trip.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
        assert_eq!(round_trip.message, error.message);
        assert_eq!(round_trip.recovery, error.recovery);
    }
}
