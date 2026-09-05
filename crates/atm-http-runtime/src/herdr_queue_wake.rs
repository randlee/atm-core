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
use atm_core::protocol::RuntimeMemberState;
use atm_core::types::IsoTimestamp;
use atm_herdr::{AgentSnapshot, HerdrAgentStatus, HerdrProcessAdapter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::runtime_health::RuntimeHealth;

/// Poll cadence required by AQ2.7.
pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
/// Maximum number of prompts admitted by one poll tick.
pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
/// Consecutive no-input releases before one retry-budget attempt is spent.
pub const HERDR_MAX_CONSECUTIVE_RELEASES: u32 = 10;
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
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
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
            release_streaks: Arc::new(Mutex::new(HashMap::new())),
            last_stats: Arc::new(Mutex::new(HerdrQueueWakeStats::default())),
        }
    }

    /// Starts the single polling task. The task owns no per-member workers.
    pub fn start(self: Arc<Self>, mut shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(HERDR_POLL_INTERVAL_MS));
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        let _ = changed;
                        break;
                    }
                    _ = interval.tick() => {
                        tokio::select! {
                            changed = shutdown.changed() => {
                                let _ = changed;
                                break;
                            }
                            _ = self.tick_once() => {}
                        }
                    }
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

        let eligible = self.list_eligible(candidates, &mut stats).await;
        self.drain_eligible(pending_store, eligible, &mut stats)
            .await;
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

    async fn list_eligible(
        &self,
        candidates: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
    ) -> Vec<HerdrCandidate> {
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
                    self.collect_idle_members(outcome.agents, members, stats, &mut eligible)
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
        eligible
    }

    fn collect_idle_members(
        &self,
        agents: Vec<AgentSnapshot>,
        members: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
        eligible: &mut Vec<HerdrCandidate>,
    ) {
        let snapshots: HashMap<&str, &AgentSnapshot> = agents
            .iter()
            .filter_map(|snapshot| snapshot.name.as_deref().map(|name| (name, snapshot)))
            .collect();
        for member in members {
            let Some(snapshot) = snapshots.get(member.key.agent().as_str()) else {
                if member.pending {
                    stats.not_present += 1;
                    tracing::info!(
                        event = "herdr_queue_poll_outcome",
                        member = %member.key,
                        queue_kind = NudgeKind::Queue.as_str(),
                        outcome = "held_target_not_present",
                        "Herdr queue target was absent from the poll result"
                    );
                }
                continue;
            };
            if snapshot.status == HerdrAgentStatus::Unknown {
                continue;
            }
            self.runtime_health
                .record_herdr_poll_state(&member.key, runtime_state(snapshot.status));
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

    async fn drain_eligible(
        &self,
        pending_store: Arc<dyn PendingNudgeStore + Send + Sync>,
        eligible: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
    ) {
        if eligible.is_empty() {
            return;
        }
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
            self.process_candidate(
                &pending_store,
                &eligible[(start + offset) % eligible.len()],
                stats,
            )
            .await;
        }
        *self
            .cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = (start + visited) % eligible.len();
    }

    async fn process_candidate(
        &self,
        pending_store: &Arc<dyn PendingNudgeStore + Send + Sync>,
        member: &HerdrCandidate,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let claim = match run_blocking({
            let pending_store = Arc::clone(pending_store);
            let member = member.key.clone();
            move || pending_store.claim_next_pending(&member)
        })
        .await
        {
            Ok(Some(claim)) => claim,
            Ok(None) | Err(_) => return,
        };
        let mut release = ReleasePendingOnDrop::new(
            Arc::clone(pending_store),
            member.key.clone(),
            claim.clone(),
            Arc::clone(&self.release_streaks),
        );
        let dispatch = match self.rebuild_dispatch(member, claim.msg).await {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) | Err(_) => {
                release.release_without_input();
                stats.released += 1;
                tracing::info!(
                    event = "herdr_queue_poll_outcome",
                    member = %member.key,
                    msg_id = %claim.msg,
                    queue_kind = NudgeKind::Queue.as_str(),
                    outcome = "dispatch_failed_released",
                    "Herdr queue dispatch could not be rebuilt"
                );
                return;
            }
        };
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            release.release_without_input();
            stats.released += 1;
            tracing::info!(
                event = "herdr_queue_poll_outcome",
                member = %member.key,
                msg_id = %claim.msg,
                queue_kind = NudgeKind::Queue.as_str(),
                outcome = "held_target_not_present",
                "Herdr queue selector returned no emitter"
            );
            return;
        };
        self.emit_claim(emitter, dispatch, member, claim, &mut release, stats)
            .await;
    }

    async fn rebuild_dispatch(
        &self,
        member: &HerdrCandidate,
        message_id: atm_core::schema::AtmMessageId,
    ) -> Result<Option<atm_core::boundary::BuiltInPostSendDispatch>, AtmError> {
        let runtime = self.service_runtime.clone();
        let member_key = member.key.clone();
        run_blocking(move || {
            rebuild_received_hook_dispatch(&runtime, &member_key, message_id, NudgeKind::Queue)
        })
        .await
    }

    async fn emit_claim(
        &self,
        emitter: &dyn atm_core::boundary::AsyncMessageReceivedHookEmitter,
        dispatch: atm_core::boundary::BuiltInPostSendDispatch,
        member: &HerdrCandidate,
        claim: atm_core::boundary::NudgeClaim,
        release: &mut ReleasePendingOnDrop,
        stats: &mut HerdrQueueWakeStats,
    ) {
        match emitter
            .emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE))
            .await
        {
            Ok(_) => {
                let runtime = self.service_runtime.clone();
                let member_key = member.key.clone();
                let message_id = claim.msg;
                let health = self.runtime_health.clone();
                let _ = run_blocking(move || {
                    atm_core::nudge_dispatch::clear_queue_marker_after_handoff(
                        &runtime,
                        &member_key,
                        &message_id,
                        || health.record_graft_queue_marker_clear_failure(),
                    );
                    Ok(())
                })
                .await;
                self.reset_release_streak(&member.key);
                release.disarm();
                stats.prompted += 1;
                tracing::info!(
                    event = "herdr_queue_poll_outcome",
                    member = %member.key,
                    msg_id = %claim.msg,
                    queue_kind = NudgeKind::Queue.as_str(),
                    outcome = "prompted",
                    "Herdr queue prompt accepted"
                );
            }
            Err(error) => {
                if error.code() == AtmErrorCode::HerdrUnavailable {
                    stats.breaker_open += 1;
                }
                let outcome = match error.code() {
                    AtmErrorCode::HerdrPromptFailed => {
                        release.requeue();
                        "dispatch_failed_requeued"
                    }
                    AtmErrorCode::HerdrAgentNotVisible => {
                        release.release_without_input();
                        "held_target_not_present"
                    }
                    AtmErrorCode::PostSendHerdrPromptFailed => {
                        release.release_without_input();
                        "blocked_before_input_released"
                    }
                    _ => {
                        release.release_without_input();
                        "dispatch_failed_released"
                    }
                };
                stats.released += 1;
                tracing::info!(
                    event = "herdr_queue_poll_outcome",
                    member = %member.key,
                    msg_id = %claim.msg,
                    queue_kind = NudgeKind::Queue.as_str(),
                    outcome,
                    error_code = ?error.code(),
                    "Herdr queue prompt failed"
                );
            }
        }
    }

    #[must_use]
    pub fn stats(&self) -> HerdrQueueWakeStats {
        self.last_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[cfg(test)]
    fn cursor_position(&self) -> usize {
        *self
            .cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn release_streak_for(&self, member: &MemberKey) -> u32 {
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(member)
            .copied()
            .unwrap_or_default()
    }

    fn save_stats(&self, stats: HerdrQueueWakeStats) {
        self.runtime_health
            .record_herdr_queue_tick(stats.last_tick_at);
        *self
            .last_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = stats;
    }

    fn reset_release_streak(&self, member: &MemberKey) {
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(member);
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
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    armed: bool,
}

impl ReleasePendingOnDrop {
    fn new(
        store: Arc<dyn PendingNudgeStore + Send + Sync>,
        member: MemberKey,
        claim: atm_core::boundary::NudgeClaim,
        release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    ) -> Self {
        Self {
            store,
            member,
            claim,
            release_streaks,
            armed: true,
        }
    }

    fn release_without_input(&mut self) {
        if self.armed {
            let should_requeue = {
                let mut streaks = self
                    .release_streaks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let streak = streaks.entry(self.member.clone()).or_default();
                if *streak >= HERDR_MAX_CONSECUTIVE_RELEASES {
                    streaks.remove(&self.member);
                    true
                } else {
                    *streak = streak.saturating_add(1);
                    false
                }
            };
            let result = if should_requeue {
                self.store.requeue_pending(&self.member, &self.claim)
            } else {
                self.store.release_pending(&self.member, &self.claim)
            };
            if let Err(error) = result {
                tracing::warn!(error = %error, member = %self.member, "failed to resolve Herdr queue claim");
            }
            self.armed = false;
        }
    }

    fn requeue(&mut self) {
        if self.armed {
            if let Err(error) = self.store.requeue_pending(&self.member, &self.claim) {
                tracing::warn!(error = %error, member = %self.member, "failed to requeue Herdr queue claim");
            }
            self.release_streaks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.member);
            self.armed = false;
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReleasePendingOnDrop {
    fn drop(&mut self) {
        self.release_without_input();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HERDR_MAX_CONSECUTIVE_RELEASES, HERDR_MAX_PROMPTS_PER_TICK, HERDR_POLL_INTERVAL_MS,
        HerdrQueueWakePump, runtime_state,
    };
    use atm_core::LocalServiceRuntime;
    use atm_core::api::RequestDeadline;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
        PostSendEmissionPath, RosterEntry, RosterHarness, RosterMemberKind,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::observability::NullObservability;
    use atm_core::protocol::RuntimeMemberState;
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
    use atm_core::types::{ModelName, TeamName};
    use atm_herdr::{
        AgentSnapshot, HerdrAgentStatus, HerdrListOutcome, HerdrProcessAdapter, HerdrPromptOutcome,
    };
    use atm_runtime_test_support::open_isolated_sqlite_boundary;
    use atm_storage::RosterSnapshot;
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::watch;

    struct FakeSelector {
        emitter: FakeEmitter,
    }

    struct FakeEmitter {
        process: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    }

    impl atm_core::boundary::sealed::Sealed for FakeSelector {}
    impl atm_core::boundary::sealed::Sealed for FakeEmitter {}

    impl MessageReceivedHookSelector for FakeSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            Some(&self.emitter)
        }
    }

    impl AsyncMessageReceivedHookEmitter for FakeEmitter {
        fn emit_received_message(
            &self,
            dispatch: BuiltInPostSendDispatch,
            deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
        {
            let process = Arc::clone(&self.process);
            Box::pin(async move {
                let target = match dispatch.target {
                    atm_core::boundary::PostSendBuiltInTarget::LocalSteer(
                        atm_core::boundary::LocalSteerTarget::Herdr(target),
                    ) => target,
                    _ => {
                        return Err(AtmError::new(
                            AtmErrorCode::InternalError,
                            "test dispatch was not Herdr",
                        ));
                    }
                };
                process
                    .prompt(
                        &dispatch.event.recipient,
                        target.session.as_ref(),
                        &target.rendered_nudge,
                        deadline,
                    )
                    .await
                    .map(|HerdrPromptOutcome::Accepted(_)| PostSendEmissionPath::LocalHerdr)
                    .map_err(Into::into)
            })
        }
    }

    fn herdr_member_with_session(team: &TeamName, agent: &str, session: &str) -> RosterEntry {
        RosterEntry {
            team_name: team.clone(),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::CodexCli,
            agent_type: atm_storage::AgentType::default(),
            model: ModelName::default(),
            recipient_pane_id: None,
            metadata_json: {
                let mut metadata = atm_core::delivery_channel::test_backend_type_metadata("herdr");
                metadata.insert("herdrSession".to_owned(), json!(session));
                metadata
            },
        }
    }

    fn herdr_member(team: &TeamName, agent: &str) -> RosterEntry {
        herdr_member_with_session(team, agent, "aq27-test")
    }

    fn queue_message(
        root: &std::path::Path,
        runtime: &LocalServiceRuntime,
        team: &TeamName,
        agent: &str,
    ) -> AtmMessageId {
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let recipient = format!("{agent}@{team}");
        write_mail_with_runtime(
            WriteRequest::new(
                home.clone(),
                home,
                "sender".parse().expect("sender"),
                &recipient,
                team.clone(),
                SendMessageSource::Inline("AQ2.7 test message".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("write request")
            .with_nudge_mode(NudgeMode::Deferred),
            &NullObservability,
            runtime,
        )
        .expect("queue write")
        .persisted_message_id()
    }

    fn build_test_pump_with_agents(
        agents: Vec<AgentSnapshot>,
    ) -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        Arc<HerdrQueueWakePump>,
        super::RuntimeHealth,
        atm_core::boundary::MemberKey,
    ) {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        let roster_agents: Vec<String> = if agents.is_empty() {
            vec!["aq27-agent".to_owned()]
        } else {
            agents
                .iter()
                .filter_map(|snapshot| snapshot.name.clone())
                .collect()
        };
        let members = roster_agents
            .iter()
            .map(|agent| herdr_member(&team, agent))
            .collect();
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("roster");
        for agent in &roster_agents {
            queue_message(root.path(), &assembly.service_runtime, &team, agent);
        }
        let key =
            atm_core::boundary::MemberKey::new(team, roster_agents[0].parse().expect("agent"));
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Ok(HerdrListOutcome { agents }));
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let health = super::RuntimeHealth::default();
        let pump = Arc::new(HerdrQueueWakePump::new(
            assembly.service_runtime.clone(),
            selector,
            health.clone(),
            process,
        ));
        (root, assembly.service_runtime, fake, pump, health, key)
    }

    fn build_test_pump() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        Arc<HerdrQueueWakePump>,
        super::RuntimeHealth,
        atm_core::boundary::MemberKey,
    ) {
        build_test_pump_with_agents(vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }])
    }

    fn build_test_pump_with_two_sessions() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        Arc<HerdrQueueWakePump>,
        atm_core::boundary::MemberKey,
    ) {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        let members = vec![
            herdr_member_with_session(&team, "aq27-agent", "aq27-session-a"),
            herdr_member_with_session(&team, "aq27-agent-b", "aq27-session-b"),
        ];
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("roster");
        queue_message(root.path(), &assembly.service_runtime, &team, "aq27-agent");
        queue_message(
            root.path(),
            &assembly.service_runtime,
            &team,
            "aq27-agent-b",
        );
        let agents = vec![
            AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
            AgentSnapshot {
                name: Some("aq27-agent-b".to_owned()),
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
        ];
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        for _ in 0..2 {
            fake.queue_list_result(Ok(HerdrListOutcome {
                agents: agents.clone(),
            }));
        }
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let pump = Arc::new(HerdrQueueWakePump::new(
            assembly.service_runtime.clone(),
            selector,
            super::RuntimeHealth::default(),
            process,
        ));
        let key = atm_core::boundary::MemberKey::new(team, "aq27-agent".parse().expect("agent"));
        (root, assembly.service_runtime, fake, pump, key)
    }

    async fn cancel_inflight_prompt() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        atm_core::boundary::MemberKey,
    ) {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        let prompt_gate = fake.block_next_prompt();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let sender_clone = shutdown_tx.clone();
        let task = pump.start(shutdown_rx);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if fake
                    .calls()
                    .iter()
                    .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the fake prompt is in flight before shutdown");
        shutdown_tx.send(()).expect("shutdown notification");
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("pump joins after shutdown notification")
            .expect("poll task join");
        drop(prompt_gate);
        drop(sender_clone);
        (root, runtime, fake, key)
    }

    async fn test_pump() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        atm_core::boundary::MemberKey,
    ) {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1, "one queued message prompted");
        (root, runtime, fake, key)
    }

    #[test]
    fn poll_contract_uses_fixed_cadence_and_cap() {
        assert_eq!(HERDR_POLL_INTERVAL_MS, 5_000);
        assert_eq!(HERDR_MAX_PROMPTS_PER_TICK, 16);
    }

    #[tokio::test]
    async fn ac01_fifo_per_member_via_claim() {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
        queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1, "FIFO claims one message per tick");
        assert!(fake.calls().iter().any(|call| matches!(
            call,
            atm_herdr::testing::FakeHerdrCall::Prompt {
                agent,
                session: Some(session),
                text,
                ..
            } if agent == "aq27-agent"
                && session.as_str() == "aq27-test"
                && text.contains("atm read --message-id")
                && text.contains("AQ2.7 test message")
                && !text.contains("<when ")
        )));
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
    }

    #[tokio::test]
    async fn ac02_burst_cap_is_sixteen_successful_prompts() {
        let agents: Vec<AgentSnapshot> = (0..17)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            })
            .collect();
        let (_root, runtime, fake, pump, _health, key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, HERDR_MAX_PROMPTS_PER_TICK);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            HERDR_MAX_PROMPTS_PER_TICK
        );
        assert_eq!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .len(),
            1
        );
        let remaining = atm_core::boundary::MemberKey::new(
            key.team().clone(),
            "aq27-agent-16".parse().expect("agent"),
        );
        let store = runtime.pending_nudge_store().expect("pending store");
        let claim = store
            .claim_next_pending(&remaining)
            .expect("remaining claim")
            .expect("cap leaves remaining marker");
        assert_eq!(claim.attempt, 0, "the capped member was never claimed");
        store
            .release_pending(&remaining, &claim)
            .expect("restore cap assertion claim");
    }

    #[tokio::test]
    async fn ac03_session_grouping_is_part_of_the_poll_contract() {
        let (_root, _runtime, fake, pump, _key) = build_test_pump_with_two_sessions();
        pump.tick_once().await;
        let list_sessions: Vec<_> = fake
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::List { session } => session,
                _ => None,
            })
            .collect();
        assert_eq!(list_sessions.len(), 2);
        assert!(
            list_sessions
                .iter()
                .any(|session| session.as_str() == "aq27-session-a")
        );
        assert!(
            list_sessions
                .iter()
                .any(|session| session.as_str() == "aq27-session-b")
        );
    }

    #[tokio::test]
    async fn ac04_shutdown_send_stops_pump_before_drain_completes() {
        let (_root, runtime, fake, key) = cancel_inflight_prompt().await;
        let calls = fake.calls();
        assert_eq!(
            calls
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            1,
            "shutdown leaves no second prompt"
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
    }

    #[tokio::test]
    async fn ac05_fake_adapter_breaker_error_does_not_prompt() {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![herdr_member(&team, "aq27-agent")],
                refreshed_at: None,
            })
            .expect("roster");
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable));
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let pump = HerdrQueueWakePump::new(
            assembly.service_runtime,
            selector,
            super::RuntimeHealth::default(),
            process,
        );
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 0);
        assert!(pump.stats().breaker_open > 0);
    }

    #[tokio::test]
    async fn ac06_blocked_race_releases_pending_with_zero_injected_bytes() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentBlocked));

        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 0, "blocked prompt injected no bytes");
        assert_eq!(pump.stats().released, 1);
        assert_eq!(pump.release_streak_for(&key), 1);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            1,
            "the post-claim prompt was attempted exactly once"
        );

        let store = runtime.pending_nudge_store().expect("pending store");
        let claim = store
            .claim_next_pending(&key)
            .expect("claim released message")
            .expect("blocked claim remains pending");
        assert_eq!(claim.attempt, 0, "blocked input consumes no retry debt");
        store.release_pending(&key, &claim).expect("restore claim");
    }

    #[tokio::test]
    async fn ac06_not_found_family_releases_without_input() {
        for error in [
            atm_herdr::HerdrError::AgentNotFound,
            atm_herdr::HerdrError::AgentTargetAmbiguous,
            atm_herdr::HerdrError::AgentNotReady,
        ] {
            let (_root, runtime, fake, pump, _health, key) = build_test_pump();
            fake.queue_prompt_result(Err(error));

            pump.tick_once().await;

            assert_eq!(pump.stats().prompted, 0);
            assert_eq!(pump.stats().released, 1);
            assert_eq!(pump.release_streak_for(&key), 1);
            let prompt_calls = fake
                .calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count();
            assert_eq!(
                prompt_calls, 1,
                "each lifecycle error reaches one prompt call"
            );

            let store = runtime.pending_nudge_store().expect("pending store");
            let claim = store
                .claim_next_pending(&key)
                .expect("claim released message")
                .expect("not-found-family claim remains pending");
            assert_eq!(claim.attempt, 0, "not-present input consumes no retry debt");
            store.release_pending(&key, &claim).expect("restore claim");
        }
    }

    #[tokio::test]
    async fn ac06_consecutive_release_bound_requeues_after_ten() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        for _ in 0..HERDR_MAX_CONSECUTIVE_RELEASES {
            fake.queue_list_result(Ok(HerdrListOutcome {
                agents: vec![AgentSnapshot {
                    name: Some(key.agent().to_string()),
                    status: HerdrAgentStatus::Idle,
                    workspace_id: None,
                }],
            }));
        }
        for _ in 0..=HERDR_MAX_CONSECUTIVE_RELEASES {
            fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentBlocked));
        }

        let store = runtime.pending_nudge_store().expect("pending store");
        for release_number in 1..=HERDR_MAX_CONSECUTIVE_RELEASES + 1 {
            pump.tick_once().await;
            let claim = store
                .claim_next_pending(&key)
                .expect("claim resolved message")
                .expect("resolved claim remains pending");
            let expected_attempt = if release_number > HERDR_MAX_CONSECUTIVE_RELEASES {
                1
            } else {
                0
            };
            assert_eq!(claim.attempt, expected_attempt, "release {release_number}");
            assert_eq!(
                pump.release_streak_for(&key),
                if release_number > HERDR_MAX_CONSECUTIVE_RELEASES {
                    0
                } else {
                    release_number
                },
                "release counter at outcome {release_number}"
            );
            store.release_pending(&key, &claim).expect("restore claim");
        }
    }

    #[tokio::test]
    async fn ac07_absent_members_are_not_presented_as_idle() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump_with_agents(Vec::new());
        pump.tick_once().await;
        assert_eq!(pump.stats().not_present, 1);
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
    }

    #[tokio::test]
    async fn ac08_dispatch_selector_is_used_by_tick_once() {
        let (_root, runtime, fake, key) = test_pump().await;
        assert!(
            fake.calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .is_empty()
        );
        assert_eq!(key.agent().as_str(), "aq27-agent");
    }

    #[tokio::test]
    async fn ac09_fake_adapter_never_needs_wait_for_queue_wake() {
        let (_root, _runtime, fake, _key) = test_pump().await;
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Wait { .. }))
        );
    }

    #[tokio::test]
    async fn ac10_herdr_statuses_update_runtime_health_states() {
        let agents = vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            status: HerdrAgentStatus::Working,
            workspace_id: None,
        }];
        let (_root, _runtime, _fake, pump, health, key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        let member = health
            .snapshot()
            .members
            .into_iter()
            .find(|member| member.member.as_str() == key.agent().as_str())
            .expect("Herdr member health observation");
        assert_eq!(member.state, RuntimeMemberState::Active);
        assert_eq!(
            member.state_changed_by,
            Some(atm_core::protocol::RuntimeObservationSource::HerdrPoll)
        );
    }

    #[tokio::test]
    async fn ac11_claim_drop_guard_releases_marker_on_cancellation() {
        let (_root, runtime, _fake, key) = cancel_inflight_prompt().await;
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
        assert_eq!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .claim_next_pending(&key)
                .expect("claim after cancellation")
                .expect("released claim")
                .attempt,
            0
        );
    }

    #[tokio::test]
    async fn ac12_cursor_contract_is_rotation_not_reordering() {
        let agents: Vec<AgentSnapshot> = (0..20)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            })
            .collect();
        let (root, runtime, fake, pump, _health, _key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        assert_eq!(pump.cursor_position(), HERDR_MAX_PROMPTS_PER_TICK);

        let changed_agents: Vec<AgentSnapshot> = (0..22)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            })
            .collect();
        let team: TeamName = "aq27-team".parse().expect("team");
        let members = (0..22)
            .map(|index| {
                let agent = if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                };
                herdr_member(&team, &agent)
            })
            .collect();
        runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("changed roster");
        runtime.clear_roster_cache();
        queue_message(root.path(), &runtime, &team, "aq27-agent");
        queue_message(root.path(), &runtime, &team, "aq27-agent-20");
        queue_message(root.path(), &runtime, &team, "aq27-agent-21");
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: changed_agents,
        }));
        pump.tick_once().await;
        let prompted: Vec<String> = fake
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Prompt { agent, .. } => Some(agent),
                _ => None,
            })
            .collect();
        assert_eq!(prompted.len(), 23);
        for index in 0..22 {
            let agent = if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            };
            let expected = usize::from(index == 0) + 1;
            assert_eq!(
                prompted
                    .iter()
                    .filter(|prompted| **prompted == agent)
                    .count(),
                expected,
                "prompt count for {agent}"
            );
        }
        assert_eq!(pump.stats().pending_members, 7);
        assert_eq!(pump.stats().prompted, 7);
        assert_eq!(pump.cursor_position(), 2);
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
