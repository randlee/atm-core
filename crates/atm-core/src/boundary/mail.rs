use crate::error::AtmError;
use crate::schema::{AtmMessageId, ThreadMode};
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use atm_storage::contract::{Message, MessageKey};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::sealed;
pub use atm_storage::contract::{MailMessageState, MessageFingerprint};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct ReplaySource(String);

impl ReplaySource {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(AtmError::validation(
                "replay source must not be empty or whitespace-only",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReplaySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ReplaySource {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ReplaySource> for String {
    fn from(value: ReplaySource) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreIngestReplayState {
    pub team: TeamName,
    pub agent: AgentName,
    /// Invariant: source must name one concrete ingest origin chosen by the
    /// caller (for example a file path or inbox export id) and must never be
    /// synthesized from an empty or whitespace-only string.
    pub source: ReplaySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fingerprint: Option<MessageFingerprint>,
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
    pub read_message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_message_timestamp: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreMailboxMetadataRow {
    pub message_key: MessageKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<AtmMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<AtmMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_mode: Option<ThreadMode>,
    pub from_agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub message_at: IsoTimestamp,
    pub read: bool,
    pub requires_ack: bool,
    pub pending_ack: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreMailboxMetadataCounts {
    pub total_messages: u64,
    pub unread_message_count: u64,
    pub pending_ack_messages: u64,
}

/// Stub mail-store upsert-message-state request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpsertMailMessageStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub state: MailMessageState,
}

/// Stub mail-store upsert-message-state response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpsertMailMessageStateResponse {
    pub state: MailMessageState,
}

/// Stub mail-store load-message-state request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadMailMessageStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub message_key: MessageKey,
}

/// Stub mail-store load-message-state response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadMailMessageStateResponse {
    #[serde(default)]
    pub state: Option<MailMessageState>,
}

pub type DoctorFinding = crate::doctor::DoctorFinding;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MailStoreDoctorReport {
    pub findings: Vec<DoctorFinding>,
}

/// BOUNDARY-MailStore — see docs/atm-core/boundaries.md.
#[deprecated(note = "use canonical atm-storage types instead")]
pub trait MailStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when the mailbox transaction cannot be started,
    /// executed, or committed safely.
    fn upsert_message(&self, record: Message) -> Result<(), AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested message cannot be loaded.
    fn load_message(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<Option<Message>, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when mailbox metadata rows cannot be queried.
    fn query_mailbox_metadata(
        &self,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<MailStoreMailboxMetadataRow>, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when mailbox metadata counts cannot be queried.
    fn query_mailbox_metadata_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<MailStoreMailboxMetadataCounts, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when message-state persistence fails.
    fn upsert_message_state(
        &self,
        request: UpsertMailMessageStateRequest,
    ) -> Result<UpsertMailMessageStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when message state cannot be loaded.
    fn load_message_state(
        &self,
        request: LoadMailMessageStateRequest,
    ) -> Result<LoadMailMessageStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when ingest-replay state persistence fails.
    fn record_ingest_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        source: &ReplaySource,
        state: &MailStoreIngestReplayState,
    ) -> Result<(), AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when ingest-replay state cannot be loaded.
    fn load_ingest_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        source: &ReplaySource,
    ) -> Result<Option<MailStoreIngestReplayState>, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a mailbox health snapshot cannot be read.
    fn health_snapshot(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<MailStoreHealthSnapshot, AtmError>;
}

/// BOUNDARY-MailStoreDoctor — see docs/atm-core/boundaries.md.
pub trait MailStoreDoctor: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when durable mail-store diagnostics cannot be
    /// collected or summarized into the shared doctor report shape.
    fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError>;
}
