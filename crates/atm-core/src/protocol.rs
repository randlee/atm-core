//! Shared protocol DTOs for the core transport boundary family.

use std::env;
use std::fmt;
use std::num::NonZeroU64;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use interprocess::local_socket::Name;
#[cfg(not(windows))]
use interprocess::local_socket::{GenericFilePath, ToFsName};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::ack::AckOutcome;
use crate::clear::{ClearOutcome, ClearQuery};
use crate::doctor::{DoctorQuery, DoctorReport};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::home;
use crate::list::{ListOutcome, ListQuery};
use crate::read::{PeekQuery, ReadOutcome, ReadQuery};
use crate::search::{SearchRequest, SearchResponse};
use crate::send::{SendOutcome, WriteRequest};
use crate::types::{AgentName, IsoTimestamp, SessionId, TeamName, deserialize_optional_session_id};

pub use atm_storage::{
    GraftReceiverLease, GraftReceiverRegistration, LocalCapability, OwnerGeneration,
};

/// Body representation for the local graft receiver lookup route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverLookupRequest {
    pub team: TeamName,
    pub agent: AgentName,
}

const DAEMON_SOCKET_FILENAME: &str = "atm-daemon.sock";
const MAX_VERSION_LENGTH: usize = 256;

/// Shared protocol send-shaped response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendResponseEnvelope {
    Sent(SendOutcome),
    Acknowledged(AckOutcome),
}

/// Shared protocol request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestEnvelope {
    Write(Box<WriteRequest>),
    CompatibilityPreflight(CompatibilityPreflight),
    Heartbeat(TeamMemberHeartbeatRequest),
    GraftReceiverRegister(GraftReceiverRegistration),
    GraftReceiverUnregister(GraftReceiverUnregistration),
    GraftReceiverLookup {
        team: TeamName,
        agent: AgentName,
    },
    List(ListQuery),
    Peek(PeekQuery),
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
    Search(Box<SearchRequest>),
    /// Authenticated local control request that reloads the daemon's durable runtime view.
    ReloadRuntimeView,
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    CompatibilityVerdict(CompatibilityVerdict),
    Heartbeat(TeamMemberHeartbeatResponse),
    GraftReceiverRegister,
    GraftReceiverUnregister,
    GraftReceiverLookup(Option<GraftReceiverLease>),
    List(ListOutcome),
    Peek(Box<ReadOutcome>),
    Receive(Box<ReadOutcome>),
    Clear(ClearOutcome),
    Doctor(Box<DoctorReport>),
    Search(Box<SearchResponse>),
    RuntimeViewReloaded,
    Error(AtmError),
}

pub const CLI_SCHEMA_VERSION: u16 = 1;
pub const HTTP_API_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ReleaseVersion(Version);

impl ReleaseVersion {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, AtmError> {
        parse_semver(value.as_ref(), "ATM release version").map(Self)
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("package version must be semver")
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.to_string().as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct HttpApiVersion(Version);

impl HttpApiVersion {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, AtmError> {
        parse_semver(value.as_ref(), "ATM HTTP API version").map(Self)
    }

    pub fn current() -> Self {
        Self::parse(HTTP_API_VERSION).expect("HTTP API version must be semver")
    }

    pub const fn major(&self) -> u64 {
        self.0.major
    }
}

fn parse_semver(value: &str, label: &str) -> Result<Version, AtmError> {
    let value = value.trim();
    if value.len() > MAX_VERSION_LENGTH {
        return Err(AtmError::new(
            AtmErrorCode::ClientDaemonVersionIncompatible,
            format!("{label} exceeds the {MAX_VERSION_LENGTH}-byte limit"),
        ));
    }
    Version::parse(value).map_err(|_| {
        AtmError::new(
            AtmErrorCode::ClientDaemonVersionIncompatible,
            format!("invalid {label} `{value}`"),
        )
    })
}

impl fmt::Display for HttpApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.to_string().as_str())
    }
}

