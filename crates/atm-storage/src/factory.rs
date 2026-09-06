use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::{
    AsyncMailboxReader, AsyncMessageSearchStore, AsyncMessageStore, AtmError,
    DiagnosticTimelineStore, GraftReceiverEndpointStore, MessageSearchStore, MessageStore,
    NudgeTemplateOverrideStore, PeerConfigStore, PendingNudgeStore, RosterRuntimeMirror,
    RosterStore, TemplateCatalogStore,
};

/// Effective capacity settings for one reader lane, selected by the backend
/// when it constructs the live pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveReaderLane {
    pub pool_size: usize,
    pub queue_depth: usize,
}

/// Shared effective reader-pool settings, including compatibility lane views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveReaderPool {
    pub pool_size: usize,
    pub queue_depth: usize,
    pub tool_class_max_in_flight: usize,
    pub mailbox: EffectiveReaderLane,
    pub search: EffectiveReaderLane,
}

/// Backend-neutral effective capacity settings for the benchmarked reader
/// lanes. This is runtime data, not a benchmark-side default.
pub type EffectiveReaderLanes = EffectiveReaderPool;

/// A roster [`RosterStore`] handle proven, *by type*, to be the
/// write-through RAM-mirroring seam -- paired with the
/// [`RosterRuntimeMirror`] read handle over the same backing state.
///
/// # Why this is a newtype and not two plain fields
///
/// [`StorageHandleParts`] used to take `roster_store` and
/// `roster_runtime_mirror` as two independent `Arc<dyn ...>` fields. Nothing
/// stopped a future edit from passing a backend's *raw durable* roster store
/// (e.g. `backend.roster_store()`) as `roster_store` while pairing it with an
/// unrelated mirror: every roster read would then silently go back to hitting
/// the database, defeating the RAM-roster design ruling, and no boundary lint
/// would notice because the bypass is intra-crate.
///
/// This type closes that vector at the type level. Its only constructor,
/// [`WriteThroughRosterStore::from_write_through_view`], requires a *single*
/// value that implements **both** [`RosterStore`] and [`RosterRuntimeMirror`]
/// -- i.e. a store that serves its own reads from its own RAM mirror. A
/// durable-backed roster store implements only [`RosterStore`], and an
/// `Arc<dyn RosterStore + Send + Sync>` cannot coerce into the bound at all,
/// so the bypass is a compile error rather than a silent regression.
#[derive(Clone)]
pub struct WriteThroughRosterStore {
    store: Arc<dyn RosterStore + Send + Sync>,
    mirror: Arc<dyn RosterRuntimeMirror + Send + Sync>,
}

impl fmt::Debug for WriteThroughRosterStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteThroughRosterStore").finish()
    }
}

impl WriteThroughRosterStore {
    /// The sole constructor: adopts one write-through view as both the
    /// durable-write [`RosterStore`] handle and the [`RosterRuntimeMirror`]
    /// read handle.
    ///
    /// The `V: RosterStore + RosterRuntimeMirror` bound is the enforcement
    /// mechanism -- a raw durable roster store cannot satisfy it.
    pub fn from_write_through_view<V>(view: Arc<V>) -> Self
    where
        V: RosterStore + RosterRuntimeMirror + 'static,
    {
        Self {
            store: Arc::clone(&view) as Arc<dyn RosterStore + Send + Sync>,
            mirror: view as Arc<dyn RosterRuntimeMirror + Send + Sync>,
        }
    }

    /// The write-through durable-write handle.
    pub fn store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        Arc::clone(&self.store)
    }

    /// The RAM mirror read handle paired with [`Self::store`].
    pub fn mirror(&self) -> Arc<dyn RosterRuntimeMirror + Send + Sync> {
        Arc::clone(&self.mirror)
    }
}

