use crate::error::{AtmError, AtmErrorCode};
use crate::protocol::RequestEnvelope;
use crate::types::{AgentName, IsoTimestamp, TeamName};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use super::{
    ConfigDoctor, MailStore, MailStoreDoctor, MessageKey, RosterStore, RosterStoreDoctor,
    TaskStore, TaskStoreDoctor, sealed,
};

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
}

#[derive(Clone)]
pub struct RuntimeBundle {
    pub mail_store: Arc<dyn MailStore + Send + Sync>,
    pub task_store: Arc<dyn TaskStore + Send + Sync>,
    pub roster_store: Arc<dyn RosterStore + Send + Sync>,
    pub mail_store_doctor: Arc<dyn MailStoreDoctor + Send + Sync>,
    pub task_store_doctor: Arc<dyn TaskStoreDoctor + Send + Sync>,
    pub roster_store_doctor: Arc<dyn RosterStoreDoctor + Send + Sync>,
    pub config_doctor: Arc<dyn ConfigDoctor + Send + Sync>,
    pub remote_replay_store: Arc<dyn RemoteReplayStore + Send + Sync>,
}

impl fmt::Debug for RuntimeBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeBundle")
            .field("mail_store", &"dyn MailStore")
            .field("task_store", &"dyn TaskStore")
            .field("roster_store", &"dyn RosterStore")
            .field("mail_store_doctor", &"dyn MailStoreDoctor")
            .field("task_store_doctor", &"dyn TaskStoreDoctor")
            .field("roster_store_doctor", &"dyn RosterStoreDoctor")
            .field("config_doctor", &"dyn ConfigDoctor")
            .field("remote_replay_store", &"dyn RemoteReplayStore")
            .finish()
    }
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
