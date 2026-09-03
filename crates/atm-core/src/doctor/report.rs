use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::boundary::{ConfigDoctorReport, MailStoreDoctorReport, RosterStoreDoctorReport};
use crate::error_codes::AtmErrorCode;
use crate::observability::AtmObservabilityHealth;
use crate::peer_wire::PeerWireSecurity;
use crate::protocol::{ReleaseVersion, RuntimeStatusSnapshot};
use crate::team_admin::MembersList;
use crate::types::{AgentName, TeamName};

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
    /// The immutable peer-wire launch policy selected by this daemon process.
    /// It is diagnostic-only and never includes a certificate, pin, or key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_wire_security: Option<PeerWireSecurityStatus>,
}

/// Typed, public diagnostic projection of the daemon's peer-wire policy.
///
/// This preserves the established JSON launch spellings while preventing a
/// doctor caller from treating an arbitrary string as a valid security mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PeerWireSecurityStatus {
    MutualTls,
    PlaintextTest,
}

impl From<PeerWireSecurity> for PeerWireSecurityStatus {
    fn from(value: PeerWireSecurity) -> Self {
        match value {
            PeerWireSecurity::Mtls => Self::MutualTls,
            PeerWireSecurity::PlaintextTest => Self::PlaintextTest,
        }
    }
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
    /// Legacy literal-IP trusted-peer rows (enabled or disabled) with their
    /// exact safe migration/retirement remediation. See issue #972: a
    /// disabled row never blocks startup and an enabled row fails startup
    /// closed unless the operator opts into the testing/benchmarking-only
    /// skip; both cases are surfaced here so `atm doctor` reports them
    /// before/at upgrade rather than only at daemon launch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy_literal_ip_peers: Vec<LegacyLiteralIpPeerDoctorReport>,
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

/// Safe projection of one legacy literal-IP trusted-peer row, carrying the
/// exact commands that migrate or retire it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyLiteralIpPeerDoctorReport {
    pub host: String,
    pub enabled: bool,
    /// Converts this row to a durable hostname, keeping its fingerprint and
    /// port. `<hostname>` is a placeholder for the operator's chosen name.
    pub migrate_command: String,
    /// Retires this row from the trusted-peer catalog.
    pub revoke_command: String,
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

/// Safe projection of one registry-backed graft receiver lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftReceiverLeaseDoctorReport {
    pub team: TeamName,
    pub agent: AgentName,
    pub endpoint: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_seen_age_seconds: i64,
    pub reachable_at_last_use: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GraftReceiversDoctorReport {
    pub receivers: Vec<GraftReceiverLeaseDoctorReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HerdrBreakerDoctorState {
    Closed,
    Open,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HerdrBreakerDoctorReport {
    pub state: HerdrBreakerDoctorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consecutive_failures: Option<u32>,
}

impl Default for HerdrBreakerDoctorReport {
    fn default() -> Self {
        Self {
            state: HerdrBreakerDoctorState::Closed,
            retry_after_ms: None,
            consecutive_failures: None,
        }
    }
}

pub trait HerdrBreakerDoctor: Send + Sync {
    fn report(&self) -> HerdrBreakerDoctorReport;
}

#[derive(Debug, Default)]
pub struct ClosedHerdrBreakerDoctor;

impl HerdrBreakerDoctor for ClosedHerdrBreakerDoctor {
    fn report(&self) -> HerdrBreakerDoctorReport {
        HerdrBreakerDoctorReport::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HerdrQueuePumpDoctorReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tick_at: Option<crate::types::IsoTimestamp>,
    #[serde(default)]
    pub breaker: HerdrBreakerDoctorReport,
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
    #[serde(default)]
    pub graft_receivers: GraftReceiversDoctorReport,
    pub observability: AtmObservabilityHealth,
    #[serde(default)]
    pub herdr_breaker: HerdrBreakerDoctorReport,
    #[serde(default)]
    pub herdr_queue_pump: HerdrQueuePumpDoctorReport,
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

#[cfg(test)]
mod tests {
    use super::PeerWireSecurityStatus;
    use crate::peer_wire::PeerWireSecurity;

    #[test]
    fn peer_wire_security_status_is_typed_and_preserves_public_json_values() {
        assert_eq!(
            PeerWireSecurityStatus::from(PeerWireSecurity::Mtls),
            PeerWireSecurityStatus::MutualTls
        );
        assert_eq!(
            serde_json::to_string(&PeerWireSecurityStatus::PlaintextTest)
                .expect("diagnostic status serializes"),
            "\"plaintext-test\""
        );
    }
}