/// Backend-neutral handles returned by the selected durable storage backend.
#[derive(Clone)]
pub struct StorageHandles {
    message_store: Arc<dyn MessageStore + Send + Sync>,
    async_message_store: Arc<dyn AsyncMessageStore + Send + Sync>,
    async_mailbox_reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
    roster_store: Arc<dyn RosterStore + Send + Sync>,
    /// Runtime-owned, write-through RAM roster mirror paired with
    /// `roster_store`. Every roster consumer reads through this handle
    /// after startup hydration; see [`RosterRuntimeMirror`].
    roster_runtime_mirror: Arc<dyn RosterRuntimeMirror + Send + Sync>,
    nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    pending_nudge_store: Arc<dyn PendingNudgeStore + Send + Sync>,
    graft_receiver_endpoint_store: Arc<dyn GraftReceiverEndpointStore + Send + Sync>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    template_catalog_store: Arc<dyn TemplateCatalogStore + Send + Sync>,
    message_search_store: Arc<dyn MessageSearchStore + Send + Sync>,
    async_message_search_store: Arc<dyn AsyncMessageSearchStore + Send + Sync>,
    diagnostic_timeline: Arc<dyn DiagnosticTimelineStore + Send + Sync>,
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
    /// The write-through roster seam: the durable-write [`RosterStore`]
    /// handle and its paired RAM [`RosterRuntimeMirror`], carried as one
    /// value that only [`WriteThroughRosterStore::from_write_through_view`]
    /// can construct. Handing a raw durable roster store here does not
    /// compile.
    pub roster: WriteThroughRosterStore,
    pub nudge_template_override_store: Arc<dyn NudgeTemplateOverrideStore + Send + Sync>,
    pub pending_nudge_store: Arc<dyn PendingNudgeStore + Send + Sync>,
    pub graft_receiver_endpoint_store: Arc<dyn GraftReceiverEndpointStore + Send + Sync>,
    pub peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    pub template_catalog_store: Arc<dyn TemplateCatalogStore + Send + Sync>,
    pub message_search_store: Arc<dyn MessageSearchStore + Send + Sync>,
    pub async_message_search_store: Arc<dyn AsyncMessageSearchStore + Send + Sync>,
    pub diagnostic_timeline: Arc<dyn DiagnosticTimelineStore + Send + Sync>,
    pub effective_reader_lanes: Option<EffectiveReaderLanes>,
}

impl fmt::Debug for StorageHandles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageHandles")
            .field("message_store", &"dyn MessageStore")
            .field("async_message_store", &"dyn AsyncMessageStore")
            .field("roster_store", &"dyn RosterStore")
            .field("roster_runtime_mirror", &"dyn RosterRuntimeMirror")
            .field(
                "nudge_template_override_store",
                &"dyn NudgeTemplateOverrideStore",
            )
            .field("pending_nudge_store", &"dyn PendingNudgeStore")
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
            roster_store: parts.roster.store(),
            roster_runtime_mirror: parts.roster.mirror(),
            nudge_template_override_store: parts.nudge_template_override_store,
            pending_nudge_store: parts.pending_nudge_store,
            graft_receiver_endpoint_store: parts.graft_receiver_endpoint_store,
            peer_config_store: parts.peer_config_store,
            template_catalog_store: parts.template_catalog_store,
            message_search_store: parts.message_search_store,
            async_message_search_store: parts.async_message_search_store,
            diagnostic_timeline: parts.diagnostic_timeline,
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

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        Arc::clone(&self.roster_store)
    }

    /// Returns the write-through RAM roster mirror paired with
    /// [`Self::roster_store`]. Every roster consumer reads through this
    /// handle after startup hydration.
    pub fn roster_runtime_mirror(&self) -> Arc<dyn RosterRuntimeMirror + Send + Sync> {
        Arc::clone(&self.roster_runtime_mirror)
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

    /// Returns the bounded, read-only retained diagnostic timeline capability.
    pub fn diagnostic_timeline(&self) -> Arc<dyn DiagnosticTimelineStore + Send + Sync> {
        Arc::clone(&self.diagnostic_timeline)
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
