#![allow(
    dead_code,
    reason = "AC.2 internalizes Claude-only storage seams before their later deletion or full consumer cutover."
)]

use crate::config::AtmConfig;
use crate::error::AtmError;
use crate::schema::AgentType;
use crate::schema::{AgentMember, MessageEnvelope};
use crate::types::{AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};
use atm_storage::contract::{AckTransition, MessageKey, TaskState};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::mail::DoctorFinding;
use super::{ReplaySource, sealed};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskStoreTaskMetadata {
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreTaskRecord {
    pub team: TeamName,
    pub task_id: TaskId,
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_message_keys: Vec<MessageKey>,
    #[serde(default)]
    pub metadata: TaskStoreTaskMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshot {
    pub team: TeamName,
    pub member_count: u64,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterMemberKind {
    Permanent,
    Ephemeral,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterHarness {
    ClaudeCode,
    CodexCli,
    GeminiCli,
    Opencode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterMemberRecord {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub member_kind: RosterMemberKind,
    pub harness: RosterHarness,
    #[serde(default)]
    pub agent_type: AgentType,
    #[serde(default)]
    pub model: ModelName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_pane_id: Option<PaneId>,
    #[serde(default)]
    pub metadata_json: Map<String, Value>,
}

impl RosterMemberRecord {
    pub fn from_claude_code_member(team_name: TeamName, member: AgentMember) -> Self {
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

        Self {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionRosterMember {
    pub member_name: AgentName,
    pub harness: RosterHarness,
    pub inbox_path: Option<PathBuf>,
    pub tmux_pane_id: Option<PaneId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionRoster {
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

/// Stub task-store request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreCreateTaskRequest {
    pub team: TeamName,
    pub record: TaskStoreTaskRecord,
}

/// Stub task-store response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreCreateTaskResponse {
    pub record: TaskStoreTaskRecord,
}

/// Stub task-store load-task request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreLoadTaskRequest {
    pub team: TeamName,
    pub task_id: TaskId,
}

/// Stub task-store load-task response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreLoadTaskResponse {
    #[serde(default)]
    pub record: Option<TaskStoreTaskRecord>,
}

/// Stub task-store update-task request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreUpdateTaskRequest {
    pub team: TeamName,
    pub task_id: TaskId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TaskState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TaskStoreTaskMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub append_message_keys: Vec<MessageKey>,
}

/// Stub task-store update-task response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreUpdateTaskResponse {
    pub record: TaskStoreTaskRecord,
}

/// Stub task-store attach-message-link request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreAttachMessageLinkRequest {
    pub team: TeamName,
    pub task_id: TaskId,
    pub message_key: MessageKey,
}

/// Stub task-store attach-message-link response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreAttachMessageLinkResponse {
    pub record: TaskStoreTaskRecord,
}

/// Stub task-store detach-message-link request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreDetachMessageLinkRequest {
    pub team: TeamName,
    pub task_id: TaskId,
    pub message_key: MessageKey,
}

/// Stub task-store detach-message-link response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreDetachMessageLinkResponse {
    pub record: TaskStoreTaskRecord,
}

/// Stub task-store record-ack-transition request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreRecordAckTransitionRequest {
    pub team: TeamName,
    pub task_id: TaskId,
    pub message_key: MessageKey,
    pub actor: AgentName,
    pub transitioned_at: IsoTimestamp,
    pub transition: AckTransition,
}

/// Stub task-store record-ack-transition response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreRecordAckTransitionResponse {
    pub record: TaskStoreTaskRecord,
}

/// Stub task-store query-task-metadata request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreQueryTaskMetadataRequest {
    pub team: TeamName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_key: Option<MessageKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TaskState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Stub task-store query-task-metadata response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreQueryTaskMetadataResponse {
    pub records: Vec<TaskStoreTaskRecord>,
}

/// Canonical Phase R task-store request entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreRequest {
    pub team: TeamName,
    pub record: TaskStoreTaskRecord,
}

/// Canonical Phase R task-store response entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStoreResponse {
    pub record: TaskStoreTaskRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskStoreDoctorReport {
    pub findings: Vec<DoctorFinding>,
}

/// Stub roster-store request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreReplaceRosterRequest {
    pub team: TeamName,
    pub members: Vec<RosterMemberRecord>,
    /// Invariant: when present, source names one concrete roster-ingest origin
    /// and must not be synthesized from an empty or whitespace-only string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ReplaySource>,
}

/// Stub roster-store response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreReplaceRosterResponse {
    pub team: TeamName,
    pub previous_member_count: u64,
    pub current_member_count: u64,
    pub replaced: bool,
}

/// Stub roster-store load-roster request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreLoadRosterRequest {
    pub team: TeamName,
}

/// Stub roster-store load-roster response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreLoadRosterResponse {
    pub team: TeamName,
    pub members: Vec<RosterMemberRecord>,
}

/// Stub roster-store query-membership request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreQueryMembershipRequest {
    pub team: TeamName,
    pub member: AgentName,
}

