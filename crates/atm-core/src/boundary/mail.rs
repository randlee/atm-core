use crate::error::AtmError;
use crate::schema::{AtmMessageId, MessageEnvelope, TeamConfig, ThreadMode};
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use super::{MessageKey, sealed};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailStoreMessageRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailMessageState {
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
    pub expires_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<IsoTimestamp>,
}

/// Opaque hash or content-addressable identifier that marks the last
/// successfully ingested message boundary for a replay source. Used by
/// incremental ingest workflows to resume without re-processing already-seen
/// messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageFingerprint(pub String);

impl std::fmt::Display for MessageFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MessageFingerprint {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl AsRef<str> for MessageFingerprint {
    fn as_ref(&self) -> &str {
        &self.0
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
    pub pending_ack: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreQueryMailboxMetadataRequest {
    pub team: TeamName,
    pub agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreQueryMailboxMetadataResponse {
    pub rows: Vec<MailStoreMailboxMetadataRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreMailboxMetadataCounts {
    pub total_messages: u64,
    pub unread_message_count: u64,
    pub pending_ack_messages: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreQueryMailboxMetadataCountsRequest {
    pub team: TeamName,
    pub agent: AgentName,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreQueryMailboxMetadataCountsResponse {
    pub counts: MailStoreMailboxMetadataCounts,
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

/// Stub mail-store load-stored-message request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreLoadStoredMessageRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
}

/// Stub mail-store load-stored-message response for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailStoreLoadStoredMessageResponse {
    #[serde(default)]
    pub record: Option<MailStoreMessageRecord>,
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

/// Stub mail-store record-ingest-replay request for the Phase R skeleton.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStoreRecordIngestReplayStateRequest {
    pub team: TeamName,
    pub agent: AgentName,
    /// Invariant: source identifies one concrete ingest origin and must never
    /// be synthesized from an empty or whitespace-only string.
    pub source: ReplaySource,
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
    /// Invariant: source identifies one concrete ingest origin and must never
    /// be synthesized from an empty or whitespace-only string.
    pub source: ReplaySource,
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

pub type DoctorFinding = crate::doctor::DoctorFinding;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MailStoreDoctorReport {
    pub findings: Vec<DoctorFinding>,
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
    /// Returns `AtmError` when the requested stored message cannot be loaded.
    fn load_stored_message(
        &self,
        request: MailStoreLoadStoredMessageRequest,
    ) -> Result<MailStoreLoadStoredMessageResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when mailbox metadata rows cannot be queried.
    fn query_mailbox_metadata(
        &self,
        request: MailStoreQueryMailboxMetadataRequest,
    ) -> Result<MailStoreQueryMailboxMetadataResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when mailbox metadata counts cannot be queried.
    fn query_mailbox_metadata_counts(
        &self,
        request: MailStoreQueryMailboxMetadataCountsRequest,
    ) -> Result<MailStoreQueryMailboxMetadataCountsResponse, AtmError>;

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

/// BOUNDARY-MailStoreDoctor — see docs/atm-core/boundaries.md.
pub trait MailStoreDoctor: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when durable mail-store diagnostics cannot be
    /// collected or summarized into the shared doctor report shape.
    fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::DoctorSeverity;

    struct WitnessMailStoreDoctor;

    impl sealed::Sealed for WitnessMailStoreDoctor {}

    impl MailStoreDoctor for WitnessMailStoreDoctor {
        fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError> {
            Ok(MailStoreDoctorReport::default())
        }
    }

    #[test]
    fn mail_store_doctor_trait_is_object_safe_and_compiles() {
        fn assert_object_safe(_doctor: &dyn MailStoreDoctor) {}

        let witness = WitnessMailStoreDoctor;
        assert_object_safe(&witness);
    }

    #[test]
    fn canonical_doctor_finding_round_trips_json() {
        let finding = DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: crate::error_codes::AtmErrorCode::WarningIdentityDrift,
            message: "identity drift".to_string(),
            remediation: Some("refresh ATM_IDENTITY".to_string()),
        };

        let json = serde_json::to_string(&finding).expect("serialize doctor finding");
        let round_trip: crate::doctor::DoctorFinding =
            serde_json::from_str(&json).expect("deserialize doctor finding");

        assert_eq!(round_trip, finding);
    }
}
