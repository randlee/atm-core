//! Shared protocol DTOs for the core transport boundary family.

use std::env;
use std::fmt;
use std::num::NonZeroU64;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use atm_storage::{CronSchedule, HookRegistration};
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
use crate::hook::{
    CronEndRequest, CronListQuery, CronStartRequest, HookActionsResponse, HookDisableRequest,
    HookListQuery, HookRegisterRequest,
};
use crate::list::{ListOutcome, ListQuery};
use crate::read::{PeekQuery, ReadOutcome, ReadQuery};
use crate::schema::AtmMessageId;
use crate::search::{SearchRequest, SearchResponse};
use crate::send::{SendOutcome, WriteRequest};
use crate::types::TaskId;
use crate::types::{AgentName, IsoTimestamp, SessionId, TeamName, deserialize_optional_session_id};

pub use atm_storage::{
    GraftReceiverLease, GraftReceiverRegistration, LocalCapability, OwnerGeneration,
    RosterRuntimeIdentity, RosterRuntimeMutationOutcome, RosterRuntimeObservation,
    RosterRuntimeObservationUpdate, RosterStateRevision, RuntimeMemberState,
    RuntimeObservationAvailability, RuntimeObservationSource,
};
use atm_storage::{MoveTarget, QueuePosition};

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
    TaskMove(TaskMoveRequest),
    CompatibilityPreflight(CompatibilityPreflight),
    Heartbeat(TeamMemberHeartbeatRequest),
    QueueGetNext(QueueGetNextRequest),
    GraftReceiverRegister(GraftReceiverRegistration),
    GraftReceiverRefresh(GraftReceiverRefreshRequest),
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
    HookRegister(HookRegisterRequest),
    HookDisable(HookDisableRequest),
    HookList(HookListQuery),
    CronStart(CronStartRequest),
    CronEnd(CronEndRequest),
    CronList(CronListQuery),
}

/// Shared protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEnvelope {
    Send(SendResponseEnvelope),
    TaskMove(TaskMoveOutcome),
    CompatibilityVerdict(CompatibilityVerdict),
    Heartbeat(TeamMemberHeartbeatResponse),
    QueueGetNext(QueueGetNextResponse),
    GraftReceiverRegister,
    GraftReceiverRefresh,
    GraftReceiverUnregister,
    GraftReceiverLookup(Option<GraftReceiverLease>),
    List(ListOutcome),
    Peek(Box<ReadOutcome>),
    Receive(Box<ReadOutcome>),
    Clear(ClearOutcome),
    Doctor(Box<DoctorReport>),
    Search(Box<SearchResponse>),
    RuntimeViewReloaded,
    HookRegistered(HookRegistration),
    HookList(Vec<HookRegistration>),
    HookDisabled(Vec<HookRegistration>),
    CronStarted(CronSchedule),
    CronList(Vec<CronSchedule>),
    CronEnded(Vec<CronSchedule>),
    /// Hook actions ran in place of the hooked command; the count is also sent in `X-ATM-Hook-Actions`.
    HookActions(HookActionsResponse),
    Error(AtmError),
}

pub const CLI_SCHEMA_VERSION: u16 = 1;
pub const HTTP_API_VERSION: &str = "1.13.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMoveRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub task_id: TaskId,
    pub target: MoveTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMoveOutcome {
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub from: QueuePosition,
    pub to: QueuePosition,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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

/// Runtime heartbeat activity transported into the canonical ephemeral
/// master-roster record.
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_session_id"
    )]
    pub session_id: Option<SessionId>,
}

/// One daemon heartbeat response after canonical roster-state application.
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

/// Authenticated local request to drain one member's bare-CLI queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueueGetNextRequest {
    pub team: TeamName,
    pub member: AgentName,
}

/// One item returned by the bare-CLI queue pull.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedNudgeMessage {
    pub kind: crate::boundary::NudgeKind,
    pub msg_id: AtmMessageId,
    pub body: String,
}

/// Bare-CLI queue drain result: all current steer items and at most one queue item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueueGetNextResponse {
    pub messages: Vec<QueuedNudgeMessage>,
}

/// Owner-checked removal request for one durable graft receiver lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverUnregistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub owner_generation: OwnerGeneration,
}

