#![allow(
    deprecated,
    reason = "the retained runtime bridge still consumes the transitional shared storage traits until the direct boundary fully replaces it"
)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use atm_storage::{
    AsyncMessageSearchStore, AsyncMessageStore as SharedAsyncMessageStore, GraftEndpointStoreError,
    GraftReceiverEndpointStore, GraftReceiverLease, MessageStore as SharedMessageStore,
    OwnerGeneration, PendingNudgeStore, RosterStore as SharedRosterStore, TemplateCatalogStore,
};

use crate::boundary::TemplateComposer;
use crate::config::{self, AtmConfig};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
#[cfg(test)]
use crate::protocol::NotificationEvent;
use crate::read::seen_state;
use crate::schema::InboxMessage;
use crate::types::{AgentName, IsoTimestamp, TeamName};
const MAX_NON_CLAUDE_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceConfigAccess {
    #[default]
    Client,
    Disabled,
}

/// Reload-scoped cache for immutable roster snapshots used during admission.
#[derive(Default)]
struct RosterSnapshotCache {
    snapshots: RwLock<BTreeMap<TeamName, Arc<[crate::boundary::RosterEntry]>>>,
}

/// Lease snapshots avoid a control-path SQLite lookup for every admitted
/// local message. A graft receiver refreshes every second, so retaining a
/// value for that same bounded interval preserves restart recovery: a missing
/// refresh expires the snapshot and the next delivery re-reads durable state.
const GRAFT_RECEIVER_LEASE_CACHE_TTL: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct CachedGraftReceiverLease {
    lease: Option<GraftReceiverLease>,
    expires_at: Instant,
}

#[derive(Default)]
struct GraftReceiverLeaseEntry {
    state: std::sync::Mutex<GraftReceiverLeaseState>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
enum GraftReceiverLeaseState {
    #[default]
    Empty,
    Loading,
    Cached(CachedGraftReceiverLease),
}

struct GraftReceiverLeaseCache {
    entries: RwLock<BTreeMap<(TeamName, AgentName), Arc<GraftReceiverLeaseEntry>>>,
    ttl: Duration,
}

impl Default for GraftReceiverLeaseCache {
    fn default() -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
            ttl: GRAFT_RECEIVER_LEASE_CACHE_TTL,
        }
    }
}

impl GraftReceiverLeaseCache {
    fn entry(&self, key: &(TeamName, AgentName)) -> Arc<GraftReceiverLeaseEntry> {
        {
            let entries = self
                .entries
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(entry) = entries.get(key) {
                return Arc::clone(entry);
            }
        }

        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            entries
                .entry(key.clone())
                .or_insert_with(|| Arc::new(GraftReceiverLeaseEntry::default())),
        )
    }

    fn invalidate(&self, team: &TeamName, agent: &AgentName) {
        let key = (team.clone(), agent.clone());
        let Some(entry) = self
            .entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()
        else {
            return;
        };
        let mut state = entry
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Waiting for an in-flight durable lookup is deliberate: once a
        // receiver lifecycle mutation returns, it cannot be overwritten by
        // an older lookup that began before the mutation.
        while matches!(*state, GraftReceiverLeaseState::Loading) {
            state = entry
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        *state = GraftReceiverLeaseState::Empty;
    }

    fn load(
        &self,
        team: &TeamName,
        agent: &AgentName,
        load: impl FnOnce() -> Result<Option<GraftReceiverLease>, AtmError>,
    ) -> Result<Option<GraftReceiverLease>, AtmError> {
        let key = (team.clone(), agent.clone());
        let entry = self.entry(&key);
        let mut load = Some(load);

        loop {
            let mut state = entry
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &*state {
                GraftReceiverLeaseState::Cached(cached) if cached.expires_at > Instant::now() => {
                    return Ok(cached.lease.clone());
                }
                GraftReceiverLeaseState::Loading => {
                    while matches!(*state, GraftReceiverLeaseState::Loading) {
                        state = entry
                            .changed
                            .wait(state)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                }
                GraftReceiverLeaseState::Cached(_) | GraftReceiverLeaseState::Empty => {
                    // Only this key's entry lock is held while loading. Other
                    // receivers remain free to perform their own durable
                    // lookup instead of being serialized behind a process-wide
                    // cache-map write guard.
                    *state = GraftReceiverLeaseState::Loading;
                    drop(state);
                    let result = load
                        .take()
                        .expect("each cache caller owns one durable lookup")(
                    );
                    let mut state = entry
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    match &result {
                        Ok(lease) => {
                            *state = GraftReceiverLeaseState::Cached(CachedGraftReceiverLease {
                                lease: lease.clone(),
                                expires_at: Instant::now() + self.ttl,
                            });
                        }
                        Err(_) => *state = GraftReceiverLeaseState::Empty,
                    }
                    entry.changed.notify_all();
                    return result;
                }
            }
        }
    }

    #[cfg(test)]
    fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
            ttl,
        }
    }
}

