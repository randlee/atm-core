use crate::config::AtmConfig;
use crate::error::AtmError;
use crate::schema::{AgentMember, MessageEnvelope};
use crate::types::{AgentName, IsoTimestamp, PaneId, TeamName};
pub use atm_storage::contract::{RosterHarness, RosterMemberKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use super::mail::{DoctorFinding, MessageFingerprint};
use super::{ReplaySource, sealed};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshot {
    pub team: TeamName,
    pub member_count: u64,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<IsoTimestamp>,
}

pub type RosterMemberRecord = atm_storage::contract::RosterMember;

pub fn roster_member_record_from_claude_code_member(
    team_name: TeamName,
    member: AgentMember,
) -> RosterMemberRecord {
    let recipient_pane_id = member.tmux_pane_id;
    let mut metadata_json = member.extra;
    if !member.agent_id.is_empty() {
        metadata_json.insert(
            "agentId".to_string(),
            Value::String(member.agent_id.to_string()),
        );
    }
    if let Some(joined_at) = member.joined_at {
        metadata_json.insert(
            "joinedAt".to_string(),
            Value::Number(serde_json::Number::from(joined_at)),
        );
    }
    if !member.cwd.as_os_str().is_empty() {
        metadata_json.insert(
            "cwd".to_string(),
            Value::String(member.cwd.display().to_string()),
        );
    }

    RosterMemberRecord {
        team_name,
        agent_name: member.name,
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::ClaudeCode,
        agent_type: member.agent_type,
        model: member.model,
        recipient_pane_id,
        metadata_json,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRosterMember {
    pub member_name: AgentName,
    pub harness: RosterHarness,
    pub inbox_path: Option<PathBuf>,
    pub tmux_pane_id: Option<PaneId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRoster {
    pub team_name: TeamName,
    pub members: Arc<[ProjectionRosterMember]>,
}

impl ProjectionRoster {
    pub fn from_roster_snapshot(team_name: TeamName, records: &[RosterMemberRecord]) -> Self {
        let members = records
            .iter()
            .filter(|record| record.harness == RosterHarness::ClaudeCode)
            .map(|record| ProjectionRosterMember {
                member_name: record.agent_name.clone(),
                harness: record.harness,
                inbox_path: None,
                tmux_pane_id: record.recipient_pane_id.clone(),
            })
            .collect::<Vec<_>>()
            .into();
        Self { team_name, members }
    }

    pub fn contains_member(&self, member: &AgentName) -> bool {
        self.members
            .iter()
            .any(|entry| entry.member_name == *member)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RosterStoreDoctorReport {
    pub findings: Vec<DoctorFinding>,
}

/// Stub config-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigLoadRequest {
    pub current_dir: PathBuf,
}

/// Stub config-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLoadResponse {
    pub config: Option<AtmConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ConfigDoctorReport {
    pub findings: Vec<DoctorFinding>,
}

/// Imported source-file snapshot returned by inbox ingress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceFileRecord {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

/// Imported Claude source request for the daemon/private compatibility path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceImportRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Imported Claude source response for the daemon/private compatibility path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceImportResponse {
    pub source_files: Vec<SourceFileRecord>,
}

/// Claude source identity-fingerprint request for the daemon/private path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceIdentityFingerprintRequest {
    pub message: MessageEnvelope,
}

/// Claude source identity-fingerprint response for the daemon/private path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceIdentityFingerprintResponse {
    pub fingerprint: Option<MessageFingerprint>,
}

/// Claude source diagnostics request for the daemon/private path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceDiagnosticsRequest {
    pub source_files: Vec<SourceFileRecord>,
}

/// Claude source diagnostics response for the daemon/private path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceDiagnosticsResponse {
    pub duplicate_message_ids: usize,
    pub messages_without_ids: usize,
}

/// Claude projection-record request for the daemon/private compatibility path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionRecordRequest {
    pub source_files: Vec<SourceFileRecord>,
}

/// Claude projection-record response for the daemon/private compatibility path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionRecordResponse {
    pub committed_paths: usize,
}

/// Claude projection re-export request for the daemon/private compatibility path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionReexportMessageRequest {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

/// Claude projection re-export response for the daemon/private compatibility path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionReexportMessageResponse {
    pub wrote_messages: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionAppendMode {
    RecoveredLogicalMessageSet,
}

/// Explicit inbox-export append-message-set request for recovered Claude delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionAppendMessageSetRequest {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
    pub mode: ProjectionAppendMode,
}

/// Explicit inbox-export append-message-set response for recovered Claude delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionAppendMessageSetResponse {
    pub wrote_messages: usize,
}

/// Canonical non-Claude outbound request payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NonClaudeOutboundDeliveryRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub recipient_pane_id: Option<PaneId>,
    /// Payload serialized to JSONL must not exceed `MAX_NON_CLAUDE_PAYLOAD_BYTES` (1 MiB),
    /// enforced by `DaemonNonClaudeOutbound::deliver_payloads` (daemon path) and
    /// `LocalFileNonClaudeOutbound::deliver_payloads` (CLI path, see service_runtime.rs:218).
    pub messages: Vec<MessageEnvelope>,
}

