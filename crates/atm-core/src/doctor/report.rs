use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::boundary::{ConfigDoctorReport, MailStoreDoctorReport, RosterStoreDoctorReport};
use crate::error_codes::AtmErrorCode;
use crate::observability::AtmObservabilityHealth;
use crate::protocol::{ReleaseVersion, RuntimeStatusSnapshot};
use crate::team_admin::MembersList;
use crate::types::{AgentName, HostName, IsoTimestamp, TeamName};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Healthy,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorFinding {
    pub severity: DoctorSeverity,
    pub code: AtmErrorCode,
    pub message: String,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorSummary {
    pub status: DoctorStatus,
    pub message: String,
    pub info_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorEnvironmentVisibility {
    pub atm_home: Option<PathBuf>,
    pub atm_team: Option<TeamName>,
    pub atm_identity: Option<AgentName>,
    pub team_override: Option<TeamName>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DoctorExecutionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<ReleaseVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_schema_version: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_api_version: Option<crate::protocol::HttpApiVersion>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapConnectOutcome {
    Connected,
    NotFound,
    Timeout,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapLaunchGateOutcome {
    Launched,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapAutoStartOutcome {
    AutoStarted,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapTraceReport {
    pub daemon_connect: BootstrapConnectOutcome,
    pub daemon_launch_gate: BootstrapLaunchGateOutcome,
    pub daemon_auto_start: BootstrapAutoStartOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_gate_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_start_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DaemonRuntimeDoctorReport {
    pub findings: Vec<DoctorFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_config: Option<PeerConfigDoctorReport>,
    /// Bounded, in-memory delivery health for configured peers. This is
    /// diagnostic state only: it never contains message data, receipts,
    /// cursors, resolved addresses, or TLS material.
    #[serde(default)]
    pub peer_links: Vec<PeerLinkStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_wire_security: Option<PeerWireSecurityStatus>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerWireSecurityStatus {
    MutualTls,
    PlaintextTest,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[deprecated(note = "AK.2: delete worker-only peer delivery doctor projection")]
pub enum PeerLinkQuality {
    Healthy,
    Degraded,
    Unreachable,
    #[default]
    Misconfigured,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[deprecated(note = "AK.2: delete worker-only peer delivery doctor projection")]
pub enum PeerDrainState {
    #[default]
    Idle,
    Connecting,
    Draining,
}

/// Safe per-peer delivery-health projection. The peer is always the registered
/// hostname; this deliberately excludes addresses resolved from DNS.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[deprecated(note = "AK.2: delete worker-only peer delivery doctor projection")]
pub struct PeerLinkStatus {
    pub peer: HostName,
    pub quality: PeerLinkQuality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<AtmErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_attempt_at: Option<IsoTimestamp>,
    #[serde(default)]
    pub drain: PeerDrainState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
}

impl PeerLinkStatus {
    pub fn misconfigured(peer: HostName) -> Self {
        Self {
            peer,
            quality: PeerLinkQuality::Misconfigured,
            last_success_at: None,
            last_failure_at: None,
            last_error_code: None,
            next_attempt_at: None,
            drain: PeerDrainState::Idle,
            candidate_count: None,
        }
    }
}

/// Safe control-plane visibility for cross-host HTTPS. Private key references
/// and key material are intentionally omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PeerConfigDoctorReport {
    pub configured_interface_count: usize,
    pub enabled_interface_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_fingerprint: Option<String>,
    pub trusted_peer_count: usize,
    pub enabled_trusted_peer_count: usize,
    #[serde(default)]
    pub trusted_peers: Vec<PeerAuthorityDoctorReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_failure: Option<DoctorFinding>,
}

/// Safe durable peer authority projection. Never includes DNS output or secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerAuthorityDoctorReport {
    pub host: String,
    pub https_port: u16,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PostSendHookRuleReport {
    pub recipient_matcher: String,
    pub executable: PathBuf,
    pub argv: Vec<String>,
    pub config_root: PathBuf,
}

/// Stable, zero-based position of an external post-send rule in the active
/// configuration.  Keeping this distinct from arbitrary integers prevents a
/// consumer from treating a recipient index as a hook-rule reference.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct PostSendHookRuleIndex(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecipientDeliveryPath {
    BuiltIn,
    ExternalOverride { rule: PostSendHookRuleIndex },
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipientDeliveryPathReport {
    pub recipient: AgentName,
    pub path: RecipientDeliveryPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PostSendDoctorReport {
    pub config_root: PathBuf,
    pub external_rules: Vec<PostSendHookRuleReport>,
    pub recipient_paths: Vec<RecipientDeliveryPathReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoctorReport {
    pub summary: DoctorSummary,
    pub findings: Vec<DoctorFinding>,
    pub recommendations: Vec<String>,
    pub environment: DoctorEnvironmentVisibility,
    #[serde(default)]
    pub client_context: DoctorExecutionContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_context: Option<DoctorExecutionContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_roster: Option<MembersList>,
    pub observability: AtmObservabilityHealth,
    #[serde(default)]
    pub post_send: PostSendDoctorReport,
    pub config: ConfigDoctorReport,
    pub mail_store: MailStoreDoctorReport,
    pub roster_store: RosterStoreDoctorReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_runtime: Option<DaemonRuntimeDoctorReport>,
    #[serde(default)]
    pub drift_findings: Vec<DoctorFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<RuntimeStatusSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_trace: Option<BootstrapTraceReport>,
}

impl DoctorReport {
    pub fn has_errors(&self) -> bool {
        self.summary.status == DoctorStatus::Error
    }
}