impl RosterSnapshotCache {
    fn clear(&self) {
        let mut snapshots = match self.snapshots.write() {
            Ok(snapshots) => snapshots,
            Err(poisoned) => poisoned.into_inner(),
        };
        snapshots.clear();
    }

    fn load(
        &self,
        team: &TeamName,
        load: impl FnOnce() -> Result<Arc<[crate::boundary::RosterEntry]>, AtmError>,
    ) -> Result<Arc<[crate::boundary::RosterEntry]>, AtmError> {
        {
            let snapshots = match self.snapshots.read() {
                Ok(snapshots) => snapshots,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(snapshot) = snapshots.get(team) {
                return Ok(Arc::clone(snapshot));
            }
        }

        // Hold the write lock across the one backing-store read. This makes a
        // cold admission burst load a team once instead of creating one SQLite
        // reader per concurrent request.
        let mut snapshots = match self.snapshots.write() {
            Ok(snapshots) => snapshots,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(snapshot) = snapshots.get(team) {
            return Ok(Arc::clone(snapshot));
        }
        let snapshot = load()?;
        snapshots.insert(team.clone(), Arc::clone(&snapshot));
        Ok(snapshot)
    }
}

/// Invoke a closure with the installed retained local runtime.
#[doc(hidden)]
pub fn with_default_local_service_runtime<T>(
    f: impl FnOnce(&LocalServiceRuntime) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let runtime = crate::service_runtime_store::default_runtime()?;
    f(&runtime)
}

pub(crate) trait RetainedServiceRuntime: crate::boundary::sealed::Sealed {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError>;
    fn load_nudge_template_override(
        &self,
        _team: &TeamName,
        _kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        Ok(None)
    }
    #[allow(dead_code)]
    fn graft_receiver_lease(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
    ) -> Result<Option<GraftReceiverLease>, AtmError> {
        Ok(None)
    }
    #[allow(dead_code)]
    fn mark_graft_receiver_unreachable(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
        _owner_generation: &OwnerGeneration,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AtmError> {
        Ok(())
    }
    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError>;
    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<IsoTimestamp>, AtmError>;
    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError>;
    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[InboxMessage],
    ) -> Result<(), AtmError>;
    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError>;
    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError>;
}

#[derive(Clone)]
pub struct LocalServiceRuntime {
    pub(crate) message_store: std::sync::Arc<dyn SharedMessageStore + Send + Sync>,
    async_message_store: Option<std::sync::Arc<dyn SharedAsyncMessageStore + Send + Sync>>,
    async_mailbox_reader: Option<std::sync::Arc<dyn atm_storage::AsyncMailboxReader + Send + Sync>>,
    async_message_search_store: Option<std::sync::Arc<dyn AsyncMessageSearchStore + Send + Sync>>,
    pub(crate) roster_store: std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
    pub(crate) nudge_template_override_store:
        std::sync::Arc<dyn crate::boundary::NudgeTemplateOverrideStore + Send + Sync>,
    pub(crate) non_claude_outbound:
        std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
    /// Optional durable at-most-once delivery capability for deferred
    /// (`atm queue`) nudges. Unset in runtimes that never enqueue a deferred
    /// nudge, e.g. plain-text mailbox tests.
    pending_nudge_store: Option<std::sync::Arc<dyn PendingNudgeStore + Send + Sync>>,
    graft_receiver_endpoint_store:
        Option<std::sync::Arc<dyn GraftReceiverEndpointStore + Send + Sync>>,
    /// Optional renderer selected by the bootstrap composition root. Core send
    /// policy sees only this port; it never depends on `sc-composer` itself.
    pub(crate) template_composer: Option<std::sync::Arc<dyn TemplateComposer>>,
    /// The sealed storage capability for the one atomic
    /// template-registration-plus-decomposed-message operation.
    pub(crate) template_catalog_store:
        Option<std::sync::Arc<dyn TemplateCatalogStore + Send + Sync>>,
    /// Immutable roster snapshots used by daemon-owned admission.
    ///
    /// A daemon reload clears this cache before publishing its replacement
    /// admission view. Keeping the roster snapshot here prevents every local
    /// message admission from opening another SQLite reader connection merely
    /// to rediscover an unchanged recipient.
    roster_cache: Arc<RosterSnapshotCache>,
    /// Short-lived receiver endpoint snapshots keep the graft registry's
    /// control-path SQLite lookup out of the local write hot path.
    graft_receiver_lease_cache: Arc<GraftReceiverLeaseCache>,
    workspace_config_access: WorkspaceConfigAccess,
}

impl LocalServiceRuntime {
    pub fn new_with_delivery_boundaries(
        message_store: std::sync::Arc<dyn SharedMessageStore + Send + Sync>,
        roster_store: std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
        nudge_template_override_store: std::sync::Arc<
            dyn crate::boundary::NudgeTemplateOverrideStore + Send + Sync,
        >,
        non_claude_outbound: std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
    ) -> Self {
        Self {
            message_store,
            async_message_store: None,
            async_mailbox_reader: None,
            async_message_search_store: None,
            roster_store,
            nudge_template_override_store,
            non_claude_outbound,
            pending_nudge_store: None,
            graft_receiver_endpoint_store: None,
            template_composer: None,
            template_catalog_store: None,
            roster_cache: Arc::new(RosterSnapshotCache::default()),
            graft_receiver_lease_cache: Arc::new(GraftReceiverLeaseCache::default()),
            workspace_config_access: WorkspaceConfigAccess::Client,
        }
    }

    /// Installs the storage catalog and the composition-root renderer port
    /// used by render-on-read. The core remains independent of the concrete
    /// adapter; tests may leave this seam unset for plain-text mailboxes.
    #[must_use]
    pub fn with_template_rendering(
        mut self,
        template_catalog_store: std::sync::Arc<dyn TemplateCatalogStore + Send + Sync>,
        template_composer: Option<std::sync::Arc<dyn crate::boundary::TemplateComposer>>,
    ) -> Self {
        self.template_catalog_store = Some(template_catalog_store);
        self.template_composer = template_composer;
        self
    }

    /// Attaches the Tokio-safe durable-admission boundary selected by the
    /// composition root. The legacy synchronous store remains available only
    /// to transitional non-Tokio callers.
    #[must_use]
    pub fn with_async_message_store(
        mut self,
        async_message_store: std::sync::Arc<dyn SharedAsyncMessageStore + Send + Sync>,
    ) -> Self {
        self.async_message_store = Some(async_message_store);
        self
    }

    /// Attaches the bounded read-only mailbox capability selected by the
    /// storage composition root.  It is deliberately separate from the
    /// ordered durable writer lane.
    #[must_use]
    pub fn with_async_mailbox_reader(
        mut self,
        async_mailbox_reader: std::sync::Arc<dyn atm_storage::AsyncMailboxReader + Send + Sync>,
    ) -> Self {
        self.async_mailbox_reader = Some(async_mailbox_reader);
        self
    }

    /// Returns the Tokio-safe bounded mailbox reader selected by composition.
    pub fn async_mailbox_reader(
        &self,
    ) -> Result<std::sync::Arc<dyn atm_storage::AsyncMailboxReader + Send + Sync>, AtmError> {
        self.async_mailbox_reader.clone().ok_or_else(|| {
            AtmError::daemon_unavailable("Tokio mailbox reader was not installed in this runtime")
        })
    }

    /// Attaches the Tokio-safe typed search capability selected by the one
    /// storage composition root.  HTTP awaits this port directly; it never
    /// opens a synchronous SQLite reader on a request worker.
    #[must_use]
    pub fn with_async_message_search_store(
        mut self,
        async_message_search_store: std::sync::Arc<dyn AsyncMessageSearchStore + Send + Sync>,
    ) -> Self {
        self.async_message_search_store = Some(async_message_search_store);
        self
    }

    /// Returns the runtime-selected typed asynchronous search capability.
    pub fn async_message_search_store(
        &self,
    ) -> Result<std::sync::Arc<dyn AsyncMessageSearchStore + Send + Sync>, AtmError> {
        self.async_message_search_store.clone().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "Tokio typed message search was not installed in this runtime",
            )
        })
    }

    /// Attaches the durable at-most-once delivery capability for deferred
    /// (`atm queue`) nudges selected by the composition root.
    #[must_use]
    pub fn with_pending_nudge_store(
        mut self,
        pending_nudge_store: std::sync::Arc<dyn PendingNudgeStore + Send + Sync>,
    ) -> Self {
        self.pending_nudge_store = Some(pending_nudge_store);
        self
    }

    /// Returns the runtime-selected deferred-nudge delivery capability.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if no `PendingNudgeStore` was installed in this
    /// runtime.
    pub fn pending_nudge_store(
        &self,
    ) -> Result<std::sync::Arc<dyn PendingNudgeStore + Send + Sync>, AtmError> {
        self.pending_nudge_store.clone().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "the deferred-nudge pending store was not installed in this runtime",
            )
        })
    }

    /// Attaches the durable graft receiver endpoint registry for local-only
    /// registration, refresh, unregistration, and lookup requests.
    #[must_use]
    pub fn with_graft_receiver_endpoint_store(
        mut self,
        store: std::sync::Arc<dyn GraftReceiverEndpointStore + Send + Sync>,
    ) -> Self {
        self.graft_receiver_endpoint_store = Some(store);
        self
    }

    pub fn graft_receiver_endpoint_store(
        &self,
    ) -> Result<std::sync::Arc<dyn GraftReceiverEndpointStore + Send + Sync>, AtmError> {
        self.graft_receiver_endpoint_store.clone().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "the graft receiver endpoint store was not installed in this runtime",
            )
        })
    }

    /// Invalidates a receiver's admission snapshot after a durable lease
    /// mutation. Registration, refresh, unregistration, and delivery failure
    /// all call this so a stale endpoint cannot outlive a receiver restart.
    pub fn invalidate_graft_receiver_lease(&self, team: &TeamName, agent: &AgentName) {
        self.graft_receiver_lease_cache.invalidate(team, agent);
    }

    /// Attaches the approved template renderer port at the composition root.
    #[must_use]
    pub fn with_template_composer(
        mut self,
        template_composer: std::sync::Arc<dyn TemplateComposer>,
    ) -> Self {
        self.template_composer = Some(template_composer);
        self
    }

    /// Attaches the matching durable immutable-template capability.
    #[must_use]
    pub fn with_template_catalog_store(
        mut self,
        template_catalog_store: std::sync::Arc<dyn TemplateCatalogStore + Send + Sync>,
    ) -> Self {
        self.template_catalog_store = Some(template_catalog_store);
        self
    }

    /// Returns the bootstrap-installed renderer, if this runtime supports
    /// template-aware sends.
    #[must_use]
    pub fn template_composer(&self) -> Option<std::sync::Arc<dyn TemplateComposer>> {
        self.template_composer.clone()
    }

    /// Returns the bootstrap-installed catalog capability, if this runtime
    /// supports decomposed template admission.
    #[must_use]
    pub fn template_catalog_store(
        &self,
    ) -> Option<std::sync::Arc<dyn TemplateCatalogStore + Send + Sync>> {
        self.template_catalog_store.clone()
    }

    /// Awaits bounded admission and the durable outcome without blocking a
    /// Tokio request executor. This is the replacement daemon's write seam.
    pub async fn save_message_if_absent_async(
        &self,
        message: crate::boundary::Message,
    ) -> Result<Option<crate::boundary::Message>, AtmError> {
        let store = self.async_message_store.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "Tokio durable message admission was not installed in this runtime",
            )
        })?;
        store.save_message_if_absent_async(message).await
    }

    /// One durable Tokio admission for a decomposed template message.
    pub async fn admit_template_message_async(
        &self,
        admission: atm_storage::TemplateMessageAdmission,
    ) -> Result<Option<crate::boundary::Message>, AtmError> {
        let store = self.async_message_store.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "Tokio template message admission was not installed in this runtime",
            )
        })?;
        store.admit_template_message_async(admission).await
    }

    /// Loads a threaded-message validation projection through the bounded
    /// Tokio reader lane. It never enters the ordered writer queue.
    pub async fn list_messages_async(
        &self,
        query: atm_storage::MessageQuery,
    ) -> Result<Vec<crate::boundary::Message>, AtmError> {
        let scope = atm_storage::MailboxScope::new(query.team.clone(), query.agent.clone());
        let deadline = atm_storage::ReadDeadline::new(std::time::Duration::from_secs(5))?;
        self.async_mailbox_reader()?
            .list_messages(scope, query, deadline)
            .await
            .map_err(|error| AtmError::daemon_unavailable(error.to_string()))
    }

    /// Performs the acknowledgement source transition and reply insertion on
    /// the same async durable-admission lane as ordinary writes.
    pub async fn acknowledge_message_atomically_async(
        &self,
        source: atm_storage::AcknowledgementSource,
        builder: std::sync::Arc<dyn atm_storage::AcknowledgementReplyBuilder>,
    ) -> Result<atm_storage::AcknowledgementCommit, AtmError> {
        let store = self.async_message_store.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "Tokio acknowledgement admission was not installed in this runtime",
            )
        })?;
        store
            .acknowledge_message_atomically_async(source, builder)
            .await
    }

    /// Returns the daemon-owned runtime view. A system daemon must not read a
    /// caller-supplied workspace path while handling an IPC or peer request.
    pub fn without_workspace_config(mut self) -> Self {
        self.workspace_config_access = WorkspaceConfigAccess::Disabled;
        self
    }

    pub fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError> {
        Ok(self
            .load_cached_roster(team)?
            .iter()
            .find(|member| &member.agent_name == agent)
            .cloned())
    }

    pub fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError> {
        Ok(self.load_cached_roster(team)?.as_ref().to_vec())
    }

    /// Drops roster data held by this runtime before a control-plane reload.
    ///
    /// Cache invalidation is deliberately explicit: mutable roster state is
    /// observed only at the daemon's existing reload boundary, never through
    /// a reader connection on the synchronous message-admission path.
    pub fn clear_roster_cache(&self) {
        self.roster_cache.clear();
    }

    fn load_cached_roster(
        &self,
        team: &TeamName,
    ) -> Result<Arc<[crate::boundary::RosterEntry]>, AtmError> {
        self.roster_cache.load(team, || {
            Ok(self.roster_store.load_roster(team)?.members.into())
        })
    }

    #[doc(hidden)]
    pub fn shared_roster_store_arc(&self) -> std::sync::Arc<dyn SharedRosterStore + Send + Sync> {
        self.roster_store.clone()
    }
}

