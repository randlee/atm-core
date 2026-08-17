// This Phase AC boundary file mixes seams that are dormant in the library
// build with helpers still exercised by retained/test callers. Under
// `--all-targets`, `#[expect(dead_code)]` turns into
// `unfulfilled_lint_expectations` on the test-reachable subset before the
// later cutover/deletion work lands, so this branch intentionally keeps
// `allow(dead_code)` here.
#![allow(
    dead_code,
    reason = "AC.2 internalizes Claude-only storage seams before their later deletion or full consumer cutover."
)]

use crate::config::AtmConfig;
use crate::error::AtmError;
use crate::schema::{AgentMember, HOME_DIR_METADATA_KEY, InboxMessage};
use crate::types::{AgentName, IsoTimestamp, PaneId, TeamName};
pub use atm_storage::contract::{RosterHarness, RosterMemberKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use super::mail::DoctorFinding;
use super::sealed;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshot {
    pub team: TeamName,
    pub member_count: u64,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<IsoTimestamp>,
}

pub type RosterEntry = atm_storage::contract::RosterMember;

pub fn roster_member_record_from_claude_code_member(
    team_name: TeamName,
    member: AgentMember,
) -> RosterEntry {
    let harness = member
        .extra
        .get("harness")
        .and_then(Value::as_str)
        .and_then(roster_harness_from_metadata)
        .unwrap_or(RosterHarness::CodexCli);
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
    if !member.home_dir.is_empty() {
        metadata_json.insert(
            HOME_DIR_METADATA_KEY.to_string(),
            Value::String(member.home_dir.as_path().display().to_string()),
        );
    }

    RosterEntry {
        team_name,
        agent_name: member.name,
        member_kind: RosterMemberKind::Permanent,
        harness,
        agent_type: member.agent_type,
        model: member.model,
        recipient_pane_id,
        metadata_json,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedRosterEntry {
    pub member_name: AgentName,
    pub harness: RosterHarness,
    pub inbox_path: Option<PathBuf>,
    pub tmux_pane_id: Option<PaneId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionRoster {
    pub team_name: TeamName,
    pub members: Arc<[ProjectedRosterEntry]>,
}

impl ProjectionRoster {
    pub fn from_roster_snapshot(team_name: TeamName, records: &[RosterEntry]) -> Self {
        let members = records
            .iter()
            .map(|record| ProjectedRosterEntry {
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

fn roster_harness_from_metadata(value: &str) -> Option<RosterHarness> {
    match value {
        "claude-code" => Some(RosterHarness::ClaudeCode),
        "codex-cli" => Some(RosterHarness::CodexCli),
        "gemini-cli" => Some(RosterHarness::GeminiCli),
        "opencode" => Some(RosterHarness::Opencode),
        "hermes" => Some(RosterHarness::Hermes),
        "python-graft" => Some(RosterHarness::PythonGraft),
        _ => None,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionAppendMode {
    RecoveredLogicalMessageSet,
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
    pub messages: Vec<InboxMessage>,
}

/// Canonical non-Claude outbound response payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NonClaudeOutboundDeliveryResponse {
    pub delivered_messages: usize,
}

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
#[deprecated(note = "use canonical atm-storage types instead")]
pub trait RosterStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when roster replacement cannot be applied safely.
    fn replace_roster(&self, team: &TeamName, members: &[RosterEntry]) -> Result<(), AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one roster snapshot cannot be loaded.
    fn load_roster(&self, team: &TeamName) -> Result<Vec<RosterEntry>, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when membership cannot be queried.
    fn query_membership(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<Option<RosterEntry>, AtmError>;
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
