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

/// Ephemeral per-member roster state that never round-trips through the
/// durable roster store. This struct is deliberately small and extensible:
/// every field here is mutated in RAM only, through explicit
/// [`LocalServiceRuntime`] methods, on an observed state change. Nothing here
/// is ever read from or written to SQLite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RosterMemberEphemeralState {
    /// Set/cleared by Herdr queue-wake bookkeeping when a member's steer
    /// target is pending a wake attempt.
    pub herdr_wake_pending: bool,
}

/// One team's write-through roster mirror: the durable roster columns plus
/// the ephemeral per-member state layered on top of them. The whole record is
/// replaced as one `Arc` on every mutation, so readers observe either the
/// entries-before or the entries-after a write, never a torn mix of the two.
#[derive(Clone, Debug, Default)]
struct TeamRosterRecord {
    entries: Arc<[crate::boundary::RosterEntry]>,
    ephemeral: Arc<BTreeMap<AgentName, RosterMemberEphemeralState>>,
}

impl TeamRosterRecord {
    fn from_durable(entries: Vec<crate::boundary::RosterEntry>) -> Self {
        let ephemeral = entries
            .iter()
            .map(|entry| {
                (
                    entry.agent_name.clone(),
                    RosterMemberEphemeralState::default(),
                )
            })
            .collect();
        Self {
            entries: entries.into(),
            ephemeral: Arc::new(ephemeral),
        }
    }

    /// Builds the record a durable roster replacement produces: retained
    /// members keep their ephemeral state, new members start with the
    /// default (empty) ephemeral state, and removed members' ephemeral state
    /// is dropped with them.
    fn replaced_with(&self, members: &[crate::boundary::RosterEntry]) -> Self {
        let ephemeral = members
            .iter()
            .map(|member| {
                let state = self
                    .ephemeral
                    .get(&member.agent_name)
                    .copied()
                    .unwrap_or_default();
                (member.agent_name.clone(), state)
            })
            .collect();
        Self {
            entries: members.to_vec().into(),
            ephemeral: Arc::new(ephemeral),
        }
    }
}

/// Runtime-owned, write-through in-memory roster mirror: one record per
/// team, holding durable roster columns plus the ephemeral state columns
/// that never appear in the database.
///
/// This is hydrated from the durable roster store once at runtime startup
/// (see [`LocalServiceRuntime::new_with_delivery_boundaries`]) and is
/// updated in the same operation as every durable roster write thereafter
/// (see [`RosterRuntimeView`]). No consumer reads the durable store after
/// startup; a control-plane reload only re-hydrates this mirror, it is never
/// the only synchronization mechanism.
///
/// Race ordering between a reader and a concurrent add/update/remove is
/// intentionally not guaranteed beyond memory safety: the only invariant
/// enforced is that one durable write and its RAM update happen in the same
/// operation.
#[derive(Default)]
struct RosterRuntimeState {
    teams: RwLock<BTreeMap<TeamName, Arc<TeamRosterRecord>>>,
}

impl RosterRuntimeState {
    /// Seeds one team's record straight from a durable read. Used only at
    /// startup hydration and at an explicit reload re-hydration; never on a
    /// per-request or per-tick path.
    fn hydrate(&self, team: TeamName, entries: Vec<crate::boundary::RosterEntry>) {
        let mut teams = self
            .teams
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        teams.insert(team, Arc::new(TeamRosterRecord::from_durable(entries)));
    }

    /// Replaces one team's *durable* roster columns in RAM in the same
    /// operation as the durable write that produced `members`. An empty
    /// roster drops the team entirely, mirroring the durable store's
    /// `list_teams` semantics (a team with zero roster rows is not
    /// enumerable).
    fn replace_roster(&self, team: &TeamName, members: &[crate::boundary::RosterEntry]) {
        let mut teams = self
            .teams
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if members.is_empty() {
            teams.remove(team);
            return;
        }
        let next = match teams.get(team) {
            Some(existing) => existing.replaced_with(members),
            None => TeamRosterRecord::from_durable(members.to_vec()),
        };
        teams.insert(team.clone(), Arc::new(next));
    }