impl fmt::Debug for LocalServiceRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalServiceRuntime")
            .field(
                "message_store",
                &std::sync::Arc::as_ptr(&self.message_store),
            )
            .field("async_message_store", &self.async_message_store.is_some())
            .field("roster_store", &std::sync::Arc::as_ptr(&self.roster_store))
            .field(
                "nudge_template_override_store",
                &std::sync::Arc::as_ptr(&self.nudge_template_override_store),
            )
            .field(
                "non_claude_outbound",
                &std::sync::Arc::as_ptr(&self.non_claude_outbound),
            )
            .field("pending_nudge_store", &self.pending_nudge_store.is_some())
            .field(
                "graft_receiver_endpoint_store",
                &self.graft_receiver_endpoint_store.is_some(),
            )
            .finish()
    }
}

impl crate::boundary::sealed::Sealed for LocalServiceRuntime {}

type OutputPathFactory = std::sync::Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;

#[derive(Clone)]
/// Production fallback boundary used when the daemon runtime is not composing
/// a dedicated non-Claude outbound adapter. This is not a test double.
pub struct LocalFileNonClaudeOutbound {
    path_factory: OutputPathFactory,
}

impl LocalFileNonClaudeOutbound {
    pub fn new() -> Self {
        Self::new_with_path_factory(std::sync::Arc::new(|| {
            Ok(crate::home::host_runtime_dir()?.join("non_claude_outbound.jsonl"))
        }))
    }

