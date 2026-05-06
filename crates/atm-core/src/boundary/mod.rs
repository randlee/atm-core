//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::protocol::{FramePayload, RequestEnvelope, ResponseEnvelope};
pub use crate::protocol::{
    NotificationEvent, ReconcileRequest, ReconcileResult, RuntimeStatusSnapshot, WatchEventBatch,
    WatchSubscriptionRequest,
};
use crate::schema::{AgentMember, MessageEnvelope, TeamConfig};
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Workspace-convention seal only; not compiler-enforced outside this crate.
///
/// Only ATM workspace crates may implement boundary traits. Enforced by
/// boundary lint, forbidden-edge rules, and review gates.
#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct MessageKey(String);

impl MessageKey {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the key is blank or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("message key must not be blank").with_recovery(
                    "Populate a stable ATM message key before calling the Phase R boundary.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for MessageKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for MessageKey {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TaskState(String);

impl TaskState {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the state is blank or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("task state must not be blank").with_recovery(
                    "Populate a non-empty task state before calling the Phase R boundary.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for TaskState {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for TaskState {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl PartialEq<&str> for TaskState {
    fn eq(&self, other: &&str) -> bool {
        self.as_ref() == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AckTransition(String);

impl AckTransition {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the transition is blank or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("ack transition must not be blank").with_recovery(
                    "Populate a non-empty ack transition before calling the Phase R boundary.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for AckTransition {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AckTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for AckTransition {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailStoreMessageRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub envelope: MessageEnvelope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreVisibilityState {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub message_key: MessageKey,
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreIngestReplayState {
    pub team: TeamName,
    pub agent: AgentName,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ingested_at: Option<IsoTimestamp>,
    #[serde(default)]
    pub ingested_rows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreHealthSnapshot {
    pub team: TeamName,
    pub agent: AgentName,
    pub total_messages: u64,
    pub pending_ack_messages: u64,
    pub read_messages: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_message_timestamp: Option<IsoTimestamp>,
}

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

/// Stub mail-store request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreBootstrapRequest {
    pub team_dir: PathBuf,
    pub team: TeamName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_config: Option<TeamConfig>,
}

/// Stub mail-store response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreBootstrapResponse {
    pub team: TeamName,
    pub bootstrapped: bool,
    pub opened: bool,
}

/// Stub mail-store transaction request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreTransactionRequest {
    pub team: TeamName,
    pub requested_operations: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Stub mail-store transaction response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreTransactionResponse {
    pub team: TeamName,
    pub committed: bool,
    pub operations_executed: usize,
}

/// Stub mail-store upsert-message request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailStoreUpsertMessageRequest {
    pub record: MailStoreMessageRecord,
}

/// Stub mail-store upsert-message response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailStoreUpsertMessageResponse {
    pub record: MailStoreMessageRecord,
    pub inserted: bool,
}

/// Stub mail-store load-message request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreLoadMessageRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
}

/// Stub mail-store load-message response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailStoreLoadMessageResponse {
    #[serde(default)]
    pub record: Option<MailStoreMessageRecord>,
}

/// Stub mail-store upsert-visibility request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreUpsertVisibilityStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub state: MailStoreVisibilityState,
}

/// Stub mail-store upsert-visibility response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreUpsertVisibilityStateResponse {
    pub state: MailStoreVisibilityState,
}

/// Stub mail-store load-visibility request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreLoadVisibilityStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub message_key: MessageKey,
}

/// Stub mail-store load-visibility response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreLoadVisibilityStateResponse {
    #[serde(default)]
    pub state: Option<MailStoreVisibilityState>,
}

/// Stub mail-store record-ingest-replay request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreRecordIngestReplayStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub source: String,
    pub state: MailStoreIngestReplayState,
}

/// Stub mail-store record-ingest-replay response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreRecordIngestReplayStateResponse {
    pub state: MailStoreIngestReplayState,
}

/// Stub mail-store load-ingest-replay request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreLoadIngestReplayStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub source: String,
}

/// Stub mail-store load-ingest-replay response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreLoadIngestReplayStateResponse {
    #[serde(default)]
    pub state: Option<MailStoreIngestReplayState>,
}

/// Stub mail-store health-snapshot request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreHealthSnapshotRequest {
    pub team: TeamName,
    pub agent: AgentName,
}

/// Stub mail-store health-snapshot response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreHealthSnapshotResponse {
    pub snapshot: MailStoreHealthSnapshot,
}

/// Canonical Phase R mail-store request entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreRequest {
    pub team_dir: PathBuf,
    pub team: TeamName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_config: Option<TeamConfig>,
}

/// Canonical Phase R mail-store response entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreResponse {
    pub team: TeamName,
    pub bootstrapped: bool,
    pub opened: bool,
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

