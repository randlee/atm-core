use crate::error::AtmError;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{MessageKey, sealed};

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
    // TODO(P6): Introduce a validated ReplaySource newtype once the replay
    // boundary callers are consolidated. Keeping the raw String here avoids a
    // broad boundary fan-out in this integration fix while preserving the
    // documented non-empty source invariant at construction sites.
    /// Invariant: source must name one concrete ingest origin chosen by the
    /// caller (for example a file path or inbox export id) and must never be
    /// synthesized from an empty or whitespace-only string.
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
    // TODO(P6): Introduce a validated ReplaySource newtype once the replay
    // boundary callers are consolidated. Keeping the raw String here avoids a
    // broad boundary fan-out in this integration fix while preserving the
    // documented non-empty source invariant at construction sites.
    /// Invariant: source identifies one concrete ingest origin and must never
    /// be synthesized from an empty or whitespace-only string.
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
    // TODO(P6): Introduce a validated ReplaySource newtype once the replay
    // boundary callers are consolidated. Keeping the raw String here avoids a
    // broad boundary fan-out in this integration fix while preserving the
    // documented non-empty source invariant at construction sites.
    /// Invariant: source identifies one concrete ingest origin and must never
    /// be synthesized from an empty or whitespace-only string.
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
