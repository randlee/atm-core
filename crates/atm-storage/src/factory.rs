use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::{AtmError, MessageStore, NudgeTemplateOverrideStore, RosterStore};

/// Backend-neutral handles returned by the selected durable storage backend.
#[derive(Clone)]
pub struct StorageHandles {
    message_store: Arc<dyn MessageStore + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
}

impl fmt::Debug for StorageHandles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageHandles")
            .field("message_store", &"dyn MessageStore")
            .field("roster_store", &"dyn RosterStore")
            .field(
                "nudge_template_override_store",
                &"dyn NudgeTemplateOverrideStore",
            )
            .finish()
    }
}

impl StorageHandles {
    pub fn new(
        message_store: Arc<dyn MessageStore + Send + Sync>,
        roster_store: Arc<dyn RosterStore + Send + Sync>,
        nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    ) -> Self {
        Self {
            message_store,
            roster_store,
            nudge_template_override_store,
        }
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore + Send + Sync> {
        Arc::clone(&self.message_store)
    }

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        Arc::clone(&self.roster_store)
    }

    pub fn nudge_template_override_store(
        &self,
    ) -> Arc<dyn NudgeTemplateOverrideStore + Send + Sync> {
        Arc::clone(&self.nudge_template_override_store)
    }
}

/// Opens the durable storage backend selected by an executable composition root.
pub trait StorageFactory: Send + Sync {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError>;
}
