use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::{AgentName, IsoTimestamp, TeamName};

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
    pub degraded_peer_listener: bool,
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
