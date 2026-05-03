use std::path::PathBuf;

use crate::schema::{AtmMessageId, LegacyMessageId};
use crate::store::{
    InsertOutcome, MessageKey, RecipientPaneId, SourceFingerprint, StoreBoundary, StoreError,
};
use crate::types::{AgentName, IsoTimestamp, TeamName};

/// Canonical durable source family for a stored message row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSourceKind {
    Atm,
    Legacy,
    External,
}

impl MessageSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Atm => "atm",
            Self::Legacy => "legacy",
            Self::External => "external",
        }
    }
}

/// Canonical durable message row stored in SQLite Phase Q mail state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessageRecord {
    pub message_key: MessageKey,
    pub team_name: TeamName,
    pub recipient_agent: AgentName,
    pub sender_display: String,
    pub sender_canonical: Option<AgentName>,
    pub sender_team: Option<TeamName>,
    pub body: String,
    pub summary: Option<String>,
    pub created_at: IsoTimestamp,
    pub source_kind: MessageSourceKind,
    pub legacy_message_id: Option<LegacyMessageId>,
    pub atm_message_id: Option<AtmMessageId>,
    pub raw_metadata_json: Option<String>,
}

/// Durable imported-inbox dedupe record keyed by source fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRecord {
    pub team_name: TeamName,
    pub recipient_agent: AgentName,
    pub source_path: PathBuf,
    pub source_fingerprint: SourceFingerprint,
    pub message_key: MessageKey,
    pub imported_at: IsoTimestamp,
}

/// Durable acknowledgement state keyed by canonical message identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckStateRecord {
    pub message_key: MessageKey,
    pub pending_ack_at: Option<IsoTimestamp>,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub ack_reply_message_key: Option<MessageKey>,
    pub ack_reply_team: Option<TeamName>,
    pub ack_reply_agent: Option<AgentName>,
}

/// Durable read/clear visibility state keyed by canonical message identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibilityStateRecord {
    pub message_key: MessageKey,
    pub read_at: Option<IsoTimestamp>,
    pub cleared_at: Option<IsoTimestamp>,
}

/// Phase Q durable export replay entry.
///
/// Implementations must commit the originating message row before exporting to
/// a Claude inbox projection. Re-export/replay is keyed by `message_key` and
/// remains durable until bounded retry expiry removes the pending work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingExportRecord {
    pub message_key: MessageKey,
    pub export_target_team: TeamName,
    pub export_target_agent: AgentName,
    pub recipient_pane_id: Option<RecipientPaneId>,
    pub attempt_count: u32,
    pub next_attempt_at: IsoTimestamp,
    pub expires_at: IsoTimestamp,
}

/// Readiness snapshot for the durable mail-store tables owned by `MailStore`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailStoreHealth {
    pub messages_ready: bool,
    pub inbox_ingest_ready: bool,
    pub ack_state_ready: bool,
    pub message_visibility_ready: bool,
    pub pending_exports_ready: bool,
}

/// Durable message store boundary. Direct SQLite calls must stay in the
/// `atm-rusqlite` crate; higher layers work only through this trait.
pub trait MailStore: StoreBoundary {
    fn insert_message(
        &self,
        message: &StoredMessageRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError>;

    fn insert_message_batch(&self, messages: &[StoredMessageRecord]) -> Result<(), StoreError>;

    fn load_message(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<StoredMessageRecord>, StoreError>;

    fn load_message_by_legacy_id(
        &self,
        legacy_message_id: &LegacyMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError>;

    fn load_message_by_atm_id(
        &self,
        atm_message_id: &AtmMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError>;

    fn upsert_ack_state(&self, ack_state: &AckStateRecord) -> Result<AckStateRecord, StoreError>;

    fn load_ack_state(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<AckStateRecord>, StoreError>;

    fn upsert_visibility(
        &self,
        visibility: &VisibilityStateRecord,
    ) -> Result<VisibilityStateRecord, StoreError>;

    fn load_visibility(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<VisibilityStateRecord>, StoreError>;

    fn record_ingest(
        &self,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<IngestRecord>, StoreError>;

    fn insert_message_with_ingest(
        &self,
        message: &StoredMessageRecord,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError>;

    fn load_ingest(
        &self,
        team_name: &TeamName,
        recipient_agent: &AgentName,
        source_fingerprint: &SourceFingerprint,
    ) -> Result<Option<IngestRecord>, StoreError>;

    fn record_pending_export(&self, export: &PendingExportRecord) -> Result<(), StoreError>;

    fn remove_pending_export(&self, message_key: &MessageKey) -> Result<(), StoreError>;

    fn load_due_pending_exports(
        &self,
        now: &IsoTimestamp,
        limit: usize,
    ) -> Result<Vec<PendingExportRecord>, StoreError>;

    fn remove_expired_pending_exports(&self, now: &IsoTimestamp) -> Result<u64, StoreError>;

    fn mail_health(&self) -> Result<MailStoreHealth, StoreError>;
}
