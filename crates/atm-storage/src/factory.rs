use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::{
    AsyncMailboxReader, AsyncMessageSearchStore, AsyncMessageStore, AsyncTaskLedgerReader,
    AtmError, GraftReceiverEndpointStore, MessageSearchStore, MessageStore,
    NudgeTemplateOverrideStore, PeerConfigStore, PendingNudgeStore, RosterStore, TaskStore,
    TemplateCatalogStore,
};

/// Effective capacity settings for one reader lane, selected by the backend
/// when it constructs the live pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveReaderLane {
    pub pool_size: usize,
    pub queue_depth: usize,
}

/// Backend-neutral effective capacity settings for the benchmarked reader
/// lanes. This is runtime data, not a benchmark-side default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveReaderLanes {
    pub mailbox: EffectiveReaderLane,
    pub search: EffectiveReaderLane,
}

/// Backend-neutral handles returned by the selected durable storage backend.
#[derive(Clone)]
pub struct StorageHandles {
    message_store: Arc<dyn MessageStore + Send + Sync>,
    async_message_store: Arc<dyn AsyncMessageStore + Send + Sync>,
    async_mailbox_reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
    async_task_ledger_reader: Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    pending_nudge_store: Arc<dyn PendingNudgeStore + Send + Sync>,
    task_store: Arc<dyn TaskStore + Send + Sync>,
    graft_receiver_endpoint_store: Arc<dyn GraftReceiverEndpointStore + Send + Sync>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    template_catalog_store: Arc<dyn TemplateCatalogStore + Send + Sync>,
    message_search_store: Arc<dyn MessageSearchStore + Send + Sync>,
    async_message_search_store: Arc<dyn AsyncMessageSearchStore + Send + Sync>,
    effective_reader_lanes: Option<EffectiveReaderLanes>,
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
    pub async_mailbox_reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
    pub async_task_ledger_reader: Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    pub roster_store: Arc<dyn RosterStore + Send + Sync>,
    pub nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    pub pending_nudge_store: Arc<dyn PendingNudgeStore + Send + Sync>,
    pub task_store: Arc<dyn TaskStore + Send + Sync>,
    pub graft_receiver_endpoint_store: Arc<dyn GraftReceiverEndpointStore + Send + Sync>,
    pub peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    pub template_catalog_store: Arc<dyn TemplateCatalogStore + Send + Sync>,
    pub message_search_store: Arc<dyn MessageSearchStore + Send + Sync>,
    pub async_message_search_store: Arc<dyn AsyncMessageSearchStore + Send + Sync>,
    pub effective_reader_lanes: Option<EffectiveReaderLanes>,
}

impl fmt::Debug for StorageHandles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageHandles")
            .field("message_store", &"dyn MessageStore")
            .field("async_message_store", &"dyn AsyncMessageStore")
            .field("async_task_ledger_reader", &"dyn AsyncTaskLedgerReader")
            .field("roster_store", &"dyn RosterStore")
            .field(
                "nudge_template_override_store",
                &"dyn NudgeTemplateOverrideStore",
            )
            .field("pending_nudge_store", &"dyn PendingNudgeStore")
            .field("task_store", &"dyn TaskStore")
            .field(
                "graft_receiver_endpoint_store",
                &"dyn GraftReceiverEndpointStore",
            )
            .field("peer_config_store", &"dyn PeerConfigStore")
            .field("template_catalog_store", &"dyn TemplateCatalogStore")
            .field("message_search_store", &"dyn MessageSearchStore")
            .field("async_message_search_store", &"dyn AsyncMessageSearchStore")
            .field("effective_reader_lanes", &self.effective_reader_lanes)
            .finish()
    }
}

impl StorageHandles {
    pub fn from_parts(parts: StorageHandleParts) -> Self {
        Self {
            message_store: parts.message_store,
            async_message_store: parts.async_message_store,
            async_mailbox_reader: parts.async_mailbox_reader,
            async_task_ledger_reader: parts.async_task_ledger_reader,
            roster_store: parts.roster_store,
            nudge_template_override_store: parts.nudge_template_override_store,
            pending_nudge_store: parts.pending_nudge_store,
            task_store: parts.task_store,
            graft_receiver_endpoint_store: parts.graft_receiver_endpoint_store,
            peer_config_store: parts.peer_config_store,
            template_catalog_store: parts.template_catalog_store,
            message_search_store: parts.message_search_store,
            async_message_search_store: parts.async_message_search_store,
            effective_reader_lanes: parts.effective_reader_lanes,
        }
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore + Send + Sync> {
        Arc::clone(&self.message_store)
    }

    /// Returns the Tokio-safe durable message-admission boundary.
    pub fn async_message_store(&self) -> Arc<dyn AsyncMessageStore + Send + Sync> {
        Arc::clone(&self.async_message_store)
    }

    /// Returns the bounded read-only mailbox lane selected by composition.
    pub fn async_mailbox_reader(&self) -> Arc<dyn AsyncMailboxReader + Send + Sync> {
        Arc::clone(&self.async_mailbox_reader)
    }

    /// Returns the bounded read-only task-ledger lane selected by composition.
    pub fn async_task_ledger_reader(&self) -> Arc<dyn AsyncTaskLedgerReader + Send + Sync> {
        Arc::clone(&self.async_task_ledger_reader)
    }

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        Arc::clone(&self.roster_store)
    }

    pub fn nudge_template_override_store(
        &self,
    ) -> Arc<dyn NudgeTemplateOverrideStore + Send + Sync> {
        Arc::clone(&self.nudge_template_override_store)
    }

    /// Returns the durable at-most-once delivery capability for deferred
    /// (`atm queue`) nudges.
    pub fn pending_nudge_store(&self) -> Arc<dyn PendingNudgeStore + Send + Sync> {
        Arc::clone(&self.pending_nudge_store)
    }

    /// Returns the durable task-ledger capability selected by composition.
    pub fn task_store(&self) -> Arc<dyn TaskStore + Send + Sync> {
        Arc::clone(&self.task_store)
    }

    pub fn graft_receiver_endpoint_store(
        &self,
    ) -> Arc<dyn GraftReceiverEndpointStore + Send + Sync> {
        Arc::clone(&self.graft_receiver_endpoint_store)
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

    /// Returns the effective reader-pool capacities selected by the backend,
    /// when that backend exposes the benchmarked mailbox and search lanes.
    #[must_use]
    pub fn effective_reader_lanes(&self) -> Option<EffectiveReaderLanes> {
        self.effective_reader_lanes
    }
}

/// Opens the durable storage backend selected by an executable composition root.
pub trait StorageFactory: Send + Sync {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError>;
}