/// Stub roster-store request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreReplaceRosterRequest {
    pub team: TeamName,
    pub roster: TeamConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
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
    pub roster: TeamConfig,
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
    pub member: Option<AgentMember>,
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

/// Canonical Phase R roster-store request entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreRequest {
    pub team: TeamName,
    pub roster: TeamConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Canonical Phase R roster-store response entrypoint payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterStoreResponse {
    pub team: TeamName,
    pub previous_member_count: u64,
    pub current_member_count: u64,
    pub replaced: bool,
}

/// Stub config-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigLoadRequest {
    pub current_dir: PathBuf,
}

/// Stub config-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLoadResponse {
    pub config: Option<crate::config::AtmConfig>,
}

/// Team-config load request for the Phase R config-ingress boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigTeamLoadRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
}

/// Team-config load response for the Phase R config-ingress boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigTeamLoadResponse {
    pub team_dir: PathBuf,
    pub team_config: TeamConfig,
}

/// Imported source-file snapshot returned by inbox ingress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxSourceFileRecord {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

/// Stub inbox-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxIngressImportRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub agent: AgentName,
}

/// Stub inbox-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxIngressImportResponse {
    pub source_files: Vec<InboxSourceFileRecord>,
}

/// Stub inbox-ingress identity-fingerprint request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxIngressIdentityFingerprintRequest {
    pub message: MessageEnvelope,
}

/// Stub inbox-ingress identity-fingerprint response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxIngressIdentityFingerprintResponse {
    pub fingerprint: Option<String>,
}

/// Stub inbox-ingress diagnostics request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxIngressDiagnosticsRequest {
    pub source_files: Vec<InboxSourceFileRecord>,
}

/// Stub inbox-ingress diagnostics response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxIngressDiagnosticsResponse {
    pub duplicate_legacy_message_ids: usize,
    pub messages_without_ids: usize,
}

/// Canonical Phase R inbox-ingress request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressRequest;

/// Canonical Phase R inbox-ingress response entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressResponse;

/// Stub inbox-export request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxExportRecordRequest {
    pub source_files: Vec<InboxSourceFileRecord>,
}

/// Stub inbox-export response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxExportRecordResponse {
    pub committed_paths: usize,
}

/// Stub inbox-export re-export request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxExportReexportMessageRequest {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

/// Stub inbox-export re-export response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxExportReexportMessageResponse {
    pub wrote_messages: usize,
}

/// Canonical Phase R inbox-export request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportRequest;

/// Canonical Phase R inbox-export response entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportResponse;

/// BOUNDARY-AtmProtocol — see docs/atm-core/boundaries.md.
pub trait AtmProtocol: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a protocol request envelope cannot be converted
    /// into a frame payload.
    fn request_to_frame(&self, request: RequestEnvelope) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol request envelope.
    fn request_from_frame(&self, frame: FramePayload) -> Result<RequestEnvelope, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a protocol response envelope cannot be
    /// converted into a frame payload.
    fn response_to_frame(&self, response: ResponseEnvelope) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol response envelope.
    fn response_from_frame(&self, frame: FramePayload) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-ClientTransport — see docs/atm-core/boundaries.md.
pub trait ClientTransport: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when the framed request cannot be delivered or when
    /// the peer returns an unrecoverable protocol response.
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-ServerTransport — see docs/atm-core/boundaries.md.
pub trait ServerTransport: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when framing, transport serving, or dispatch handoff
    /// cannot proceed reliably.
    fn serve(
        &self,
        dispatcher: std::sync::Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError>;
}

/// BOUNDARY-RequestDispatcher — see docs/atm-core/boundaries.md.
pub trait RequestDispatcher: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when protocol request routing or handler dispatch
    /// cannot produce a valid response.
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-NotificationSink — see docs/atm-core/boundaries.md.
pub trait NotificationSink: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when notification delivery cannot be executed.
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError>;
}

/// BOUNDARY-StatusSource — see docs/atm-core/boundaries.md.
pub trait StatusSource: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a runtime status snapshot cannot be collected.
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError>;
}

/// BOUNDARY-WatchEventSource — see docs/atm-core/boundaries.md.
pub trait WatchEventSource: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when watch subscriptions cannot be created or events
    /// cannot be delivered as a batch.
    fn poll(&self, request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError>;
}

/// BOUNDARY-ReconcileCoordinator — see docs/atm-core/boundaries.md.
pub trait ReconcileCoordinator: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when reconcile policy cannot be executed for the
    /// request input.
    fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError>;
}

