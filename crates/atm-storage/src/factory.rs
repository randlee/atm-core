use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::{
    AsyncMessageSearchStore, AsyncMessageStore, AtmError, MessageSearchStore, MessageStore,
    NudgeTemplateOverrideStore, PeerConfigStore, RosterStore, TemplateCatalogStore,
};

/// Backend-neutral handles returned by the selected durable storage backend.
#[derive(Clone)]
pub struct StorageHandles {
    message_store: Arc<dyn MessageStore + Send + Sync>,
    async_message_store: Arc<dyn AsyncMessageStore + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    template_catalog_store: Arc<dyn TemplateCatalogStore + Send + Sync>,
    message_search_store: Arc<dyn MessageSearchStore + Send + Sync>,
    async_message_search_store: Arc<dyn AsyncMessageSearchStore + Send + Sync>,
}

/// Typed assembly input for [`StorageHandles`].
///
/// New optional storage capabilities belong in this named composition record,
/// avoiding a positional constructor that silently changes meaning when a
/// backend adds another handle.
#[derive(Clone)]
pub struct StorageHandleParts {
    pub message_store: Arc<dyn MessageStore + Send + Sync>,
    pub async_message_store: Arc<dyn AsyncMessageStore + Send + Sync>,
    pub roster_store: Arc<dyn RosterStore + Send + Sync>,
    pub nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    pub peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    pub template_catalog_store: Arc<dyn TemplateCatalogStore + Send + Sync>,
    pub message_search_store: Arc<dyn MessageSearchStore + Send + Sync>,
    pub async_message_search_store: Arc<dyn AsyncMessageSearchStore + Send + Sync>,
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
            .field("template_catalog_store", &"dyn TemplateCatalogStore")
            .field("message_search_store", &"dyn MessageSearchStore")
            .field("async_message_search_store", &"dyn AsyncMessageSearchStore")
            .finish()
    }
}

impl StorageHandles {
    pub fn from_parts(parts: StorageHandleParts) -> Self {
        Self {
            message_store: parts.message_store,
            async_message_store: parts.async_message_store,
            roster_store: parts.roster_store,
            nudge_template_override_store: parts.nudge_template_override_store,
            peer_config_store: parts.peer_config_store,
            template_catalog_store: parts.template_catalog_store,
            message_search_store: parts.message_search_store,
            async_message_search_store: parts.async_message_search_store,
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

    /// Returns the sealed immutable-template catalog capability.
    pub fn template_catalog_store(&self) -> Arc<dyn TemplateCatalogStore + Send + Sync> {
        Arc::clone(&self.template_catalog_store)
    }

    /// Returns the sealed typed search capability. AN.6 maps CLI/HTTP input
    /// into this leaf contract; neither surface owns SQL or FTS syntax.
    pub fn message_search_store(&self) -> Arc<dyn MessageSearchStore + Send + Sync> {
        Arc::clone(&self.message_search_store)
    }

    /// Returns the Tokio-safe companion for the same search semantics.
    pub fn async_message_search_store(&self) -> Arc<dyn AsyncMessageSearchStore + Send + Sync> {
        Arc::clone(&self.async_message_search_store)
    }
}

/// Opens the durable storage backend selected by an executable composition root.
pub trait StorageFactory: Send + Sync {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError>;
}
