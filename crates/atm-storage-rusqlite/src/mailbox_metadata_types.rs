//! Test-only mailbox metadata query shape.

use atm_storage::contract::MessageKey;
use atm_storage::schema::{AtmMessageId, ThreadMode};
use atm_storage::types::{AgentName, IsoTimestamp};

#[allow(
    dead_code,
    reason = "metadata positive-path fields are owned by the query DTO while current tests exercise malformed-row validation"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqliteMailboxMetadataRow {
    pub message_key: MessageKey,
    pub message_id: Option<AtmMessageId>,
    pub parent_message_id: Option<AtmMessageId>,
    pub thread_mode: Option<ThreadMode>,
    pub from_agent: AgentName,
    pub source_chat_id: Option<atm_storage::types::ChatId>,
    pub destination_chat_id: Option<atm_storage::types::ChatId>,
    pub summary: Option<String>,
    pub message_at: IsoTimestamp,
    pub read: bool,
    pub requires_ack: bool,
    pub pending_ack: bool,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub expires_at: Option<IsoTimestamp>,
    pub task_id: Option<atm_storage::types::TaskId>,
}
