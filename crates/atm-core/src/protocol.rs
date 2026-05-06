//! Shared protocol DTOs for the core transport boundary family.

use std::env;
use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::clear::{ClearOutcome, ClearQuery};
use crate::doctor::{DoctorQuery, DoctorReport};
use crate::error::{AtmError, AtmErrorKind};
use crate::error_codes::AtmErrorCode;
use crate::home;
use crate::read::{ReadOutcome, ReadQuery};
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, TeamName};

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
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    Receive(ReadOutcome),
    Clear(ClearOutcome),
    Doctor(DoctorReport),
    Error(ProtocolErrorEnvelope),
}

/// Serialized daemon-side ATM error for protocol transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolErrorEnvelope {
    pub code: AtmErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
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
        let error =
            AtmError::new_with_code(self.code, error_kind_for_code(self.code), self.message);
        match self.recovery {
            Some(recovery) => error.with_recovery(recovery),
            None => error,
        }
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
        AtmErrorCode::IdentityUnavailable | AtmErrorCode::WarningIdentityDrift => {
            AtmErrorKind::Identity
        }
        AtmErrorCode::DaemonUnavailable => AtmErrorKind::DaemonUnavailable,
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
        AtmErrorCode::SerializationFailed => AtmErrorKind::Serialization,
        AtmErrorCode::WaitTimeout => AtmErrorKind::Timeout,
        AtmErrorCode::ObservabilityEmitFailed => AtmErrorKind::ObservabilityEmit,
        AtmErrorCode::ObservabilityQueryFailed => AtmErrorKind::ObservabilityQuery,
        AtmErrorCode::ObservabilityFollowFailed => AtmErrorKind::ObservabilityFollow,
        AtmErrorCode::ObservabilityHealthFailed
        | AtmErrorCode::ObservabilityHealthOk
        | AtmErrorCode::WarningObservabilityHealthDegraded => AtmErrorKind::ObservabilityHealth,
        AtmErrorCode::ObservabilityBootstrapFailed => AtmErrorKind::ObservabilityBootstrap,
        AtmErrorCode::MessageValidationFailed
        | AtmErrorCode::AckInvalidState
        | AtmErrorCode::ClearInvalidState
        | AtmErrorCode::WarningInvalidTeamMemberSkipped
        | AtmErrorCode::WarningMalformedAtmFieldIgnored
        | AtmErrorCode::WarningOriginInboxEntrySkipped
        | AtmErrorCode::WarningMissingTeamConfigFallback
        | AtmErrorCode::WarningSendAlertStateDegraded
        | AtmErrorCode::WarningBaselineMemberMissing
        | AtmErrorCode::WarningRestoreInProgress
        | AtmErrorCode::WarningHookSkipped
        | AtmErrorCode::WarningHookExecutionFailed => AtmErrorKind::Validation,
    }
}

/// Raw protocol frame payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FramePayload {
    pub bytes: Vec<u8>,
}

/// Maximum encoded daemon request/response frame size.
pub const MAX_DAEMON_FRAME_BYTES: usize = 1024 * 1024;

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
/// Returns [`AtmError`] when `ATM_HOME` cannot be resolved.
pub fn daemon_socket_path() -> Result<PathBuf, AtmError> {
    if let Some(path) = env::var_os("ATM_DAEMON_SOCKET").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    Ok(home::atm_home()?.join("atm-daemon.sock"))
}

/// Shared notification event payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationEvent {
    pub kind: String,
    pub detail: String,
    pub team: Option<TeamName>,
    pub agent: Option<AgentName>,
}

/// Runtime status snapshot transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot {
    pub status: String,
    pub detail: Option<String>,
}

/// Watch subscription request payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchSubscriptionRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Watch event batch transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchEventBatch {
    pub paths: Vec<PathBuf>,
}

/// Reconcile request transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Reconcile outcome transport payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileResult {
    pub observed_paths: usize,
    pub imported_sources: usize,
}
