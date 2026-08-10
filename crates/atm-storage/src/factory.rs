use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::{
    AsyncMessageStore, AtmError, MessageStore, NudgeTemplateOverrideStore, OutboundMessageQuery,
    PeerConfigStore, RosterStore,
};

/// Backend-neutral handles returned by the selected durable storage backend.
#[derive(Clone)]
pub struct StorageHandles {
    message_store: Arc<dyn MessageStore + Send + Sync>,
    async_message_store: Arc<dyn AsyncMessageStore + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    outbound_message_query: Arc<dyn OutboundMessageQuery + Send + Sync>,
}

impl fmt::Debug for StorageHandles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageHandles")
            .field("message_store", &"dyn MessageStore")
            .field("async_message_store", &"dyn AsyncMessageStore")
            .field("roster_store", &"dyn RosterStore")
            .field(
                "nudge_template_override_store",
                &"dyn NudgeTemplateOverrideStore",
            )
            .field("peer_config_store", &"dyn PeerConfigStore")
            .field("outbound_message_query", &"dyn OutboundMessageQuery")
            .finish()
    }
}

impl StorageHandles {
    pub fn new(
        message_store: Arc<dyn MessageStore + Send + Sync>,
        async_message_store: Arc<dyn AsyncMessageStore + Send + Sync>,
        roster_store: Arc<dyn RosterStore + Send + Sync>,
        nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
        peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
        outbound_message_query: Arc<dyn OutboundMessageQuery + Send + Sync>,
    ) -> Self {
        Self {
            message_store,
            async_message_store,
            roster_store,
            nudge_template_override_store,
            peer_config_store,
            outbound_message_query,
        }
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore + Send + Sync> {
        Arc::clone(&self.message_store)
    }

    /// Returns the Tokio-safe durable message-admission boundary.
    pub fn async_message_store(&self) -> Arc<dyn AsyncMessageStore + Send + Sync> {
        Arc::clone(&self.async_message_store)
    }

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        Arc::clone(&self.roster_store)
    }

    pub fn nudge_template_override_store(
        &self,
    ) -> Arc<dyn NudgeTemplateOverrideStore + Send + Sync> {
        Arc::clone(&self.nudge_template_override_store)
    }

    pub fn peer_config_store(&self) -> Arc<dyn PeerConfigStore + Send + Sync> {
        Arc::clone(&self.peer_config_store)
    }

    pub fn outbound_message_query(&self) -> Arc<dyn OutboundMessageQuery + Send + Sync> {
        Arc::clone(&self.outbound_message_query)
    }
}

/// Opens the durable storage backend selected by an executable composition root.
pub trait StorageFactory: Send + Sync {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError>;
}