    fn new_with_path_factory(path_factory: OutputPathFactory) -> Self {
        Self { path_factory }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_path(path: PathBuf) -> Self {
        Self::new_with_path_factory(std::sync::Arc::new(move || Ok(path.clone())))
    }
}

impl Default for LocalFileNonClaudeOutbound {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LocalFileNonClaudeOutbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalFileNonClaudeOutbound(..)")
    }
}

impl crate::boundary::sealed::Sealed for LocalFileNonClaudeOutbound {}

impl crate::boundary::NonClaudeOutbound for LocalFileNonClaudeOutbound {
    fn deliver_payloads(
        &self,
        request: crate::boundary::NonClaudeOutboundDeliveryRequest,
    ) -> Result<crate::boundary::NonClaudeOutboundDeliveryResponse, AtmError> {
        let output_path = (self.path_factory)()?;
        let bytes = serde_json::to_vec(&request)?;
        if bytes.len() > MAX_NON_CLAUDE_PAYLOAD_BYTES {
            return Err(AtmError::mailbox_write(format!(
                "non-Claude outbound payload for {} exceeded {MAX_NON_CLAUDE_PAYLOAD_BYTES} bytes",
                output_path.display()
            )));
        }
        let parent = output_path.parent().ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "non-Claude outbound path {} has no parent directory",
                output_path.display()
            ))
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create non-Claude outbound directory {}: {error}",
                parent.display()
            ))
        })?;
        crate::mailbox::atomic::append_jsonl_record(&output_path, &request)?;
        Ok(crate::boundary::NonClaudeOutboundDeliveryResponse {
            delivered_messages: request.messages.len(),
        })
    }
}