/// Canonical non-Claude outbound response payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NonClaudeOutboundDeliveryResponse {
    pub delivered_messages: usize,
}

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
pub trait RosterStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when roster replacement cannot be applied safely.
    fn replace_roster(
        &self,
        team: &TeamName,
        members: &[RosterMemberRecord],
        source: Option<&ReplaySource>,
    ) -> Result<(), AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one roster snapshot cannot be loaded.
    fn load_roster(&self, team: &TeamName) -> Result<Vec<RosterMemberRecord>, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when membership cannot be queried.
    fn query_membership(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<Option<RosterMemberRecord>, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when the canonical roster team set cannot be
    /// enumerated safely.
    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when roster health cannot be collected.
    fn health_snapshot(&self, team: &TeamName) -> Result<RosterStoreHealthSnapshot, AtmError>;
}

/// BOUNDARY-RosterStoreDoctor — see docs/atm-core/boundaries.md.
pub trait RosterStoreDoctor: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when durable roster-store diagnostics cannot be
    /// collected or summarized into the shared doctor report shape.
    fn inspect_roster_store(&self) -> Result<RosterStoreDoctorReport, AtmError>;
}

/// BOUNDARY-ConfigIngress — see docs/atm-core/boundaries.md.
pub trait ConfigIngress: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when persisted ATM configuration cannot be loaded,
    /// parsed, or validated into typed models.
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError>;
}

/// BOUNDARY-ConfigDoctor — see docs/atm-core/boundaries.md.
pub trait ConfigDoctor: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when config diagnostics cannot be collected or
    /// summarized into the shared doctor report shape.
    fn inspect_config(&self) -> Result<ConfigDoctorReport, AtmError>;
}

/// BOUNDARY-SourceIngress — see docs/atm-core/boundaries.md.
pub trait SourceIngress: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when compatibility inbox material cannot be
    /// imported, fingerprinted, or diagnosed into ATM-owned state.
    fn import_inbox_source(
        &self,
        request: SourceImportRequest,
    ) -> Result<SourceImportResponse, AtmError>;
    fn compute_identity_fingerprint(
        &self,
        request: SourceIdentityFingerprintRequest,
    ) -> SourceIdentityFingerprintResponse;
    fn report_diagnostics(&self, request: SourceDiagnosticsRequest) -> SourceDiagnosticsResponse;
}

/// BOUNDARY-ProjectionExport — see docs/atm-core/boundaries.md.
pub trait ProjectionExport: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when ATM-owned state cannot be projected back to the
    /// compatibility inbox/export surfaces.
    fn export_record(
        &self,
        request: ProjectionRecordRequest,
    ) -> Result<ProjectionRecordResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when the message re-export cannot be materialized.
    fn reexport_message(
        &self,
        request: ProjectionReexportMessageRequest,
    ) -> Result<ProjectionReexportMessageResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when a recovered Claude logical message set cannot
    /// be materialized through one owned export operation.
    fn append_message_set(
        &self,
        request: ProjectionAppendMessageSetRequest,
    ) -> Result<ProjectionAppendMessageSetResponse, AtmError>;
}

/// BOUNDARY-NonClaudeOutbound — see docs/atm-core/boundaries.md.
pub trait NonClaudeOutbound: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when non-Claude logical payload delivery cannot be
    /// executed through the approved outbound boundary.
    fn deliver_payloads(
        &self,
        request: NonClaudeOutboundDeliveryRequest,
    ) -> Result<NonClaudeOutboundDeliveryResponse, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct WitnessRosterStoreDoctor;
    struct WitnessConfigDoctor;

    impl sealed::Sealed for WitnessRosterStoreDoctor {}
    impl sealed::Sealed for WitnessConfigDoctor {}

    impl RosterStoreDoctor for WitnessRosterStoreDoctor {
        fn inspect_roster_store(&self) -> Result<RosterStoreDoctorReport, AtmError> {
            Ok(RosterStoreDoctorReport::default())
        }
    }

    impl ConfigDoctor for WitnessConfigDoctor {
        fn inspect_config(&self) -> Result<ConfigDoctorReport, AtmError> {
            Ok(ConfigDoctorReport::default())
        }
    }

    #[test]
    fn roster_store_doctor_trait_is_object_safe_and_compiles() {
        fn assert_object_safe(_doctor: &dyn RosterStoreDoctor) {}

        let witness = WitnessRosterStoreDoctor;
        assert_object_safe(&witness);
    }

    #[test]
    fn config_doctor_trait_is_object_safe_and_compiles() {
        fn assert_object_safe(_doctor: &dyn ConfigDoctor) {}

        let witness = WitnessConfigDoctor;
        assert_object_safe(&witness);
    }
}