    fn team_record(&self, team: &TeamName) -> Option<Arc<TeamRosterRecord>> {
        let teams = self
            .teams
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        teams.get(team).cloned()
    }

    fn load_team_roster(&self, team: &TeamName) -> Vec<crate::boundary::RosterEntry> {
        self.team_record(team)
            .map(|record| record.entries.to_vec())
            .unwrap_or_default()
    }

    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Option<crate::boundary::RosterEntry> {
        self.team_record(team)?
            .entries
            .iter()
            .find(|member| &member.agent_name == agent)
            .cloned()
    }

    fn list_teams(&self) -> Vec<TeamName> {
        let teams = self
            .teams
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        teams.keys().cloned().collect()
    }

    fn ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Option<RosterMemberEphemeralState> {
        self.team_record(team)?.ephemeral.get(agent).copied()
    }

    /// Mutates one member's ephemeral state in RAM only. Returns `false`
    /// without effect when the member is not present in the current
    /// snapshot; there is nothing durable to attach ephemeral state to.
    fn set_ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        mutate: impl FnOnce(&mut RosterMemberEphemeralState),
    ) -> bool {
        let mut teams = self
            .teams
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(record) = teams.get(team) else {
            return false;
        };
        if !record
            .entries
            .iter()
            .any(|member| &member.agent_name == agent)
        {
            return false;
        }
        let mut ephemeral = (*record.ephemeral).clone();
        let state = ephemeral.entry(agent.clone()).or_default();
        mutate(state);
        let next = TeamRosterRecord {
            entries: Arc::clone(&record.entries),
            ephemeral: Arc::new(ephemeral),
        };
        teams.insert(team.clone(), Arc::new(next));
        true
    }
}

/// The single write-through seam for the durable roster store: every write
/// updates the runtime-owned RAM mirror in the same operation, and every
/// read is served from RAM. This is what [`LocalServiceRuntime::shared_roster_store_arc`]
/// returns; nothing downstream of it reaches SQLite except the one durable
/// write this performs.
#[derive(Clone)]
struct RosterRuntimeView {
    durable: std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
    state: Arc<RosterRuntimeState>,
}

impl atm_storage::contract::sealed::Sealed for RosterRuntimeView {}

impl SharedRosterStore for RosterRuntimeView {
    fn load_roster(&self, team: &TeamName) -> Result<atm_storage::RosterSnapshot, AtmError> {
        Ok(atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: self.state.load_team_roster(team),
            refreshed_at: None,
        })
    }

    fn save_roster(&self, roster: &atm_storage::RosterSnapshot) -> Result<(), AtmError> {
        self.durable.save_roster(roster)?;
        self.state
            .replace_roster(&roster.team_name, &roster.members);
        Ok(())
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        Ok(self.state.list_teams())
    }
}