#[cfg(test)]
pub(crate) fn append_notification_log_at_path(
    path: &Path,
    event: &NotificationEvent,
) -> Result<(), AtmError> {
    let parent = path.parent().ok_or_else(|| {
        AtmError::mailbox_write(format!(
            "notification log path {} has no parent directory",
            path.display()
        ))
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create notification log directory {}: {error}",
            parent.display()
        ))
    })?;
    crate::mailbox::atomic::append_jsonl_record(path, event)
}

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        load_workspace_config(self.workspace_config_access, current_dir)
    }

    fn load_nudge_template_override(
        &self,
        team: &TeamName,
        kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        self.nudge_template_override_store
            .load_template_override(team, kind)
    }

    fn graft_receiver_lease(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<GraftReceiverLease>, AtmError> {
        let Some(store) = &self.graft_receiver_endpoint_store else {
            return Ok(None);
        };
        self.graft_receiver_lease_cache.load(team, agent, || {
            store.lookup(team, agent).map_err(graft_store_error)
        })
    }

    fn mark_graft_receiver_unreachable(
        &self,
        team: &TeamName,
        agent: &AgentName,
        owner_generation: &OwnerGeneration,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AtmError> {
        let Some(store) = &self.graft_receiver_endpoint_store else {
            return Ok(());
        };
        store
            .mark_unreachable(team, agent, owner_generation, now)
            .map_err(graft_store_error)?;
        self.invalidate_graft_receiver_lease(team, agent);
        Ok(())
    }

    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        crate::home::inbox_path_from_home(home_dir, team, agent)
    }

    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<IsoTimestamp>, AtmError> {
        seen_state::load_seen_watermark(home_dir, team, agent)
    }

    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError> {
        seen_state::save_seen_watermark(home_dir, team, agent, timestamp)
    }

    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError> {
        Self::load_roster_member(self, team, agent)
    }

    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError> {
        Self::load_team_roster(self, team)
    }

    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[InboxMessage],
    ) -> Result<(), AtmError> {
        self.non_claude_outbound
            .deliver_payloads(crate::boundary::NonClaudeOutboundDeliveryRequest {
                team: recipient.team.clone(),
                agent: recipient.agent.clone(),
                recipient_pane_id: recipient.recipient_pane_id.clone(),
                messages: messages.to_vec(),
            })
            .map(|_| ())
    }
}