#[cfg(test)]
mod compatibility_version_tests {
    use super::{HttpApiVersion, ReleaseVersion};

    #[test]
    fn release_versions_accept_prereleases_and_reject_non_semver() {
        assert!(ReleaseVersion::parse("1.3.2-alpha.1").is_ok());
        assert!(ReleaseVersion::parse("1.3.2-beta.1").is_ok());
        assert!(ReleaseVersion::parse("1.3").is_err());
        assert!(ReleaseVersion::parse("1.3.2-").is_err());
    }

    #[test]
    fn http_api_version_exposes_independent_major() {
        let version = HttpApiVersion::parse("1.4.0").expect("HTTP API version");
        assert_eq!(version.major(), 1);
        assert_eq!(version.to_string(), "1.4.0");
        assert!(HttpApiVersion::parse("1.4").is_err());
    }

    #[test]
    fn semver_parsing_rejects_oversized_values_before_parser() {
        let oversized = format!("1.2.3-{}", "a".repeat(super::MAX_VERSION_LENGTH));
        assert!(ReleaseVersion::parse(&oversized).is_err());
        assert!(HttpApiVersion::parse(&oversized).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityPreflight {
    pub client_release: ReleaseVersion,
    pub cli_schema_version: u16,
    pub http_api_version: HttpApiVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompatibilityVerdict {
    Compatible {
        daemon_release: ReleaseVersion,
        daemon_schema_version: u16,
        daemon_http_api_version: HttpApiVersion,
    },
    Incompatible {
        client_release: ReleaseVersion,
        daemon_release: ReleaseVersion,
        client_schema_version: u16,
        daemon_schema_version: u16,
        client_http_api_version: HttpApiVersion,
        daemon_http_api_version: HttpApiVersion,
        code: AtmErrorCode,
    },
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    pub fn new(request_id: u64) -> Result<Self, AtmError> {
        let request_id = NonZeroU64::new(request_id).ok_or_else(|| {
            AtmError::validation("ATM daemon protocol request_id must be non-zero")
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

pub fn next_request_id() -> RequestId {
    loop {
        let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(request_id) = NonZeroU64::new(request_id) {
            return RequestId(request_id);
        }
    }
}

/// Resolve the active daemon socket path for the ATM request transport.
///
/// # Errors
///
/// Returns [`AtmError`] when the accepted ATM runtime root cannot be resolved.
pub fn daemon_socket_path() -> Result<PathBuf, AtmError> {
    if env::var_os("ATM_DAEMON_SOCKET").is_some_and(|value| !value.is_empty()) {
        return Err(AtmError::socket_override_forbidden(
            "ATM_DAEMON_SOCKET cannot override the host singleton endpoint",
        ));
    }
    Ok(home::current_host_runtime_scope()?.socket)
}

/// Resolve the canonical daemon socket path for one accepted ATM home root.
pub fn daemon_socket_path_from_home(home_dir: &Path) -> PathBuf {
    home::host_runtime_dir_from_home(home_dir).join(DAEMON_SOCKET_FILENAME)
}

/// Resolve the active local IPC name for the ATM request transport.
///
/// # Errors
///
/// Returns [`AtmError`] when the active endpoint cannot be mapped to a valid
/// platform-local IPC name.
#[cfg(not(windows))]
pub fn daemon_local_ipc_name() -> Result<Name<'static>, AtmError> {
    daemon_local_ipc_name_from_path(&daemon_socket_path()?)
}

/// Windows has no local IPC endpoint. Local clients discover the daemon's
/// loopback HTTP record instead. Retaining this symbol only keeps legacy
/// callers buildable until they migrate; it never constructs a fallback.
#[cfg(windows)]
pub fn daemon_local_ipc_name() -> Result<Name<'static>, AtmError> {
    Err(AtmError::daemon_unavailable(
        "Windows local IPC is retired; use the local HTTP endpoint record",
    ))
}

/// Convert one daemon endpoint path into the concrete platform-local IPC name
/// used by the same-host transport.
///
/// # Errors
///
/// Returns [`AtmError`] when the endpoint cannot be represented by the current
/// platform-local IPC implementation.
#[cfg(not(windows))]
pub fn daemon_local_ipc_name_from_path(endpoint_path: &Path) -> Result<Name<'static>, AtmError> {
    endpoint_path
        .to_path_buf()
        .into_os_string()
        .to_fs_name::<GenericFilePath>()
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to map daemon local IPC endpoint {} to a supported platform-local IPC name",
                endpoint_path.display()
            ))
            .with_cause(source)
        })
}

#[cfg(windows)]
pub fn daemon_local_ipc_name_from_path(_endpoint_path: &Path) -> Result<Name<'static>, AtmError> {
    daemon_local_ipc_name()
}

