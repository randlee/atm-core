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
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    Heartbeat(TeamMemberHeartbeatResponse),
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
        AtmErrorCode::IdentityConflict => AtmErrorKind::Identity,
        AtmErrorCode::DaemonUnavailable
        | AtmErrorCode::DaemonLaunchGateRejected
        | AtmErrorCode::DaemonServingStateRejected
        | AtmErrorCode::DaemonStaleOwnerRecoveryFailed
        | AtmErrorCode::DaemonAutoStartFailed
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
        | AtmErrorCode::WarningHookExecutionFailed
        | AtmErrorCode::TestFakeTransportInjectionFailed => AtmErrorKind::Validation,
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
    pub sqlite_ready: bool,
    #[serde(default)]
    pub degraded_ingest: bool,
    #[serde(default)]
    pub member_counts: RuntimeStatusCounts,
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

#[cfg(test)]
mod tests {
    use super::{
        HeartbeatActivity, RequestEnvelope, ResponseEnvelope, RuntimeLivenessState,
        RuntimeMemberState, RuntimeReadinessState, RuntimeStatusCounts, RuntimeStatusSnapshot,
        TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    };
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
            sqlite_ready: true,
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
}