/// Canonical `GraftEndpointStoreError` -> `AtmError` mapping.
///
/// This is the single source of truth for how graft-receiver store errors
/// surface as `AtmError`s (and therefore as HTTP statuses, via the shared
/// `error_response` translation in `atm-http-runtime`). QA-1 finding: this
/// mapping used to be duplicated here and in
/// `atm-http-runtime::storage_and_nudge_router`, with different outcomes
/// (`NotOwner` mapped to `daemon_unavailable`/503 here but to
/// `validation`/400 there) depending on which call path produced the error.
/// A generation mismatch is a caller-input problem, not a backend outage, so
/// the unified mapping treats it (and the reserved `AlreadyActive` variant)
/// as a validation error. Genuine backend I/O failures (`Storage`) preserve
/// the originating error's own code and cause (RBP-F001) instead of
/// collapsing every failure into one generic `daemon_unavailable` shape —
/// a caller-input constraint violation surfaced by the backend therefore no
/// longer looks identical to a true outage.
pub fn graft_store_error(error: GraftEndpointStoreError) -> AtmError {
    match error {
        GraftEndpointStoreError::NotOwner => AtmError::new(
            AtmErrorCode::GraftReceiverNotOwner,
            "graft receiver lease is owned by another generation",
        ),
        GraftEndpointStoreError::Absent => AtmError::new(
            AtmErrorCode::GraftReceiverNotRegistered,
            "graft receiver lease is absent; re-announcement required",
        ),
        GraftEndpointStoreError::AlreadyActive => {
            AtmError::validation("graft receiver lease is already active")
        }
        GraftEndpointStoreError::Storage {
            code,
            message,
            cause,
        } => {
            let error = AtmError::new(code, message);
            match cause {
                Some(cause) => error.with_cause(cause),
                None => error,
            }
        }
    }
}