/// Shared notification event payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Delivery,
}

impl fmt::Display for NotificationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Delivery => "delivery",
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

/// Provenance of an observation accepted by the daemon runtime cache.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeObservationSource {
    Heartbeat,
    LocalCommand,
}

/// One daemon heartbeat request for one team member identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamMemberHeartbeatRequest {
    pub team: TeamName,
    pub member: AgentName,
    pub pid: u32,
    pub observed_at: IsoTimestamp,
    pub activity: HeartbeatActivity,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_session_id"
    )]
    pub session_id: Option<SessionId>,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_session_id"
    )]
    pub session_id: Option<SessionId>,
}

/// Owner-checked removal request for one durable graft receiver lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverUnregistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub owner_generation: OwnerGeneration,
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

/// Current non-authoritative runtime observation for one roster member.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeMemberObservation {
    pub team: TeamName,
    pub member: AgentName,
    pub state: RuntimeMemberState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_changed_by: Option<RuntimeObservationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_changed_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_changed_by: Option<RuntimeObservationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_changed_at: Option<IsoTimestamp>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<RuntimeMemberObservation>,
    /// Cumulative queue-kind graft handoff failures observed by this daemon.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub graft_queue_handoff_failures_total: u64,
    /// Cumulative failures setting a deferred queue marker.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub queue_marker_set_failures_total: u64,
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        DAEMON_SOCKET_FILENAME, HeartbeatActivity, RequestEnvelope, ResponseEnvelope,
        RuntimeLivenessState, RuntimeMemberObservation, RuntimeMemberState, RuntimeReadinessState,
        RuntimeStatusCounts, RuntimeStatusSnapshot, TeamMemberHeartbeatRequest,
        TeamMemberHeartbeatResponse, daemon_socket_path, daemon_socket_path_from_home,
    };
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::list::ListQuery;
    use crate::search::{SearchInput, SearchRequest};
    use crate::send::{SendMessageSource, SendRequest};
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, ReadSelection, SessionId, TeamName};
    use serde::Deserialize;
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    fn heartbeat_request_envelope_round_trips() {
        let observed_at = IsoTimestamp::now();
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: TeamName::from_validated("test-team"),
            member: AgentName::from_validated("test-agent"),
            pid: 4242,
            observed_at,
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: Some(SessionId::new("session-1").expect("valid session id")),
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
                assert_eq!(
                    decoded.session_id.as_ref().map(AsRef::as_ref),
                    Some("session-1")
                );
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
            session_id: None,
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
                assert_eq!(decoded.session_id, None);
            }
            other => panic!("expected heartbeat response, got {other:?}"),
        }
    }

    #[test]
    fn heartbeat_session_id_is_additive_and_normalizes_legacy_blank_input() {
        let observed_at = IsoTimestamp::now();
        let request = TeamMemberHeartbeatRequest {
            team: TeamName::from_validated("test-team"),
            member: AgentName::from_validated("test-agent"),
            pid: 4242,
            observed_at,
            activity: HeartbeatActivity::Idle,
            session_id: None,
        };

        let encoded = serde_json::to_value(&request).expect("encode heartbeat request");
        assert!(encoded.get("session_id").is_none());

        let legacy_without_session = serde_json::json!({
            "team": "test-team",
            "member": "test-agent",
            "pid": 4242,
            "observed_at": observed_at,
            "activity": "idle"
        });
        let decoded: TeamMemberHeartbeatRequest =
            serde_json::from_value(legacy_without_session).expect("decode old heartbeat");
        assert_eq!(decoded.session_id, None);

        let legacy_blank_session = serde_json::json!({
            "team": "test-team",
            "member": "test-agent",
            "pid": 4242,
            "observed_at": observed_at,
            "activity": "idle",
            "session_id": " \t "
        });
        let decoded: TeamMemberHeartbeatRequest =
            serde_json::from_value(legacy_blank_session).expect("decode blank session");
        assert_eq!(decoded.session_id, None);
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
            members: Vec::new(),
            graft_queue_handoff_failures_total: 0,
            queue_marker_set_failures_total: 0,
        };

        let encoded = serde_json::to_vec(&snapshot).expect("encode runtime snapshot");
        let decoded: RuntimeStatusSnapshot =
            serde_json::from_slice(&encoded).expect("decode runtime snapshot");

        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn runtime_status_snapshot_accepts_pre_aj_payload_without_members() {
        let legacy_payload = serde_json::json!({
            "liveness": "running",
            "readiness": "ready",
            "degraded_ingest": false,
            "member_counts": {
                "active_members": 1,
                "idle_members": 0,
                "offline_members": 0,
                "unknown_members": 0
            }
        });

        let decoded: RuntimeStatusSnapshot =
            serde_json::from_value(legacy_payload).expect("decode pre-AJ snapshot");
        assert!(decoded.members.is_empty());
    }

    #[test]
    fn older_runtime_snapshot_reader_ignores_additive_members_field() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct LegacyRuntimeStatusSnapshot {
            liveness: RuntimeLivenessState,
            readiness: RuntimeReadinessState,
            #[serde(default)]
            degraded_ingest: bool,
            #[serde(default)]
            member_counts: RuntimeStatusCounts,
        }

        let snapshot = RuntimeStatusSnapshot {
            liveness: RuntimeLivenessState::Running,
            readiness: RuntimeReadinessState::Ready,
            detail: None,
            singleton_owner_pid: None,
            degraded_ingest: false,
            member_counts: RuntimeStatusCounts::default(),
            members: vec![RuntimeMemberObservation {
                team: TeamName::from_validated("test-team"),
                member: AgentName::from_validated("test-agent"),
                state: RuntimeMemberState::Active,
                session_id: Some(SessionId::new("session-1").expect("session")),
                pid: Some(42),
                last_active_at: None,
                state_changed_by: None,
                state_changed_at: None,
                session_changed_by: None,
                session_changed_at: None,
            }],
            graft_queue_handoff_failures_total: 0,
            queue_marker_set_failures_total: 0,
        };
        let decoded: LegacyRuntimeStatusSnapshot =
            serde_json::from_value(serde_json::to_value(snapshot).expect("encode snapshot"))
                .expect("decode additive snapshot as legacy reader");
        assert_eq!(
            decoded,
            LegacyRuntimeStatusSnapshot {
                liveness: RuntimeLivenessState::Running,
                readiness: RuntimeReadinessState::Ready,
                degraded_ingest: false,
                member_counts: RuntimeStatusCounts::default(),
            }
        );
    }

    #[test]
    fn daemon_socket_path_from_home_uses_atm_home_runtime_subtree() {
        let tempdir = TempDir::new().expect("tempdir");
        let logical_endpoint =
            crate::home::host_runtime_dir_from_home(tempdir.path()).join(DAEMON_SOCKET_FILENAME);

        assert_eq!(
            daemon_socket_path_from_home(tempdir.path()),
            logical_endpoint
        );
    }

    #[test]
    #[serial(env)]
    fn daemon_socket_path_ignores_atm_home() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        let os_home = tempdir.path().join("os-home");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            ("ATM_DAEMON_SOCKET", None),
            ("HOME", Some(os_home.to_str().expect("utf8 os home"))),
        ]);

        assert_eq!(
            daemon_socket_path().expect("daemon socket path"),
            crate::home::current_host_runtime_scope()
                .expect("host scope")
                .socket
        );
    }

    #[test]
    #[serial(env)]
    fn daemon_socket_path_rejects_override() {
        let _env = EnvGuard::set_many([("ATM_DAEMON_SOCKET", Some("alternate.sock"))]);
        assert!(daemon_socket_path().is_err());
    }

    #[test]
    fn protocol_error_round_trip_preserves_exact_code_and_message() {
        let error = AtmError::member_not_found(TEST_SENDER, TEST_TEAM);
        let response = ResponseEnvelope::Error(error.clone());
        let round_trip: ResponseEnvelope =
            serde_json::from_slice(&serde_json::to_vec(&response).expect("serialize error"))
                .expect("deserialize error");

        let ResponseEnvelope::Error(round_trip) = round_trip else {
            panic!("expected error response");
        };
        assert_eq!(round_trip, error);
        assert_eq!(round_trip.code(), AtmErrorCode::MemberNotFound);
    }

    fn test_atm_home_dir() -> PathBuf {
        std::env::temp_dir().join("atm-protocol-test-home")
    }

    fn test_workspace_dir() -> PathBuf {
        std::env::temp_dir().join("atm-protocol-test-workspace")
    }

    #[test]
    fn request_envelope_round_trips_nested_send_caller_context() {
        let request = RequestEnvelope::Write(Box::new(
            SendRequest::new(
                test_atm_home_dir(),
                test_workspace_dir(),
                AgentName::from_validated(TEST_SENDER),
                "recipient@test-team",
                TeamName::from_validated(TEST_TEAM),
                SendMessageSource::Inline("hello".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        ));

        let decoded: RequestEnvelope =
            serde_json::from_slice(&serde_json::to_vec(&request).expect("encode request"))
                .expect("decode nested send request");

        match decoded {
            RequestEnvelope::Write(request) => {
                assert_eq!(request.caller_identity.as_str(), TEST_SENDER);
                assert_eq!(request.caller_team.as_str(), TEST_TEAM);
            }
            other => panic!("expected send request, got {other:?}"),
        }
    }

    #[test]
    fn request_envelope_round_trips_nested_list_caller_context() {
        let request = RequestEnvelope::List(
            ListQuery::new(
                test_atm_home_dir(),
                test_workspace_dir(),
                AgentName::from_validated(TEST_SENDER),
                Some("recipient@test-team"),
                TeamName::from_validated(TEST_TEAM),
                ReadSelection::Unread,
                false,
                Some(25),
                None,
                None,
                None,
                None,
            )
            .expect("list query"),
        );

        let decoded: RequestEnvelope =
            serde_json::from_slice(&serde_json::to_vec(&request).expect("encode request"))
                .expect("decode nested list request");

        match decoded {
            RequestEnvelope::List(query) => {
                assert_eq!(query.caller_identity.as_str(), TEST_SENDER);
                assert_eq!(query.caller_team.as_str(), TEST_TEAM);
            }
            other => panic!("expected list request, got {other:?}"),
        }
    }

    #[test]
    fn request_envelope_search_box_preserves_wire_shape() {
        let request = RequestEnvelope::Search(Box::new(SearchRequest {
            query: SearchInput {
                text: Some("workflow fact".to_owned()),
                ..SearchInput::default()
            },
            lifecycle: None,
        }));

        let encoded = serde_json::to_value(&request).expect("encode search request");
        assert_eq!(encoded["Search"]["query"]["text"], "workflow fact");
        let decoded: RequestEnvelope =
            serde_json::from_value(encoded).expect("decode search request");

        let RequestEnvelope::Search(decoded) = decoded else {
            panic!("expected search request");
        };
        assert_eq!(decoded.query.text.as_deref(), Some("workflow fact"));
    }
}