/// Hydrates the RAM roster mirror from the durable roster store. This is the
/// only place a roster read reaches SQLite outside of an explicit
/// control-plane reload; every consumer after this call reads RAM.
///
/// Best-effort: a durable read failure during startup hydration is logged
/// and leaves the affected team absent from RAM rather than failing runtime
/// construction. A later write-through mutation (or an explicit reload)
/// still recovers a correct RAM view for that team.
fn hydrate_roster_runtime_from_durable(
    durable: &std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
    state: &Arc<RosterRuntimeState>,
) {
    let teams = match durable.list_teams() {
        Ok(teams) => teams,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "roster runtime hydration could not enumerate durable teams; RAM roster starts empty"
            );
            return;
        }
    };
    for team in teams {
        match durable.load_roster(&team) {
            Ok(snapshot) => state.hydrate(team, snapshot.members),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    team = %team,
                    "roster runtime hydration could not load one team's durable roster"
                );
            }
        }
    }
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
    /// The runtime-owned, write-through RAM roster mirror used by every
    /// roster consumer (admission, send/recipient resolution, Herdr
    /// queue-wake, doctor/diagnostics, graft). Hydrated once from the
    /// durable roster store at construction; every subsequent durable roster
    /// write updates it in the same operation through
    /// [`RosterRuntimeView`]. No consumer reads durable roster state on a
    /// per-request or per-tick path.
    roster_runtime: Arc<RosterRuntimeState>,
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
        let roster_runtime = Arc::new(RosterRuntimeState::default());
        hydrate_roster_runtime_from_durable(&roster_store, &roster_runtime);
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
            roster_runtime,
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

    /// Reads one roster member from the RAM roster mirror. Never issues a
    /// durable roster read.
    pub fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError> {
        Ok(self.roster_runtime.load_roster_member(team, agent))
    }

    /// Reads one team's roster from the RAM roster mirror. Never issues a
    /// durable roster read.
    pub fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError> {
        Ok(self.roster_runtime.load_team_roster(team))
    }

    /// Enumerates every team the RAM roster mirror currently holds. Never
    /// issues a durable roster read.
    pub fn list_roster_teams(&self) -> Vec<TeamName> {
        self.roster_runtime.list_teams()
    }

    /// Reads one member's ephemeral (non-durable) roster state from RAM.
    /// Returns `None` when the member is not present in the current roster
    /// snapshot.
    pub fn roster_ephemeral_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Option<RosterMemberEphemeralState> {
        self.roster_runtime.ephemeral_state(team, agent)
    }

    /// Sets one member's Herdr wake-pending ephemeral flag in RAM only.
    /// Returns `false` without effect when the member is not present in the
    /// current roster snapshot.
    pub fn set_roster_herdr_wake_pending(
        &self,
        team: &TeamName,
        agent: &AgentName,
        pending: bool,
    ) -> bool {
        self.roster_runtime
            .set_ephemeral_state(team, agent, |state| state.herdr_wake_pending = pending)
    }

    /// Re-hydrates the RAM roster mirror from the durable roster store.
    ///
    /// This is *not* the write-through synchronization mechanism -- every
    /// durable roster write already updates RAM in the same operation
    /// through [`RosterRuntimeView`]. This method exists only for the
    /// authenticated control-plane reload boundary, which may want to
    /// re-derive RAM from durable state (e.g. after an out-of-band durable
    /// migration); it is never required for correctness of the normal
    /// mutation path.
    pub fn reload_roster_from_durable_store(&self) {
        hydrate_roster_runtime_from_durable(&self.roster_store, &self.roster_runtime);
    }

    /// Returns the single write-through roster seam used by every roster
    /// consumer.
    ///
    /// Reads served through this handle come from the RAM roster mirror,
    /// never SQLite. A write performed through this handle durably persists
    /// first, then updates the RAM mirror in the same operation. The only
    /// SQLite roster reads outside of this handle happen once, at
    /// construction (see [`LocalServiceRuntime::new_with_delivery_boundaries`])
    /// or at an explicit [`LocalServiceRuntime::reload_roster_from_durable_store`]
    /// call.
    #[doc(hidden)]
    pub fn shared_roster_store_arc(&self) -> std::sync::Arc<dyn SharedRosterStore + Send + Sync> {
        std::sync::Arc::new(RosterRuntimeView {
            durable: self.roster_store.clone(),
            state: Arc::clone(&self.roster_runtime),
        })
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
        RosterRuntimeState, RosterRuntimeView, append_notification_log_at_path,
    };
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::protocol::{NotificationEvent, NotificationKind};
    use crate::schema::InboxMessage;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use atm_storage::RosterStore as SharedRosterStore;
    use chrono::Utc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    /// Failure ceiling for the gated concurrency tests below. Every wait is
    /// released by an observed event; this bound only turns a regression that
    /// would hang into a reported failure.
    const WAIT_CEILING: Duration = Duration::from_secs(30);

    /// Repetitions of the invalidate-versus-load race. Which contender wins
    /// the entry lock is scheduler-chosen, so a single attempt can miss the
    /// interleaving a regression corrupts; repeating costs microseconds and
    /// makes detection reliable without any timing assumption.
    const RACE_ATTEMPTS: usize = 64;
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

    /// A durable roster double that counts every `save_roster`/`load_roster`/
    /// `list_teams` call, used to prove that RAM reads never fall through to
    /// the durable store once hydrated.
    #[derive(Default)]
    struct CountingRosterStore {
        rosters: std::sync::Mutex<
            std::collections::BTreeMap<TeamName, Vec<crate::boundary::RosterEntry>>,
        >,
        save_calls: AtomicUsize,
        load_calls: AtomicUsize,
        list_calls: AtomicUsize,
    }

    impl atm_storage::contract::sealed::Sealed for CountingRosterStore {}

    impl SharedRosterStore for CountingRosterStore {
        fn load_roster(&self, team: &TeamName) -> Result<atm_storage::RosterSnapshot, AtmError> {
            self.load_calls.fetch_add(1, Ordering::Relaxed);
            let rosters = self
                .rosters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Ok(atm_storage::RosterSnapshot {
                team_name: team.clone(),
                members: rosters.get(team).cloned().unwrap_or_default(),
                refreshed_at: None,
            })
        }

        fn save_roster(&self, roster: &atm_storage::RosterSnapshot) -> Result<(), AtmError> {
            self.save_calls.fetch_add(1, Ordering::Relaxed);
            let mut rosters = self
                .rosters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            rosters.insert(roster.team_name.clone(), roster.members.clone());
            Ok(())
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            self.list_calls.fetch_add(1, Ordering::Relaxed);
            let rosters = self
                .rosters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Ok(rosters.keys().cloned().collect())
        }
    }

    fn test_roster_entry(team: &TeamName, agent: &str) -> crate::boundary::RosterEntry {
        crate::boundary::RosterEntry {
            team_name: team.clone(),
            agent_name: agent.parse::<AgentName>().expect("agent"),
            member_kind: crate::boundary::RosterMemberKind::Permanent,
            harness: crate::boundary::RosterHarness::CodexCli,
            agent_type: crate::schema::AgentType::default(),
            model: Default::default(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

    #[test]
    fn write_through_updates_ram_in_the_same_operation_as_the_durable_write() {
        let team = TeamName::from_validated("test-team");
        let durable: Arc<dyn SharedRosterStore + Send + Sync> =
            Arc::new(CountingRosterStore::default());
        let state = Arc::new(RosterRuntimeState::default());
        let view = RosterRuntimeView {
            durable: durable.clone(),
            state: Arc::clone(&state),
        };

        // add
        view.save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![test_roster_entry(&team, "agent-a")],
            refreshed_at: None,
        })
        .expect("add member write-through");
        assert_eq!(state.load_team_roster(&team).len(), 1);
        assert!(
            state
                .load_roster_member(&team, &"agent-a".parse().expect("agent"))
                .is_some()
        );

        // update (replace with a changed member set, same agent retained)
        let mut updated = test_roster_entry(&team, "agent-a");
        updated.harness = crate::boundary::RosterHarness::ClaudeCode;
        view.save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![updated],
            refreshed_at: None,
        })
        .expect("update member write-through");
        let after_update = state
            .load_roster_member(&team, &"agent-a".parse().expect("agent"))
            .expect("member retained after update");
        assert_eq!(
            after_update.harness,
            crate::boundary::RosterHarness::ClaudeCode
        );

        // remove (empty roster drops the team from RAM entirely)
        view.save_roster(&atm_storage::RosterSnapshot {
            team_name: team.clone(),
            members: vec![],
            refreshed_at: None,
        })
        .expect("remove member write-through");
        assert!(state.load_team_roster(&team).is_empty());
        assert!(!state.list_teams().contains(&team));
    }

    #[test]
    fn ram_read_after_hydration_never_calls_the_durable_store_again() {
        let team = TeamName::from_validated("test-team");
        let counting = Arc::new(CountingRosterStore::default());
        counting
            .save_roster(&atm_storage::RosterSnapshot {
                team_name: team.clone(),
                members: vec![test_roster_entry(&team, "agent-a")],
                refreshed_at: None,
            })
            .expect("seed durable roster");

        let durable: Arc<dyn SharedRosterStore + Send + Sync> = counting.clone();
        let state = Arc::new(RosterRuntimeState::default());
        super::hydrate_roster_runtime_from_durable(&durable, &state);
        assert_eq!(counting.list_calls.load(Ordering::Relaxed), 1);
        assert_eq!(counting.load_calls.load(Ordering::Relaxed), 1);

        let view = RosterRuntimeView {
            durable: durable.clone(),
            state: Arc::clone(&state),
        };
        for _ in 0..64 {
            view.load_roster(&team).expect("ram roster read");
            view.list_teams().expect("ram team enumeration");
        }

        assert_eq!(
            counting.load_calls.load(Ordering::Relaxed),
            1,
            "reads after hydration must never reach the durable store"
        );
        assert_eq!(
            counting.list_calls.load(Ordering::Relaxed),
            1,
            "team enumeration after hydration must never reach the durable store"
        );
    }

    #[test]
    fn ram_roster_enumerates_every_team_without_a_durable_read() {
        let counting = Arc::new(CountingRosterStore::default());
        let team_a = TeamName::from_validated("team-a");
        let team_b = TeamName::from_validated("team-b");
        counting
            .save_roster(&atm_storage::RosterSnapshot {
                team_name: team_a.clone(),
                members: vec![test_roster_entry(&team_a, "agent-a")],
                refreshed_at: None,
            })
            .expect("seed team-a");
        counting
            .save_roster(&atm_storage::RosterSnapshot {
                team_name: team_b.clone(),
                members: vec![test_roster_entry(&team_b, "agent-b")],
                refreshed_at: None,
            })
            .expect("seed team-b");

        let durable: Arc<dyn SharedRosterStore + Send + Sync> = counting.clone();
        let state = Arc::new(RosterRuntimeState::default());
        super::hydrate_roster_runtime_from_durable(&durable, &state);

        let mut teams = state.list_teams();
        teams.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        assert_eq!(teams, vec![team_a, team_b]);
    }

    #[test]
    fn ephemeral_state_mutates_in_ram_and_is_visible_to_readers() {
        let team = TeamName::from_validated("test-team");
        let agent: AgentName = "agent-a".parse().expect("agent");
        let state = RosterRuntimeState::default();
        state.hydrate(team.clone(), vec![test_roster_entry(&team, "agent-a")]);

        assert_eq!(
            state.ephemeral_state(&team, &agent),
            Some(super::RosterMemberEphemeralState::default())
        );

        let updated = state.set_ephemeral_state(&team, &agent, |ephemeral| {
            ephemeral.herdr_wake_pending = true;
        });
        assert!(updated);
        assert!(
            state
                .ephemeral_state(&team, &agent)
                .expect("member present")
                .herdr_wake_pending
        );

        // Durable roster columns for the retained member are unaffected by
        // the ephemeral-only mutation.
        assert_eq!(state.load_team_roster(&team).len(), 1);

        // A replace_roster that retains the member must retain its ephemeral
        // state; a replace_roster that drops the member must drop it too.
        state.replace_roster(&team, &[test_roster_entry(&team, "agent-a")]);
        assert!(
            state
                .ephemeral_state(&team, &agent)
                .expect("member retained")
                .herdr_wake_pending,
            "ephemeral state must survive a durable roster replace that retains the member"
        );

        state.replace_roster(&team, &[]);
        assert_eq!(state.ephemeral_state(&team, &agent), None);
    }

    #[test]
    fn ephemeral_mutation_on_an_unknown_member_is_a_no_op() {
        let team = TeamName::from_validated("test-team");
        let agent: AgentName = "ghost".parse().expect("agent");
        let state = RosterRuntimeState::default();
        state.hydrate(team.clone(), vec![]);

        let updated = state.set_ephemeral_state(&team, &agent, |ephemeral| {
            ephemeral.herdr_wake_pending = true
        });
        assert!(!updated);
        assert_eq!(state.ephemeral_state(&team, &agent), None);
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

    /// A durable lease fixture; only its identity matters to the cache, which
    /// never inspects the value it is asked to retain.
    fn registered_lease() -> atm_storage::GraftReceiverLease {
        atm_storage::GraftReceiverLease {
            endpoint: std::net::SocketAddr::new(
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                0,
            ),
            capability: atm_storage::LocalCapability::generate().expect("capability"),
            owner_generation: atm_storage::OwnerGeneration::new(ulid::Ulid::new().to_string())
                .expect("owner generation"),
            registered_at: Utc::now(),
            last_seen_at: Utc::now(),
            unreachable_since: None,
        }
    }

    /// QA-001 / ATM-QA-009: `invalidate` racing an in-flight `load` on the
    /// *same* key. Per-key coalescing is what makes this reachable: a
    /// receiver lifecycle mutation can land while an admission is already
    /// inside its durable lookup, and the value that lookup read before the
    /// mutation must not survive it.
    ///
    /// Every wait is released by an observed event, never by elapsed time:
    /// the durable lookup blocks on a channel until this test releases it, so
    /// the invalidation is issued while the lookup is provably in flight.
    /// Which of the two contenders reaches the entry lock first is the
    /// operating system's choice, so the race is repeated; the invariants
    /// below must hold under either interleaving.
    #[test]
    fn graft_lease_cache_invalidation_racing_a_same_key_load_never_serves_a_stale_lease() {
        for _ in 0..RACE_ATTEMPTS {
            invalidate_a_lease_lookup_in_flight();
        }
    }

    /// The shared state one `invalidate`-versus-`load` race is run against: a
    /// cache holding no snapshot yet, and durable state that starts at a
    /// registered lease and is unregistered mid-race.
    struct LeaseRaceFixture {
        cache: Arc<GraftReceiverLeaseCache>,
        team: TeamName,
        agent: AgentName,
        lease: atm_storage::GraftReceiverLease,
        durable_lease: Arc<std::sync::Mutex<Option<atm_storage::GraftReceiverLease>>>,
        durable_lookups: Arc<AtomicUsize>,
    }

    impl LeaseRaceFixture {
        fn new() -> Self {
            let lease = registered_lease();
            Self {
                cache: Arc::new(GraftReceiverLeaseCache::default()),
                team: TeamName::from_validated("test-team"),
                agent: AgentName::from_validated("recipient"),
                durable_lease: Arc::new(std::sync::Mutex::new(Some(lease.clone()))),
                durable_lookups: Arc::new(AtomicUsize::new(0)),
                lease,
            }
        }

        /// One counted read of durable receiver state, as a durable lookup
        /// passed to [`GraftReceiverLeaseCache::load`] would perform.
        fn durable_read(&self) -> Option<atm_storage::GraftReceiverLease> {
            self.durable_lookups.fetch_add(1, Ordering::SeqCst);
            self.durable_lease.lock().expect("durable lease").clone()
        }

        /// The receiver unregisters: durable state goes from a registered
        /// lease to none.
        fn unregister(&self) {
            *self.durable_lease.lock().expect("durable lease") = None;
        }
    }

    /// One `invalidate`-versus-`load` race on a single key.
    ///
    /// # Panics
    ///
    /// Panics if a waiter is served the pre-invalidation lease after the
    /// invalidation completed, if `invalidate` returns while the lookup it
    /// raced is still running, or if any participant fails to make progress
    /// within [`WAIT_CEILING`].
    fn invalidate_a_lease_lookup_in_flight() {
        let fixture = LeaseRaceFixture::new();
        race_invalidate_against_an_in_flight_lookup(&fixture);
        assert_no_stale_lease_survives_the_invalidation(&fixture);
    }

    /// Drives the race itself: a durable lookup held in flight by a channel, a
    /// same-key admission coalesced onto it, and an `invalidate` issued while
    /// that lookup is provably still running.
    fn race_invalidate_against_an_in_flight_lookup(fixture: &LeaseRaceFixture) {
        let lookup_returned = std::sync::atomic::AtomicBool::new(false);
        let (lookup_started_tx, lookup_started_rx) = mpsc::sync_channel(1);
        let (release_lookup_tx, release_lookup_rx) = mpsc::sync_channel(0);
        let (invalidating_tx, invalidating_rx) = mpsc::sync_channel(1);
        let (waiter_done_tx, waiter_done_rx) = mpsc::sync_channel(1);
        let (invalidated_tx, invalidated_rx) = mpsc::sync_channel(1);

        std::thread::scope(|scope| {
            let returned = &lookup_returned;
            let loader = scope.spawn(move || {
                fixture
                    .cache
                    .load(&fixture.team, &fixture.agent, || {
                        // Reads durable state before the unregistration below
                        // and returns after it: exactly the ordering that
                        // would let a stale lease outlive the mutation.
                        let lease = fixture.durable_read();
                        lookup_started_tx.send(()).expect("announce durable lookup");
                        release_lookup_rx.recv().expect("release durable lookup");
                        returned.store(true, Ordering::SeqCst);
                        Ok(lease)
                    })
                    .expect("in-flight lease lookup")
            });
            lookup_started_rx
                .recv_timeout(WAIT_CEILING)
                .expect("the durable lookup is in flight");

            // A second admission for the same receiver coalesces onto the
            // in-flight lookup instead of starting its own.
            scope.spawn(move || {
                let result = fixture
                    .cache
                    .load(&fixture.team, &fixture.agent, || Ok(fixture.durable_read()));
                waiter_done_tx
                    .send(result)
                    .expect("report coalesced waiter");
            });

            fixture.unregister();
            scope.spawn(move || {
                invalidating_tx.send(()).expect("announce invalidation");
                fixture.cache.invalidate(&fixture.team, &fixture.agent);
                // Observed inside the racing thread: returning here while the
                // raced lookup is still running would let its older value be
                // cached after the mutation completed.
                let observed = returned.load(Ordering::SeqCst);
                invalidated_tx.send(observed).expect("report invalidation");
            });
            invalidating_rx
                .recv_timeout(WAIT_CEILING)
                .expect("the invalidation thread reached the cache");

            release_lookup_tx.send(()).expect("release durable lookup");
            assert_race_participants_agree(fixture, loader, &invalidated_rx, &waiter_done_rx);
        });
    }

    /// The invariants every interleaving of the race must satisfy.
    fn assert_race_participants_agree(
        fixture: &LeaseRaceFixture,
        loader: std::thread::ScopedJoinHandle<'_, Option<atm_storage::GraftReceiverLease>>,
        invalidated_rx: &mpsc::Receiver<bool>,
        waiter_done_rx: &mpsc::Receiver<
            Result<Option<atm_storage::GraftReceiverLease>, crate::service_runtime::AtmError>,
        >,
    ) {
        assert_eq!(
            loader.join().expect("loader thread"),
            Some(fixture.lease.clone()),
            "the in-flight lookup returns the state it read, not a torn value"
        );
        assert!(
            invalidated_rx
                .recv_timeout(WAIT_CEILING)
                .expect("invalidation must not deadlock behind the in-flight lookup"),
            "invalidate must not return while the lookup it raced is still running, or \
             a lookup that began before the receiver mutation could be cached after it"
        );

        let waited = waiter_done_rx
            .recv_timeout(WAIT_CEILING)
            .expect("a coalesced waiter must not deadlock behind the invalidation")
            .expect("coalesced waiter lookup");
        assert!(
            waited.is_none() || waited == Some(fixture.lease.clone()),
            "a coalesced waiter is served one of the two durable states, never a \
             synthesized one"
        );
    }

    /// The invalidation has completed. Well inside the snapshot TTL, the next
    /// admission must still observe post-invalidation durable state.
    fn assert_no_stale_lease_survives_the_invalidation(fixture: &LeaseRaceFixture) {
        let after_invalidation = fixture
            .cache
            .load(&fixture.team, &fixture.agent, || Ok(fixture.durable_read()))
            .expect("lookup after invalidation");
        assert_eq!(
            after_invalidation, None,
            "no load after a completed invalidation may be served the pre-invalidation lease"
        );
        assert!(
            fixture.durable_lookups.load(Ordering::SeqCst) >= 2,
            "an invalidation that raced an in-flight lookup must still force a fresh \
             durable read rather than retaining the raced snapshot"
        );
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