fn load_workspace_config(
    access: WorkspaceConfigAccess,
    current_dir: &Path,
) -> Result<Option<AtmConfig>, AtmError> {
    match access {
        WorkspaceConfigAccess::Client => config::load_config(current_dir),
        WorkspaceConfigAccess::Disabled => Ok(None),
    }
}

#[cfg(test)]
mod workspace_config_tests {
    use super::{WorkspaceConfigAccess, load_workspace_config};

    #[test]
    fn disabled_runtime_never_reads_a_callers_workspace_config() {
        let workspace = tempfile::tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".atm.toml"), "not valid toml = [")
            .expect("fixture config");

        let config = load_workspace_config(WorkspaceConfigAccess::Disabled, workspace.path())
            .expect("daemon config access is disabled");

        assert!(config.is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GraftReceiverLeaseCache, LocalFileNonClaudeOutbound, MAX_NON_CLAUDE_PAYLOAD_BYTES,
        RosterSnapshotCache, append_notification_log_at_path,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::protocol::{NotificationEvent, NotificationKind};
    use crate::schema::InboxMessage;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use chrono::Utc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::tempdir;

    fn message() -> InboxMessage {
        InboxMessage {
            from: "sender".parse::<AgentName>().expect("sender"),
            source_chat_id: None,
            text: "hello".to_string(),
            timestamp: IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("test-team".parse::<TeamName>().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: None,
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    fn read_notification_events(path: &std::path::Path) -> Vec<NotificationEvent> {
        std::fs::read_to_string(path)
            .expect("notifications")
            .lines()
            .map(|line| serde_json::from_str(line).expect("notification event"))
            .collect()
    }

    #[test]
    fn notification_logging_appends_directly_at_event_site() {
        let tempdir = tempdir().expect("tempdir");
        let notification_path = tempdir.path().join("notifications.jsonl");
        let event = NotificationEvent {
            kind: NotificationKind::Delivery,
            detail: "runtime-direct".to_string(),
            team: Some("test-team".parse::<TeamName>().expect("team")),
            agent: Some("recipient".parse::<AgentName>().expect("agent")),
        };

        append_notification_log_at_path(&notification_path, &event).expect("direct log append");

        let direct_events = read_notification_events(&notification_path);
        assert_eq!(direct_events.len(), 1);
        assert_eq!(direct_events[0].detail, "runtime-direct");
    }

    #[test]
    fn local_non_claude_outbound_rejects_oversized_payloads() {
        let tempdir = tempdir().expect("tempdir");
        let output_path = tempdir.path().join("non_claude_outbound.jsonl");
        let runtime = LocalFileNonClaudeOutbound::new_for_test_with_path(output_path.clone());
        let oversized_body = "a".repeat(MAX_NON_CLAUDE_PAYLOAD_BYTES + 1);

        let error = crate::boundary::NonClaudeOutbound::deliver_payloads(
            &runtime,
            crate::boundary::NonClaudeOutboundDeliveryRequest {
                team: TeamName::from_validated("test-team"),
                agent: AgentName::from_validated("recipient"),
                recipient_pane_id: None,
                messages: vec![InboxMessage {
                    text: oversized_body,
                    ..message()
                }],
            },
        )
        .expect_err("oversized non-claude payload must fail");

        assert_eq!(error.code(), AtmErrorCode::MailboxWriteFailed);
        assert!(error.message().contains("exceeded"));
        assert!(error.message().contains("Recovery:"));
        assert!(!output_path.exists());
    }

    #[test]
    fn cached_roster_is_reused_until_explicit_reload_invalidation() {
        let cache = RosterSnapshotCache::default();
        let team = TeamName::from_validated("test-team");
        let loads = AtomicUsize::new(0);
        let empty_roster: Arc<[crate::boundary::RosterEntry]> = Arc::from([]);

        cache
            .load(&team, || {
                loads.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::clone(&empty_roster))
            })
            .expect("first roster lookup");
        cache
            .load(&team, || {
                loads.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::clone(&empty_roster))
            })
            .expect("cached roster lookup");
        assert_eq!(loads.load(Ordering::Relaxed), 1);

        cache.clear();
        cache
            .load(&team, || {
                loads.fetch_add(1, Ordering::Relaxed);
                Ok(empty_roster)
            })
            .expect("reloaded roster lookup");
        assert_eq!(loads.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn graft_lease_cache_avoids_a_control_path_lookup_per_local_admission() {
        let cache = GraftReceiverLeaseCache::default();
        let team = TeamName::from_validated("test-team");
        let agent = AgentName::from_validated("recipient");
        let durable_lookups = AtomicUsize::new(0);

        for _ in 0..64 {
            assert_eq!(
                cache
                    .load(&team, &agent, || {
                        durable_lookups.fetch_add(1, Ordering::Relaxed);
                        Ok(None)
                    })
                    .expect("lease cache lookup"),
                None
            );
        }
        assert_eq!(
            durable_lookups.load(Ordering::Relaxed),
            1,
            "a local admission burst must load an absent graft lease once, not borrow the control path per message"
        );

        cache.invalidate(&team, &agent);
        cache
            .load(&team, &agent, || {
                durable_lookups.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            })
            .expect("lookup after receiver lifecycle invalidation");
        assert_eq!(
            durable_lookups.load(Ordering::Relaxed),
            2,
            "registration, refresh, unregistration, and unreachable updates must invalidate the lease snapshot"
        );

        let expiry_cache = GraftReceiverLeaseCache::with_ttl(Duration::ZERO);
        for _ in 0..2 {
            expiry_cache
                .load(&team, &agent, || {
                    durable_lookups.fetch_add(1, Ordering::Relaxed);
                    Ok(None)
                })
                .expect("lookup after lease snapshot expiry");
        }
        assert_eq!(
            durable_lookups.load(Ordering::Relaxed),
            4,
            "an expired receiver snapshot must re-read durable state instead of surviving a restart"
        );
    }

    #[test]
    fn graft_lease_cache_does_not_serialize_unrelated_durable_misses() {
        let cache = Arc::new(GraftReceiverLeaseCache::default());
        let first_team = TeamName::from_validated("first-team");
        let second_team = TeamName::from_validated("second-team");
        let agent = AgentName::from_validated("recipient");
        let (first_started_tx, first_started_rx) = mpsc::sync_channel(1);
        let (release_first_tx, release_first_rx) = mpsc::sync_channel(0);
        let (second_done_tx, second_done_rx) = mpsc::sync_channel(1);

        std::thread::scope(|scope| {
            let first_cache = Arc::clone(&cache);
            let first_team = first_team.clone();
            let first_agent = agent.clone();
            let first = scope.spawn(move || {
                first_cache
                    .load(&first_team, &first_agent, || {
                        first_started_tx.send(()).expect("announce first lookup");
                        release_first_rx.recv().expect("release first lookup");
                        Ok(None)
                    })
                    .expect("first cache lookup");
            });
            first_started_rx.recv().expect("first lookup started");

            let second_cache = Arc::clone(&cache);
            let second_agent = agent.clone();
            scope.spawn(move || {
                let result = second_cache.load(&second_team, &second_agent, || Ok(None));
                second_done_tx.send(result).expect("report second lookup");
            });

            let second_result = second_done_rx.recv_timeout(Duration::from_secs(1));
            release_first_tx.send(()).expect("release first lookup");
            second_result
                .expect("an unrelated cache miss must not wait behind the first durable lookup")
                .expect("second cache lookup");
            first.join().expect("first lookup thread");
        });
    }

    // RBP-F001/RBP-F003: `graft_store_error` is the single canonical
    // `GraftEndpointStoreError` -> `AtmError` mapping. This covers all three
    // variants, including the ADR-056-reserved `AlreadyActive` (RBP-F003:
    // never constructed by the only production implementer today, but the
    // mapping for it must still be exercised and correct for the day a
    // caller that cannot prove same-host exclusivity needs it).
    #[test]
    fn graft_store_error_maps_all_variants_and_preserves_storage_code_and_cause() {
        use crate::service_runtime::graft_store_error;
        use atm_storage::contract::GraftEndpointStoreError;

        let not_owner = graft_store_error(GraftEndpointStoreError::NotOwner);
        assert_eq!(not_owner.code(), AtmErrorCode::GraftReceiverNotOwner);

        let already_active = graft_store_error(GraftEndpointStoreError::AlreadyActive);
        assert_eq!(already_active.code(), AtmErrorCode::MessageValidationFailed);
        assert!(already_active.message().contains("already active"));

        // A representative backend failure code distinct from the generic
        // `daemon_unavailable` this used to collapse into (RBP-F001): the
        // original code and cause survive the round trip through `Storage`.
        let storage_error = graft_store_error(GraftEndpointStoreError::Storage {
            code: AtmErrorCode::MailboxWriteFailed,
            message: "disk full".to_string(),
            cause: Some("ENOSPC".to_string()),
        });
        assert_eq!(storage_error.code(), AtmErrorCode::MailboxWriteFailed);
        assert!(storage_error.message().starts_with("disk full"));
        assert_eq!(storage_error.cause(), Some("ENOSPC"));
    }
}