/// BOUNDARY-MailStore — see docs/atm-core/boundaries.md.
pub trait MailStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when durable mailbox persistence, transaction
    /// boundaries, or replay-state access cannot satisfy the contract.
    fn bootstrap(
        &self,
        request: MailStoreBootstrapRequest,
    ) -> Result<MailStoreBootstrapResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a mailbox transaction cannot run.
    fn run_transaction(
        &self,
        request: MailStoreTransactionRequest,
    ) -> Result<MailStoreTransactionResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the mailbox transaction cannot be started,
    /// executed, or committed safely.
    fn upsert_message(
        &self,
        request: MailStoreUpsertMessageRequest,
    ) -> Result<MailStoreUpsertMessageResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested message cannot be loaded.
    fn load_message(
        &self,
        request: MailStoreLoadMessageRequest,
    ) -> Result<MailStoreLoadMessageResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when visibility state persistence fails.
    fn upsert_visibility_state(
        &self,
        request: MailStoreUpsertVisibilityStateRequest,
    ) -> Result<MailStoreUpsertVisibilityStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when visibility state cannot be loaded.
    fn load_visibility_state(
        &self,
        request: MailStoreLoadVisibilityStateRequest,
    ) -> Result<MailStoreLoadVisibilityStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when ingest-replay state persistence fails.
    fn record_ingest_replay_state(
        &self,
        request: MailStoreRecordIngestReplayStateRequest,
    ) -> Result<MailStoreRecordIngestReplayStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when ingest-replay state cannot be loaded.
    fn load_ingest_replay_state(
        &self,
        request: MailStoreLoadIngestReplayStateRequest,
    ) -> Result<MailStoreLoadIngestReplayStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a mailbox health snapshot cannot be read.
    fn health_snapshot(
        &self,
        request: MailStoreHealthSnapshotRequest,
    ) -> Result<MailStoreHealthSnapshotResponse, AtmError>;
}

/// BOUNDARY-TaskStore — see docs/atm-core/boundaries.md.
pub trait TaskStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when durable task-state persistence or task/message
    /// linkage updates fail to satisfy the contract.
    fn create_task(
        &self,
        request: TaskStoreCreateTaskRequest,
    ) -> Result<TaskStoreCreateTaskResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested task cannot be loaded.
    fn load_task(
        &self,
        request: TaskStoreLoadTaskRequest,
    ) -> Result<TaskStoreLoadTaskResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested task cannot be updated.
    fn update_task(
        &self,
        request: TaskStoreUpdateTaskRequest,
    ) -> Result<TaskStoreUpdateTaskResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a message link cannot be attached.
    fn attach_message_link(
        &self,
        request: TaskStoreAttachMessageLinkRequest,
    ) -> Result<TaskStoreAttachMessageLinkResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a message link cannot be detached.
    fn detach_message_link(
        &self,
        request: TaskStoreDetachMessageLinkRequest,
    ) -> Result<TaskStoreDetachMessageLinkResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when an ack transition cannot be recorded.
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

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
pub trait RosterStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when durable roster persistence or membership
    /// queries fail to satisfy the contract.
    fn replace_roster(
        &self,
        request: RosterStoreReplaceRosterRequest,
    ) -> Result<RosterStoreReplaceRosterResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a roster snapshot cannot be loaded.
    fn load_roster(
        &self,
        request: RosterStoreLoadRosterRequest,
    ) -> Result<RosterStoreLoadRosterResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when roster membership cannot be queried.
    fn query_membership(
        &self,
        request: RosterStoreQueryMembershipRequest,
    ) -> Result<RosterStoreQueryMembershipResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a roster health snapshot cannot be collected.
    fn health_snapshot(
        &self,
        request: RosterStoreHealthSnapshotRequest,
    ) -> Result<RosterStoreHealthSnapshotResponse, AtmError>;
}

/// BOUNDARY-ConfigIngress — see docs/atm-core/boundaries.md.
pub trait ConfigIngress: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when persisted ATM/team configuration cannot be
    /// loaded, parsed, or validated into typed models.
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when one team config cannot be loaded or validated.
    fn load_team_config(
        &self,
        request: ConfigTeamLoadRequest,
    ) -> Result<ConfigTeamLoadResponse, AtmError>;
}

/// BOUNDARY-InboxIngress — see docs/atm-core/boundaries.md.
pub trait InboxIngress: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when compatibility inbox material cannot be
    /// imported, fingerprinted, or diagnosed into ATM-owned state.
    fn import_inbox_source(
        &self,
        request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when identity fingerprinting cannot be computed.
    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when inbox diagnostics cannot be generated.
    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError>;
}

/// BOUNDARY-InboxExport — see docs/atm-core/boundaries.md.
pub trait InboxExport: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when ATM-owned state cannot be projected back to the
    /// compatibility inbox/export surfaces.
    fn export_record(
        &self,
        request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the message re-export cannot be materialized.
    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError>;
}