/// Owner-checked liveness keepalive for one durable graft receiver lease.
///
/// Unlike [`GraftReceiverRegistration`], which unconditionally upserts, this
/// request is rejected with `NotOwner` when the stored lease no longer
/// matches `owner_generation` — see ADR-056.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverRefreshRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub owner_generation: OwnerGeneration,
}

/// Wire projection of one member's canonical ephemeral master-roster state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeMemberObservation {
    pub team: TeamName,
    pub member: AgentName,
    pub state: RuntimeMemberState,
    #[serde(default)]
    pub revision: RosterStateRevision,
    #[serde(default)]
    pub availability: RuntimeObservationAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observation_attempt_by: Option<RuntimeObservationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observation_attempt_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_by: Option<RuntimeObservationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_at: Option<IsoTimestamp>,
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
    /// Cumulative failures clearing a queue marker after a successful handoff.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub graft_queue_marker_clear_failures_total: u64,
    /// Cumulative bare-CLI FIFO overflow drops observed by the daemon.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub bare_cli_queue_full_drops_total: u64,
    /// Cumulative failures setting a deferred queue marker.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub queue_marker_set_failures_total: u64,
    /// Timestamp of the most recent Herdr queue-wake poll, when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub herdr_queue_last_tick_at: Option<IsoTimestamp>,
    /// Cumulative deferred queue messages drained by an idle transition or
    /// recovery sweep.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub queue_messages_drained_total: u64,
    /// Cumulative deferred queue drain/recovery dispatch failures.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub queue_drain_failures_total: u64,
    /// Cumulative blocking-core-bridge jobs that outlived the remaining
    /// request budget they were dispatched with.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub blocking_core_bridge_stalls_total: u64,
    /// Cumulative caller-owned file/template source preflights that outlived
    /// their request deadline while retaining bounded blocking capacity.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub write_source_preflight_stalls_total: u64,
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        CLI_SCHEMA_VERSION, CompatibilityPreflight, CompatibilityVerdict,
        GraftReceiverRefreshRequest, GraftReceiverRegistration, GraftReceiverUnregistration,
        HttpApiVersion, QueueGetNextResponse, QueuedNudgeMessage, ReleaseVersion,
        SendResponseEnvelope, TaskMoveOutcome,
    };
    use super::{
        DAEMON_SOCKET_FILENAME, HeartbeatActivity, QueueGetNextRequest, RequestEnvelope,
        ResponseEnvelope, RosterStateRevision, RuntimeLivenessState, RuntimeMemberObservation,
        RuntimeMemberState, RuntimeObservationAvailability, RuntimeReadinessState,
        RuntimeStatusCounts, RuntimeStatusSnapshot, TaskMoveRequest, TeamMemberHeartbeatRequest,
        TeamMemberHeartbeatResponse, daemon_socket_path, daemon_socket_path_from_home,
    };
    use crate::boundary::NudgeKind;
    use crate::clear::{ClearOutcome, ClearQuery, RemovedByClass};
    use crate::doctor::DoctorQuery;
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::list::{ListOutcome, ListQuery};
    use crate::read::{BucketCounts, MailboxQueryFilters, PeekQuery, ReadOutcome, ReadQuery};
    use crate::search::{SearchInput, SearchRequest, SearchResponse};
    use crate::send::{
        SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest, WriteRequest,
    };
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use crate::types::{
        AgentName, CommandAction, IsoTimestamp, ReadSelection, SessionId, TeamName,
    };
    use atm_storage::{AtmMessageId, LocalCapability, MoveTarget, OwnerGeneration, QueuePosition};
    use serde::{Deserialize, Serialize};
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
            graft_queue_marker_clear_failures_total: 0,
            bare_cli_queue_full_drops_total: 0,
            queue_marker_set_failures_total: 0,
            herdr_queue_last_tick_at: None,
            queue_messages_drained_total: 0,
            queue_drain_failures_total: 0,
            blocking_core_bridge_stalls_total: 0,
            write_source_preflight_stalls_total: 0,
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
    fn runtime_member_observation_accepts_payload_without_revision_or_availability() {
        let legacy_payload = serde_json::json!({
            "team": "test-team",
            "member": "test-agent",
            "state": "active"
        });

        let decoded: RuntimeMemberObservation =
            serde_json::from_value(legacy_payload).expect("decode legacy member observation");
        assert_eq!(decoded.state, RuntimeMemberState::Active);
        assert_eq!(decoded.revision.get(), 0);
        assert_eq!(
            decoded.availability,
            RuntimeObservationAvailability::Unobserved
        );
        assert_eq!(decoded.last_observed_by, None);
        assert_eq!(decoded.last_observation_attempt_at, None);
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
                revision: RosterStateRevision::default(),
                availability: RuntimeObservationAvailability::Fresh,
                last_observation_attempt_by: None,
                last_observation_attempt_at: None,
                last_observed_by: None,
                last_observed_at: None,
                session_id: Some(SessionId::new("session-1").expect("session")),
                pid: Some(42),
                last_active_at: None,
                state_changed_by: None,
                state_changed_at: None,
                session_changed_by: None,
                session_changed_at: None,
            }],
            graft_queue_handoff_failures_total: 0,
            graft_queue_marker_clear_failures_total: 0,
            bare_cli_queue_full_drops_total: 0,
            queue_marker_set_failures_total: 0,
            herdr_queue_last_tick_at: None,
            queue_messages_drained_total: 0,
            queue_drain_failures_total: 0,
            blocking_core_bridge_stalls_total: 0,
            write_source_preflight_stalls_total: 0,
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
    fn codec_preserves_protocol_1_5_0_fixtures() {
        #[derive(Debug, Deserialize)]
        enum RequestEnvelope15 {
            QueueGetNext(serde_json::Value),
            ReloadRuntimeView,
        }

        let fixtures = [
            serde_json::to_vec(&RequestEnvelope::QueueGetNext(QueueGetNextRequest {
                team: TeamName::from_validated(TEST_TEAM),
                member: AgentName::from_validated(TEST_SENDER),
            }))
            .expect("queue fixture"),
            serde_json::to_vec(&RequestEnvelope::ReloadRuntimeView).expect("reload fixture"),
        ];
        for fixture in fixtures {
            serde_json::from_slice::<RequestEnvelope>(&fixture).expect("1.6 decodes 1.5 fixture");
            match serde_json::from_slice::<RequestEnvelope15>(&fixture)
                .expect("pinned 1.5 enum decodes fixture")
            {
                RequestEnvelope15::QueueGetNext(value) => assert!(value.is_object()),
                RequestEnvelope15::ReloadRuntimeView => {}
            }
        }

        let task_move = serde_json::to_vec(&RequestEnvelope::TaskMove(TaskMoveRequest {
            caller_identity: AgentName::from_validated(TEST_SENDER),
            caller_team: TeamName::from_validated(TEST_TEAM),
            task_id: "T1".parse().expect("task id"),
            target: MoveTarget::Head,
        }))
        .expect("task move fixture");
        let error = serde_json::from_slice::<RequestEnvelope15>(&task_move)
            .expect_err("1.5 enum must reject the 1.6-only variant");
        assert!(error.to_string().contains("unknown variant `TaskMove`"));
    }

    /// Wire tag of every request variant. Exhaustive, so a new variant fails to compile here.
    fn request_tag(request: &RequestEnvelope) -> &'static str {
        match request {
            RequestEnvelope::Write(_) => "Write",
            RequestEnvelope::TaskMove(_) => "TaskMove",
            RequestEnvelope::CompatibilityPreflight(_) => "CompatibilityPreflight",
            RequestEnvelope::Heartbeat(_) => "Heartbeat",
            RequestEnvelope::QueueGetNext(_) => "QueueGetNext",
            RequestEnvelope::GraftReceiverRegister(_) => "GraftReceiverRegister",
            RequestEnvelope::GraftReceiverRefresh(_) => "GraftReceiverRefresh",
            RequestEnvelope::GraftReceiverUnregister(_) => "GraftReceiverUnregister",
            RequestEnvelope::GraftReceiverLookup { .. } => "GraftReceiverLookup",
            RequestEnvelope::List(_) => "List",
            RequestEnvelope::Peek(_) => "Peek",
            RequestEnvelope::Receive(_) => "Receive",
            RequestEnvelope::Clear(_) => "Clear",
            RequestEnvelope::Doctor(_) => "Doctor",
            RequestEnvelope::Search(_) => "Search",
            RequestEnvelope::ReloadRuntimeView => "ReloadRuntimeView",
            RequestEnvelope::HookRegister(_) => "HookRegister",
            RequestEnvelope::HookDisable(_) => "HookDisable",
            RequestEnvelope::HookList(_) => "HookList",
            RequestEnvelope::CronStart(_) => "CronStart",
            RequestEnvelope::CronEnd(_) => "CronEnd",
            RequestEnvelope::CronList(_) => "CronList",
        }
    }

    /// Wire tag of every response variant. Exhaustive, so a new variant fails to compile here.
    fn response_tag(response: &ResponseEnvelope) -> &'static str {
        match response {
            ResponseEnvelope::Send(_) => "Send",
            ResponseEnvelope::TaskMove(_) => "TaskMove",
            ResponseEnvelope::CompatibilityVerdict(_) => "CompatibilityVerdict",
            ResponseEnvelope::Heartbeat(_) => "Heartbeat",
            ResponseEnvelope::QueueGetNext(_) => "QueueGetNext",
            ResponseEnvelope::GraftReceiverRegister => "GraftReceiverRegister",
            ResponseEnvelope::GraftReceiverRefresh => "GraftReceiverRefresh",
            ResponseEnvelope::GraftReceiverUnregister => "GraftReceiverUnregister",
            ResponseEnvelope::GraftReceiverLookup(_) => "GraftReceiverLookup",
            ResponseEnvelope::List(_) => "List",
            ResponseEnvelope::Peek(_) => "Peek",
            ResponseEnvelope::Receive(_) => "Receive",
            ResponseEnvelope::Clear(_) => "Clear",
            ResponseEnvelope::Doctor(_) => "Doctor",
            ResponseEnvelope::Search(_) => "Search",
            ResponseEnvelope::RuntimeViewReloaded => "RuntimeViewReloaded",
            ResponseEnvelope::Error(_) => "Error",
            ResponseEnvelope::HookRegistered(_) => "HookRegistered",
            ResponseEnvelope::HookList(_) => "HookList",
            ResponseEnvelope::HookDisabled(_) => "HookDisabled",
            ResponseEnvelope::HookActions(_) => "HookActions",
            ResponseEnvelope::CronStarted(_) => "CronStarted",
            ResponseEnvelope::CronList(_) => "CronList",
            ResponseEnvelope::CronEnded(_) => "CronEnded",
        }
    }

    /// A `DoctorReport` wire captured in phase aq (972fb36ce) before 1.13 added `hooks`. It is an
    /// earlier producer's output, not an exact 1.12 release capture: no such capture is in the tree.
    fn doctor_report_pre_1_13() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/doctor-report-pre-1-13.json"
        ))
        .expect("pre-1.13 doctor report fixture is JSON")
    }

    /// Frozen HTTP API 1.12 doctor wire shape. Keep this independent of the
    /// current `DoctorReport` so additive fields cannot silently join the old
    /// consumer.
    #[derive(Debug, Serialize, Deserialize)]
    struct DoctorReport12 {
        summary: crate::doctor::DoctorSummary,
        findings: Vec<crate::doctor::DoctorFinding>,
        recommendations: Vec<String>,
        environment: crate::doctor::DoctorEnvironmentVisibility,
        #[serde(default)]
        client_context: crate::doctor::DoctorExecutionContext,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        daemon_context: Option<crate::doctor::DoctorExecutionContext>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reader_lanes: Option<crate::doctor::ReaderPoolDoctorReport>,
        #[serde(default)]
        team_scope: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        member_roster: Option<crate::team_admin::MembersList>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        team_rosters: Vec<crate::team_admin::MembersList>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alias_mismatches: Vec<crate::doctor::DoctorAliasMismatch>,
        #[serde(default)]
        graft_receivers: crate::doctor::GraftReceiversDoctorReport,
        observability: crate::observability::AtmObservabilityHealth,
        #[serde(default)]
        herdr_breaker: crate::doctor::HerdrBreakerDoctorReport,
        #[serde(default)]
        herdr_queue_pump: crate::doctor::HerdrQueuePumpDoctorReport,
        #[serde(default)]
        herdr: crate::doctor::HerdrDoctorReport,
        #[serde(default)]
        post_send: crate::doctor::PostSendDoctorReport,
        #[serde(default)]
        escalation_recipients: crate::doctor::EscalationRecipientsDoctorReport,
        config: crate::boundary::ConfigDoctorReport,
        mail_store: crate::boundary::MailStoreDoctorReport,
        roster_store: crate::boundary::RosterStoreDoctorReport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        daemon_runtime: Option<crate::doctor::DaemonRuntimeDoctorReport>,
        #[serde(default)]
        drift_findings: Vec<crate::doctor::DoctorFinding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_status: Option<RuntimeStatusSnapshot>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bootstrap_trace: Option<crate::doctor::BootstrapTraceReport>,
    }

    fn assert_1_12_doctor_report_preserves_fields(wire: serde_json::Value) -> DoctorReport12 {
        let current: crate::doctor::DoctorReport =
            serde_json::from_value(wire.clone()).expect("current reader decodes doctor report");
        let frozen: DoctorReport12 =
            serde_json::from_value(wire).expect("1.12 reader decodes doctor report");

        let mut expected = serde_json::to_value(current).expect("encode current doctor report");
        expected
            .as_object_mut()
            .expect("doctor report encodes as object")
            .remove("hooks");
        assert_eq!(
            serde_json::to_value(&frozen).expect("encode frozen 1.12 doctor report"),
            expected,
            "every frozen 1.12 doctor field retains its value"
        );
        frozen
    }

    /// One value per 1.12 request variant.
    fn request_samples_12() -> Vec<RequestEnvelope> {
        let team = TeamName::from_validated(TEST_TEAM);
        let sender = AgentName::from_validated(TEST_SENDER);
        let mut requests = message_and_task_requests_12(&team, &sender);
        requests.extend(runtime_requests_12(&team, &sender));
        requests
    }

    /// The 1.12 requests that carry write, mailbox and task payloads.
    fn message_and_task_requests_12(team: &TeamName, sender: &AgentName) -> Vec<RequestEnvelope> {
        let write = WriteRequest::new(
            test_atm_home_dir(),
            test_workspace_dir(),
            sender.clone(),
            "recipient@test-team",
            team.clone(),
            SendMessageSource::Inline("hi".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write request");
        let list = ListQuery::new(
            test_atm_home_dir(),
            test_workspace_dir(),
            sender.clone(),
            Some("recipient@test-team"),
            team.clone(),
            ReadSelection::Unread,
            false,
            Some(25),
            None,
            None,
            None,
            None,
        )
        .expect("list query");
        let peek = PeekQuery::from_filters(
            test_atm_home_dir(),
            test_workspace_dir(),
            sender.clone(),
            team.clone(),
            ReadSelection::Unread,
            false,
            MailboxQueryFilters::default(),
        )
        .expect("peek query");
        let receive = ReadQuery::from_filters(
            test_atm_home_dir(),
            test_workspace_dir(),
            sender.clone(),
            team.clone(),
            ReadSelection::Unread,
            false,
            true,
            MailboxQueryFilters::default(),
        )
        .expect("receive query");
        vec![
            RequestEnvelope::Write(Box::new(write)),
            RequestEnvelope::TaskMove(TaskMoveRequest {
                caller_identity: sender.clone(),
                caller_team: team.clone(),
                task_id: "task-1".parse().expect("task id"),
                target: MoveTarget::Head,
            }),
            RequestEnvelope::List(list),
            RequestEnvelope::Peek(peek),
            RequestEnvelope::Receive(receive),
        ]
    }

    /// The remaining 1.12 requests: preflight, heartbeat, queue, graft receivers, clear,
    /// doctor, search and reload.
    fn runtime_requests_12(team: &TeamName, sender: &AgentName) -> Vec<RequestEnvelope> {
        let generation =
            OwnerGeneration::new("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("owner generation");
        vec![
            RequestEnvelope::CompatibilityPreflight(CompatibilityPreflight {
                client_release: ReleaseVersion::current(),
                cli_schema_version: CLI_SCHEMA_VERSION,
                http_api_version: HttpApiVersion::parse("1.12.0").expect("1.12.0"),
            }),
            RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: sender.clone(),
                pid: 4242,
                observed_at: "2026-10-19T00:00:00Z".parse().expect("timestamp"),
                activity: HeartbeatActivity::Idle,
                session_id: None,
            }),
            RequestEnvelope::QueueGetNext(QueueGetNextRequest {
                team: team.clone(),
                member: sender.clone(),
            }),
            RequestEnvelope::GraftReceiverRegister(GraftReceiverRegistration {
                team: team.clone(),
                agent: sender.clone(),
                endpoint: "127.0.0.1:7001".parse().expect("endpoint"),
                capability: LocalCapability::generate().expect("capability"),
                owner_generation: generation.clone(),
            }),
            RequestEnvelope::GraftReceiverRefresh(GraftReceiverRefreshRequest {
                team: team.clone(),
                agent: sender.clone(),
                owner_generation: generation.clone(),
            }),
            RequestEnvelope::GraftReceiverUnregister(GraftReceiverUnregistration {
                team: team.clone(),
                agent: sender.clone(),
                owner_generation: generation,
            }),
            RequestEnvelope::GraftReceiverLookup {
                team: team.clone(),
                agent: sender.clone(),
            },
            RequestEnvelope::Clear(ClearQuery {
                home_dir: test_atm_home_dir(),
                current_dir: test_workspace_dir(),
                caller_identity: sender.clone(),
                caller_team: team.clone(),
                older_than: None,
                idle_only: false,
                dry_run: true,
            }),
            RequestEnvelope::Doctor(DoctorQuery {
                home_dir: test_atm_home_dir(),
                current_dir: test_workspace_dir(),
                team_override: None,
                all_teams: false,
                caller_team: Some(team.clone()),
                caller_identity: Some(sender.clone()),
            }),
            RequestEnvelope::Search(Box::new(SearchRequest {
                query: SearchInput::default(),
                lifecycle: None,
            })),
            RequestEnvelope::ReloadRuntimeView,
        ]
    }

    /// One value per 1.12 response variant.
    fn response_samples_12() -> Vec<ResponseEnvelope> {
        let team = TeamName::from_validated(TEST_TEAM);
        let sender = AgentName::from_validated(TEST_SENDER);
        let mut responses = send_and_control_responses_12(&team, &sender);
        responses.extend(query_responses_12(&team, &sender));
        responses
    }

    /// The 1.12 responses for send, task move, compatibility, heartbeat, queue and graft routes.
    fn send_and_control_responses_12(team: &TeamName, sender: &AgentName) -> Vec<ResponseEnvelope> {
        let message_id: AtmMessageId = "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().expect("message id");
        vec![
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
                action: CommandAction::Send,
                team: team.clone(),
                agent: sender.clone(),
                sender: sender.clone(),
                outcome: SendCommandOutcome::Sent,
                message_id,
                requires_ack: false,
                task_id: None,
                task_complete: None,
                already_closed: None,
                summary: None,
                message: None,
                warnings: Vec::new(),
                dry_run: false,
            })),
            ResponseEnvelope::TaskMove(TaskMoveOutcome {
                task_id: "task-1".parse().expect("task id"),
                assignee: sender.clone(),
                from: QueuePosition::HEAD,
                to: QueuePosition::HEAD,
            }),
            ResponseEnvelope::CompatibilityVerdict(CompatibilityVerdict::Compatible {
                daemon_release: ReleaseVersion::current(),
                daemon_schema_version: CLI_SCHEMA_VERSION,
                daemon_http_api_version: HttpApiVersion::parse("1.12.0").expect("1.12.0"),
            }),
            ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
                team: team.clone(),
                member: sender.clone(),
                pid: 4242,
                pid_changed: false,
                state: RuntimeMemberState::Idle,
                last_active_at: None,
                session_id: None,
            }),
            ResponseEnvelope::QueueGetNext(QueueGetNextResponse {
                messages: vec![QueuedNudgeMessage {
                    kind: NudgeKind::Queue,
                    msg_id: message_id,
                    body: "queued".to_owned(),
                }],
            }),
            ResponseEnvelope::GraftReceiverRegister,
            ResponseEnvelope::GraftReceiverRefresh,
            ResponseEnvelope::GraftReceiverUnregister,
            ResponseEnvelope::GraftReceiverLookup(None),
        ]
    }

    /// The 1.12 responses for list, peek, receive, clear, doctor, search, reload and error.
    fn query_responses_12(team: &TeamName, sender: &AgentName) -> Vec<ResponseEnvelope> {
        let read_outcome = || ReadOutcome {
            action: CommandAction::Read,
            team: team.clone(),
            agent: sender.clone(),
            selection_mode: ReadSelection::Unread,
            mutation_applied: false,
            count: 0,
            message: None,
            selected_message_id: None,
            match_count: 0,
            additional_match_count: 0,
            bucket_counts: BucketCounts {
                unread: 0,
                pending_ack: 0,
                history: 0,
            },
        };
        vec![
            ResponseEnvelope::List(ListOutcome {
                action: CommandAction::List,
                team: team.clone(),
                agent: sender.clone(),
                selection_mode: ReadSelection::Unread,
                history_collapsed: false,
                count: 0,
                rows: Vec::new(),
                bucket_counts: BucketCounts {
                    unread: 0,
                    pending_ack: 0,
                    history: 0,
                },
                task_rows: Vec::new(),
                task_event_rows: Vec::new(),
                handoffs: Vec::new(),
            }),
            ResponseEnvelope::Peek(Box::new(read_outcome())),
            ResponseEnvelope::Receive(Box::new(read_outcome())),
            ResponseEnvelope::Clear(ClearOutcome {
                action: CommandAction::Clear,
                team: team.clone(),
                agent: sender.clone(),
                removed_total: 0,
                remaining_total: 0,
                removed_by_class: RemovedByClass {
                    acknowledged: 0,
                    read: 0,
                },
            }),
            ResponseEnvelope::Doctor(Box::new(
                serde_json::from_value(doctor_report_pre_1_13()).expect("1.13 decodes 1.12 report"),
            )),
            ResponseEnvelope::Search(Box::new(SearchResponse {
                hits: Vec::new(),
                aggregate: None,
                next_cursor: None,
                lifecycle: None,
            })),
            ResponseEnvelope::RuntimeViewReloaded,
            ResponseEnvelope::Error(AtmError::validation("pinned 1.12 error")),
        ]
    }

    #[test]
    fn codec_preserves_protocol_1_12_0_fixtures() {
        // Pinned from `RequestEnvelope` and `ResponseEnvelope` at HTTP API 1.12.0
        // (git b2726d423^). A 1.12 reader decodes exactly these tags.
        const REQUEST_VARIANTS_12: &[&str] = &[
            "Write",
            "TaskMove",
            "CompatibilityPreflight",
            "Heartbeat",
            "QueueGetNext",
            "GraftReceiverRegister",
            "GraftReceiverRefresh",
            "GraftReceiverUnregister",
            "GraftReceiverLookup",
            "List",
            "Peek",
            "Receive",
            "Clear",
            "Doctor",
            "Search",
            "ReloadRuntimeView",
        ];
        const RESPONSE_VARIANTS_12: &[&str] = &[
            "Send",
            "TaskMove",
            "CompatibilityVerdict",
            "Heartbeat",
            "QueueGetNext",
            "GraftReceiverRegister",
            "GraftReceiverRefresh",
            "GraftReceiverUnregister",
            "GraftReceiverLookup",
            "List",
            "Peek",
            "Receive",
            "Clear",
            "Doctor",
            "Search",
            "RuntimeViewReloaded",
            "Error",
        ];
        // Added in 1.13.0. No 1.12 reader knows these tags.
        const REQUEST_VARIANTS_13: &[&str] = &[
            "HookRegister",
            "HookDisable",
            "HookList",
            "CronStart",
            "CronEnd",
            "CronList",
        ];
        const RESPONSE_VARIANTS_13: &[&str] = &[
            "HookRegistered",
            "HookList",
            "HookDisabled",
            "HookActions",
            "CronStarted",
            "CronList",
            "CronEnded",
        ];
        let wire_tag = |bytes: &[u8]| -> String {
            match serde_json::from_slice::<serde_json::Value>(bytes).expect("message is json") {
                serde_json::Value::String(tag) => tag,
                serde_json::Value::Object(map) if map.len() == 1 => {
                    map.keys().next().cloned().expect("one variant key")
                }
                other => panic!("unexpected message shape: {other}"),
            }
        };

        // Every 1.12 request variant encodes under its pinned tag and decodes back to itself.
        let mut request_tags = Vec::new();
        for request in request_samples_12() {
            let tag = request_tag(&request);
            let bytes = serde_json::to_vec(&request).expect("encode 1.12 request");
            assert_eq!(wire_tag(&bytes), tag);
            assert!(
                REQUEST_VARIANTS_12.contains(&tag),
                "{tag} is not a 1.12 request"
            );
            let decoded: RequestEnvelope =
                serde_json::from_slice(&bytes).expect("1.13 decodes the 1.12 request");
            assert_eq!(request_tag(&decoded), tag);
            request_tags.push(tag);
        }
        let mut pinned_requests = REQUEST_VARIANTS_12.to_vec();
        request_tags.sort_unstable();
        pinned_requests.sort_unstable();
        assert_eq!(
            request_tags, pinned_requests,
            "every pinned 1.12 request is sampled"
        );

        // Every 1.12 response variant does the same.
        let mut response_tags = Vec::new();
        for response in response_samples_12() {
            let tag = response_tag(&response);
            let bytes = serde_json::to_vec(&response).expect("encode 1.12 response");
            assert_eq!(wire_tag(&bytes), tag);
            assert!(
                RESPONSE_VARIANTS_12.contains(&tag),
                "{tag} is not a 1.12 response"
            );
            let decoded: ResponseEnvelope =
                serde_json::from_slice(&bytes).expect("1.13 decodes the 1.12 response");
            assert_eq!(response_tag(&decoded), tag);
            response_tags.push(tag);
        }
        let mut pinned_responses = RESPONSE_VARIANTS_12.to_vec();
        response_tags.sort_unstable();
        pinned_responses.sort_unstable();
        assert_eq!(
            response_tags, pinned_responses,
            "every pinned 1.12 response is sampled"
        );

        // A 1.13-only route is not a 1.12 request.
        let hook_list = RequestEnvelope::HookList(crate::hook::HookListQuery {
            caller_identity: AgentName::from_validated(TEST_SENDER),
            caller_team: TeamName::from_validated(TEST_TEAM),
            phase: None,
        });
        assert!(REQUEST_VARIANTS_13.contains(&request_tag(&hook_list)));
        assert!(!REQUEST_VARIANTS_12.contains(&request_tag(&hook_list)));
        for tag in RESPONSE_VARIANTS_13 {
            assert!(
                !RESPONSE_VARIANTS_12.contains(tag),
                "{tag} is a 1.13 response"
            );
        }

        // A frozen 1.12 DoctorReport decodes both the pre-1.13 fixture and a
        // 1.13 report, preserving every 1.12 field while ignoring `hooks`.
        let pre_13 = doctor_report_pre_1_13();
        assert!(pre_13.get("hooks").is_none());
        let from_pre_13 = assert_1_12_doctor_report_preserves_fields(pre_13);
        assert_eq!(
            from_pre_13.summary.status,
            crate::doctor::DoctorStatus::Healthy
        );
        assert!(from_pre_13.recommendations.is_empty());

        let entry = crate::doctor::hooks::HookDoctorEntry {
            module_path: "/modules/gate.wasm".into(),
            cwd: "/work".into(),
            team: TeamName::from_validated(TEST_TEAM),
            phase: crate::boundary::PhaseId::parse("bk").expect("phase"),
            hooks: vec![crate::boundary::HookName::parse("task.assign").expect("hook")],
            schedules: Vec::new(),
            timeout_ms: 1_000,
            success: Some(true),
            message: Some("ok".to_owned()),
            findings: Vec::new(),
            error: None,
        };
        let mut with_hooks = doctor_report_pre_1_13();
        with_hooks["hooks"] =
            serde_json::json!([serde_json::to_value(&entry).expect("entry json")]);
        let report: crate::doctor::DoctorReport =
            serde_json::from_value(with_hooks.clone()).expect("1.13 decodes the 1.13 report");
        assert_eq!(report.hooks.len(), 1);
        let encoded = serde_json::to_value(&report).expect("encode 1.13 report");
        assert!(encoded["hooks"].is_array());
        let from_1_13 = assert_1_12_doctor_report_preserves_fields(with_hooks);
        assert_eq!(
            from_1_13.summary.status,
            crate::doctor::DoctorStatus::Healthy
        );
        assert!(from_1_13.recommendations.is_empty());
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