/// Stub roster-store query-membership response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreQueryMembershipResponse {
    pub team: TeamName,
    #[serde(default)]
    pub member: Option<RosterMemberRecord>,
    #[serde(default)]
    pub is_member: bool,
}

/// Stub roster-store health-snapshot request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshotRequest {
    pub team: TeamName,
}

/// Stub roster-store health-snapshot response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshotResponse {
    pub snapshot: RosterStoreHealthSnapshot,
}

/// Canonical roster-store list-teams request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreListTeamsRequest;

/// Canonical roster-store list-teams response payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreListTeamsResponse {
    pub teams: Vec<TeamName>,
}

/// Canonical Phase R roster-store request entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreRequest {
    pub team: TeamName,
    pub members: Vec<RosterMemberRecord>,
    /// Invariant: when present, source names one concrete roster-ingest origin
    /// and must not be synthesized from an empty or whitespace-only string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ReplaySource>,
}

/// Canonical Phase R roster-store response entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreResponse {
    pub team: TeamName,
    pub previous_member_count: u64,
    pub current_member_count: u64,
    pub replaced: bool,
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

pub(crate) use atm_storage::compat::ProjectionAppendMode;

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

/// BOUNDARY-TaskStore — see docs/atm-core/boundaries.md.
pub trait TaskStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when task-state persistence or task-link mutation
    /// cannot satisfy the durable task-store contract.
    fn create_task(
        &self,
        request: TaskStoreCreateTaskRequest,
    ) -> Result<TaskStoreCreateTaskResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one task record cannot be loaded.
    fn load_task(
        &self,
        request: TaskStoreLoadTaskRequest,
    ) -> Result<TaskStoreLoadTaskResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one task record cannot be updated safely.
    fn update_task(
        &self,
        request: TaskStoreUpdateTaskRequest,
    ) -> Result<TaskStoreUpdateTaskResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one task/message link cannot be recorded.
    fn attach_message_link(
        &self,
        request: TaskStoreAttachMessageLinkRequest,
    ) -> Result<TaskStoreAttachMessageLinkResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one task/message link cannot be removed.
    fn detach_message_link(
        &self,
        request: TaskStoreDetachMessageLinkRequest,
    ) -> Result<TaskStoreDetachMessageLinkResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one ack transition cannot be persisted.
    fn record_ack_transition(
        &self,
        request: TaskStoreRecordAckTransitionRequest,
    ) -> Result<TaskStoreRecordAckTransitionResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when task metadata cannot be queried.
    fn query_task_metadata(
        &self,
        request: TaskStoreQueryTaskMetadataRequest,
    ) -> Result<TaskStoreQueryTaskMetadataResponse, AtmError>;
}

/// BOUNDARY-TaskStoreDoctor — see docs/atm-core/boundaries.md.
pub trait TaskStoreDoctor: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when durable task-store diagnostics cannot be
    /// collected or summarized into the shared doctor report shape.
    fn inspect_task_store(&self) -> Result<TaskStoreDoctorReport, AtmError>;
}

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
pub trait RosterStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when roster replacement cannot be applied safely.
    fn replace_roster(
        &self,
        request: RosterStoreReplaceRosterRequest,
    ) -> Result<RosterStoreReplaceRosterResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when one roster snapshot cannot be loaded.
    fn load_roster(
        &self,
        request: RosterStoreLoadRosterRequest,
    ) -> Result<RosterStoreLoadRosterResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when membership cannot be queried.
    fn query_membership(
        &self,
        request: RosterStoreQueryMembershipRequest,
    ) -> Result<RosterStoreQueryMembershipResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when the canonical roster team set cannot be
    /// enumerated safely.
    fn list_teams(
        &self,
        request: RosterStoreListTeamsRequest,
    ) -> Result<RosterStoreListTeamsResponse, AtmError>;
    /// # Errors
    ///
    /// Returns `AtmError` when roster health cannot be collected.
    fn health_snapshot(
        &self,
        request: RosterStoreHealthSnapshotRequest,
    ) -> Result<RosterStoreHealthSnapshotResponse, AtmError>;
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

    struct WitnessTaskStoreDoctor;
    struct WitnessRosterStoreDoctor;
    struct WitnessConfigDoctor;

    impl sealed::Sealed for WitnessTaskStoreDoctor {}
    impl sealed::Sealed for WitnessRosterStoreDoctor {}
    impl sealed::Sealed for WitnessConfigDoctor {}

    impl TaskStoreDoctor for WitnessTaskStoreDoctor {
        fn inspect_task_store(&self) -> Result<TaskStoreDoctorReport, AtmError> {
            Ok(TaskStoreDoctorReport::default())
        }
    }

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
    fn task_store_doctor_trait_is_object_safe_and_compiles() {
        fn assert_object_safe(_doctor: &dyn TaskStoreDoctor) {}

        let witness = WitnessTaskStoreDoctor;
        assert_object_safe(&witness);
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
