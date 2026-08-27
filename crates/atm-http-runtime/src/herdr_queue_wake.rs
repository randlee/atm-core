//! Tokio-owned polling pump for deferred Herdr queue nudges.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    DurableRosterStore, MemberKey, MessageReceivedHookSelector, NudgeKind, PendingNudgeStore,
};
use atm_core::delivery_channel::{
    DeliveryChannel, GraftLeaseState, HerdrSession, classify_delivery_channel,
    local_message_received_backend,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::rebuild_received_hook_dispatch;
use atm_core::protocol::{RuntimeMemberState, RuntimeObservationSource};
use atm_core::types::IsoTimestamp;
use atm_herdr::{AgentSnapshot, HerdrAgentStatus, HerdrProcessAdapter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::runtime_health::RuntimeHealth;

/// Poll cadence required by AQ2.7.
pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
/// Maximum number of prompts admitted by one poll tick.
pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
const HERDR_REQUEST_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrQueueWakeStats {
    pub listed_sessions: usize,
    pub pending_members: usize,
    pub idle_members: usize,
    pub prompted: usize,
    pub released: usize,
    pub breaker_open: usize,
    pub not_present: usize,
    pub last_tick_at: Option<IsoTimestamp>,
}

#[derive(Clone)]
pub struct HerdrQueueWakePump {
    service_runtime: LocalServiceRuntime,
    selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    cursor: Arc<Mutex<usize>>,
    last_stats: Arc<Mutex<HerdrQueueWakeStats>>,
}

impl HerdrQueueWakePump {
    #[must_use]
    pub fn new(
        service_runtime: LocalServiceRuntime,
        selector: Arc<dyn MessageReceivedHookSelector>,
        runtime_health: RuntimeHealth,
        herdr_process: Arc<dyn HerdrProcessAdapter>,
    ) -> Self {
        Self {
            service_runtime,
            selector,
            runtime_health,
            herdr_process,
            cursor: Arc::new(Mutex::new(0)),
            last_stats: Arc::new(Mutex::new(HerdrQueueWakeStats::default())),
        }
    }

    /// Starts the single polling task. The task owns no per-member workers.
    pub fn start(self: Arc<Self>, mut shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(HERDR_POLL_INTERVAL_MS));
            interval.tick().await;
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() {
                            break;
                        }
                    }
                    _ = interval.tick() => self.tick_once().await,
                }
            }
        })
    }

    /// Runs one complete roster/list/claim/dispatch pass.
    pub async fn tick_once(&self) {
        let mut stats = HerdrQueueWakeStats {
            last_tick_at: Some(IsoTimestamp::now()),
            ..HerdrQueueWakeStats::default()
        };
        let pending_store = match self.service_runtime.pending_nudge_store() {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(error = %error, "Herdr queue wake skipped: pending store unavailable");
                self.save_stats(stats);
                return;
            }
        };
        let roster_store = self.service_runtime.shared_roster_store_arc();
        let pending_members = match run_blocking({
            let pending_store = Arc::clone(&pending_store);
            move || pending_store.list_pending_members()
        })
        .await
        {
            Ok(members) => members,
            Err(error) => {
                tracing::warn!(error = %error, "Herdr queue wake skipped: pending roster unavailable");
                self.save_stats(stats);
                return;
            }
        };
        stats.pending_members = pending_members.len();
        let pending_set: HashSet<_> = pending_members.into_iter().collect();
        let candidates = match run_blocking({
            let pending_set = pending_set.clone();
            move || herdr_candidates(roster_store.as_ref(), &pending_set)
        })
        .await
        {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(error = %error, "Herdr queue wake skipped: roster unavailable");
                self.save_stats(stats);
                return;
            }
        };

        let mut by_session: HashMap<Option<HerdrSession>, Vec<HerdrCandidate>> = HashMap::new();
        for candidate in candidates {
            by_session
                .entry(candidate.session.clone())
                .or_default()
                .push(candidate);
        }
        let mut eligible = Vec::new();
        for (session, members) in by_session {
            stats.listed_sessions += 1;
            match self
                .herdr_process
                .list(
                    session.as_ref(),
                    RequestDeadline::after(HERDR_REQUEST_DEADLINE),
                )
                .await
            {
                Ok(outcome) => {
                    let snapshots: HashMap<&str, &AgentSnapshot> = outcome
                        .agents
                        .iter()
                        .filter_map(|snapshot| {
                            snapshot.name.as_deref().map(|name| (name, snapshot))
                        })
                        .collect();
                    for member in members {
                        let Some(snapshot) = snapshots.get(member.key.agent().as_str()) else {
                            if member.pending {
                                stats.not_present += 1;
                            }
                            continue;
                        };
                        if snapshot.status == HerdrAgentStatus::Unknown {
                            continue;
                        }
                        let state = runtime_state(snapshot.status);
                        self.runtime_health.record_observed_state(
                            &member.key,
                            state,
                            RuntimeObservationSource::HerdrPoll,
                        );
                        if member.pending
                            && matches!(
                                snapshot.status,
                                HerdrAgentStatus::Idle | HerdrAgentStatus::Done
                            )
                        {
                            stats.idle_members += 1;
                            eligible.push(member);
                        }
                    }
                }
                Err(error) => {
                    if error.is_infrastructure() {
                        stats.breaker_open += 1;
                    }
                    tracing::warn!(session = ?session, error = ?error, "Herdr queue wake list failed");
                }
            }
        }

        eligible.sort_by(|left, right| member_order(&left.key, &right.key));
        if !eligible.is_empty() {
            let start = *self
                .cursor
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                % eligible.len();
            let mut visited = 0;
            for offset in 0..eligible.len() {
                if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
                    break;
                }
                visited += 1;
                let member = &eligible[(start + offset) % eligible.len()];
                let claim = match run_blocking({
                    let pending_store = Arc::clone(&pending_store);
                    let member = member.key.clone();
                    move || pending_store.claim_next_pending(&member)
                })
                .await
                {
                    Ok(Some(claim)) => claim,
                    Ok(None) | Err(_) => continue,
                };
                let mut release = ReleasePendingOnDrop::new(
                    Arc::clone(&pending_store),
                    member.key.clone(),
                    claim.clone(),
                );
                let dispatch = match run_blocking({
                    let runtime = self.service_runtime.clone();
                    let member = member.key.clone();
                    move || {
                        rebuild_received_hook_dispatch(
                            &runtime,
                            &member,
                            claim.msg,
                            NudgeKind::Queue,
                        )
                    }
                })
                .await
                {
                    Ok(Some(dispatch)) => dispatch,
                    Ok(None) | Err(_) => {
                        release.release();
                        stats.released += 1;
                        continue;
                    }
                };
                let Some(emitter) = self.selector.select_emitter(&dispatch) else {
                    release.release();
                    stats.released += 1;
                    continue;
                };
                match emitter
                    .emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE))
                    .await
                {
                    Ok(_) => {
                        release.disarm();
                        stats.prompted += 1;
                    }
                    Err(error) => {
                        if error.code() == AtmErrorCode::HerdrUnavailable {
                            stats.breaker_open += 1;
                        }
                        release.release();
                        stats.released += 1;
                    }
                }
            }
            *self
                .cursor
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                (start + visited) % eligible.len();
        }
        self.save_stats(stats.clone());
        tracing::info!(
            event = "herdr_queue_poll_tick",
            listed_sessions = stats.listed_sessions,
            pending_members = stats.pending_members,
            idle_members = stats.idle_members,
            prompted = stats.prompted,
            released = stats.released,
            breaker_open = stats.breaker_open,
            "Herdr queue wake poll tick"
        );
    }

    #[must_use]
    pub fn stats(&self) -> HerdrQueueWakeStats {
        self.last_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn save_stats(&self, stats: HerdrQueueWakeStats) {
        *self
            .last_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = stats;
    }
}

