use crate::error::{AtmError, AtmErrorCode};
use crate::protocol::RequestEnvelope;
use crate::schema::AtmMessageId;
use crate::types::{AgentName, IsoTimestamp, TeamName};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use super::{MessageKey, sealed};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteReplayStateRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub peer_addr: SocketAddr,
    pub request: RequestEnvelope,
    pub recorded_at: IsoTimestamp,
    pub expires_at: IsoTimestamp,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<AtmErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_sender_team: Option<TeamName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_sender_agent: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_message_id: Option<AtmMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_remote_host: Option<String>,
}

/// BOUNDARY-RemoteReplayStore — see docs/atm-core/boundaries.md.
pub trait RemoteReplayStore: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when replay persistence cannot record a bounded
    /// remote-delivery retry state.
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the bounded retained replay set cannot be
    /// loaded for resume or inspection.
    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when one bounded retained replay record cannot be
    /// deleted after successful delivery.
    fn delete(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when expired bounded replay records cannot be
    /// purged safely.
    fn purge_expired(&self, now: IsoTimestamp) -> Result<usize, AtmError>;
}

/// BOUNDARY-RuntimeStorageFinalizer — see docs/atm-core/boundaries.md.
pub trait RuntimeStorageFinalizer: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when runtime-owned storage cannot finalize its
    /// bounded shutdown maintenance.
    fn finalize_storage_shutdown(&self) -> Result<(), AtmError>;
}
