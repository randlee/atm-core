use crate::error::AtmError;
use crate::schema::{AtmMessageId, ThreadMode};
use crate::types::{AgentName, ChatId, IsoTimestamp, TaskId, TeamName};
use atm_storage::contract::MessageKey;
use serde::{Deserialize, Serialize};

use super::sealed;
pub use atm_storage::contract::{MailMessageState, MessageFingerprint};

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
    pub source_chat_id: Option<ChatId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_chat_id: Option<ChatId>,
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

/// BOUNDARY-MailStoreDoctor — see docs/atm-core/boundaries.md.
pub trait MailStoreDoctor: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when durable mail-store diagnostics cannot be
    /// collected or summarized into the shared doctor report shape.
    fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError>;
}