impl crate::RuntimeMaintenance for HerdrQueueWakePump {
    fn start(&self, shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        Arc::new(self.clone()).start(shutdown)
    }
}

#[derive(Clone)]
struct HerdrCandidate {
    key: MemberKey,
    session: Option<HerdrSession>,
    pending: bool,
}

fn herdr_candidates(
    roster_store: &dyn DurableRosterStore,
    pending: &HashSet<MemberKey>,
) -> Result<Vec<HerdrCandidate>, AtmError> {
    let mut candidates = Vec::new();
    let mut teams = roster_store.list_teams()?;
    teams.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    for team in teams {
        let roster = roster_store.load_roster(&team)?;
        for member in roster.members {
            let key = MemberKey::new(member.team_name.clone(), member.agent_name.clone());
            let Some(backend) = local_message_received_backend(&member) else {
                continue;
            };
            if classify_delivery_channel(Some(&backend), GraftLeaseState::Absent)
                != DeliveryChannel::HerdrSteer
            {
                continue;
            }
            let session = match backend {
                atm_core::delivery_channel::LocalMessageReceivedBackend::Herdr { session } => {
                    session
                }
                atm_core::delivery_channel::LocalMessageReceivedBackend::Tmux { .. } => continue,
            };
            candidates.push(HerdrCandidate {
                pending: pending.contains(&key),
                key,
                session,
            });
        }
    }
    candidates.sort_by(|left, right| member_order(&left.key, &right.key));
    Ok(candidates)
}

async fn run_blocking<T, F>(job: F) -> Result<T, AtmError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AtmError> + Send + 'static,
{
    tokio::task::spawn_blocking(job).await.map_err(|source| {
        AtmError::new(
            AtmErrorCode::InternalError,
            "Herdr queue wake blocking operation ended unexpectedly",
        )
        .with_cause(source)
    })?
}

fn member_order(left: &MemberKey, right: &MemberKey) -> std::cmp::Ordering {
    left.team()
        .as_str()
        .cmp(right.team().as_str())
        .then_with(|| left.agent().as_str().cmp(right.agent().as_str()))
}

fn runtime_state(status: HerdrAgentStatus) -> RuntimeMemberState {
    match status {
        HerdrAgentStatus::Idle | HerdrAgentStatus::Done => RuntimeMemberState::Idle,
        HerdrAgentStatus::Working | HerdrAgentStatus::Blocked => RuntimeMemberState::Active,
        HerdrAgentStatus::Unknown => RuntimeMemberState::Unknown,
    }
}

struct ReleasePendingOnDrop {
    store: Arc<dyn PendingNudgeStore + Send + Sync>,
    member: MemberKey,
    claim: atm_core::boundary::NudgeClaim,
    armed: bool,
}

impl ReleasePendingOnDrop {
    fn new(
        store: Arc<dyn PendingNudgeStore + Send + Sync>,
        member: MemberKey,
        claim: atm_core::boundary::NudgeClaim,
    ) -> Self {
        Self {
            store,
            member,
            claim,
            armed: true,
        }
    }

    fn release(&mut self) {
        if self.armed {
            if let Err(error) = self.store.release_pending(&self.member, &self.claim) {
                tracing::warn!(error = %error, member = %self.member, "failed to release Herdr queue claim");
            }
            self.armed = false;
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReleasePendingOnDrop {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::{HERDR_MAX_PROMPTS_PER_TICK, HERDR_POLL_INTERVAL_MS, runtime_state};
    use atm_core::protocol::RuntimeMemberState;
    use atm_herdr::HerdrAgentStatus;

    #[test]
    fn poll_contract_uses_fixed_cadence_and_cap() {
        assert_eq!(HERDR_POLL_INTERVAL_MS, 5_000);
        assert_eq!(HERDR_MAX_PROMPTS_PER_TICK, 16);
    }

    #[test]
    fn herdr_statuses_project_to_runtime_states() {
        assert_eq!(
            runtime_state(HerdrAgentStatus::Idle),
            RuntimeMemberState::Idle
        );
        assert_eq!(
            runtime_state(HerdrAgentStatus::Done),
            RuntimeMemberState::Idle
        );
        assert_eq!(
            runtime_state(HerdrAgentStatus::Working),
            RuntimeMemberState::Active
        );
        assert_eq!(
            runtime_state(HerdrAgentStatus::Unknown),
            RuntimeMemberState::Unknown
        );
    }
}
