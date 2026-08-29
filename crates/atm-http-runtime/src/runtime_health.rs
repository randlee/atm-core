//! In-memory lifecycle and heartbeat projection for the replacement runtime.
//!
//! This is intentionally small: it retains no listener, storage, or harness
//! implementation. Listener lifecycle drives readiness; authenticated local
//! heartbeats enrich the existing doctor/status payload and make a best-effort
//! idle-transition notification. Durable pending-nudge state and the recovery
//! sweep remain the correctness backstop.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use atm_core::boundary::MemberKey;
use atm_core::protocol::{
    HeartbeatActivity, RuntimeLivenessState, RuntimeMemberObservation, RuntimeMemberState,
    RuntimeObservationSource, RuntimeReadinessState, RuntimeStatusCounts, RuntimeStatusSnapshot,
    TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
};
use atm_core::types::{IsoTimestamp, SessionId};
use tokio::sync::watch;

/// The bounded runtime-member projection is observability, not durable state.
pub const MAX_RUNTIME_STATUS_MEMBERS: usize = 4096;

#[derive(Clone)]
pub struct RuntimeHealth {
    inner: Arc<Mutex<RuntimeHealthState>>,
    /// Broadcasts every `record_herdr_queue_tick` observation so callers can
    /// await the next Herdr queue-wake pump tick directly instead of polling
    /// `snapshot()` on a fixed cadence. Sending never requires a live
    /// subscriber: production runs with none, and `watch::Sender::send_replace`
    /// only reports its previous value, never an error.
    herdr_queue_tick: watch::Sender<Option<IsoTimestamp>>,
}

impl Default for RuntimeHealth {
    fn default() -> Self {
        let (herdr_queue_tick, _receiver) = watch::channel(None);
        Self {
            inner: Arc::default(),
            herdr_queue_tick,
        }
    }
}

/// Best-effort notification of a genuine member lifecycle transition.
///
/// The callback is invoked after the health mutex is released. Implementations
/// must keep storage and process work off the heartbeat task.
pub trait MemberStateTransitionSink: atm_core::boundary::sealed::Sealed + Send + Sync {
    fn on_transition(
        &self,
        member: &atm_core::boundary::MemberKey,
        from: RuntimeMemberState,
        to: RuntimeMemberState,
    );
}

#[derive(Default)]
struct RuntimeHealthState {
    lifecycle: Lifecycle,
    detail: Option<String>,
    owner_pid: Option<u32>,
    members: HashMap<MemberKey, MemberRecord>,
    graft_queue_handoff_failures_total: u64,
    graft_queue_marker_clear_failures_total: u64,
    queue_marker_set_failures_total: u64,
    herdr_queue_last_tick_at: Option<IsoTimestamp>,
    queue_messages_drained_total: u64,
    queue_drain_failures_total: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Lifecycle {
    #[default]
    NotReady,
    Ready,
    Draining,
    Stopped,
}

#[derive(Clone)]
struct MemberRecord {
    pid: Option<u32>,
    session_id: Option<SessionId>,
    state: RuntimeMemberState,
    last_active_at: Option<IsoTimestamp>,
    state_changed_at: Option<IsoTimestamp>,
    session_changed_at: Option<IsoTimestamp>,
    state_source: RuntimeObservationSource,
}

impl RuntimeHealth {
    #[must_use]
    pub fn with_owner(owner_pid: u32) -> Self {
        let health = Self::default();
        health.set_owner(owner_pid);
        health
    }

    pub fn set_owner(&self, owner_pid: u32) {
        let mut state = self.lock();
        state.owner_pid = Some(owner_pid);
        state.lifecycle = Lifecycle::NotReady;
        state.detail = Some("validating replacement runtime configuration".to_owned());
    }

    pub fn mark_ready(&self) {
        let mut state = self.lock();
        state.lifecycle = Lifecycle::Ready;
        state.detail = None;
    }

    pub fn mark_not_ready(&self, detail: impl Into<String>) {
        let mut state = self.lock();
        state.lifecycle = Lifecycle::NotReady;
        state.detail = Some(detail.into());
    }

    pub fn begin_drain(&self) {
        let mut state = self.lock();
        state.lifecycle = Lifecycle::Draining;
        state.detail = Some("replacement runtime is draining".to_owned());
    }

    pub fn mark_stopped(&self) {
        let mut state = self.lock();
        state.lifecycle = Lifecycle::Stopped;
        state.detail = Some("replacement runtime is stopped".to_owned());
    }

    /// Records one failed queue-kind graft handoff. The counter is
    /// cumulative for the daemon lifetime and deliberately does not own any
    /// retry state; the pending-nudge store and AQ3 own that policy.
    pub fn record_graft_queue_handoff_failure(&self) {
        let mut state = self.lock();
        state.graft_queue_handoff_failures_total =
            state.graft_queue_handoff_failures_total.saturating_add(1);
    }

    /// Records a failed pending-marker clear after delivery succeeded.
    pub fn record_graft_queue_marker_clear_failure(&self) {
        let mut state = self.lock();
        state.graft_queue_marker_clear_failures_total = state
            .graft_queue_marker_clear_failures_total
            .saturating_add(1);
    }

    pub fn record_queue_marker_set_failure(&self) {
        let mut state = self.lock();
        state.queue_marker_set_failures_total =
            state.queue_marker_set_failures_total.saturating_add(1);
    }

    pub fn record_herdr_queue_tick(&self, observed_at: Option<IsoTimestamp>) {
        self.lock().herdr_queue_last_tick_at = observed_at;
        self.herdr_queue_tick.send_replace(observed_at);
    }

    /// Subscribes to Herdr queue-wake pump tick observations.
    ///
    /// The returned receiver's `changed()` future resolves the next time
    /// [`RuntimeHealth::record_herdr_queue_tick`] runs anywhere this
    /// `RuntimeHealth` (or one of its clones) is held, letting callers await
    /// the pump's own completion signal instead of polling `snapshot()` on a
    /// fixed interval.
    ///
    /// Only test harnesses subscribe today; production code observes queue
    /// activity through [`RuntimeHealth::snapshot`]. Gated to keep this
    /// accessor out of the shipped daemon dependency, matching
    /// `DirectPeerTcpConfig::ephemeral_for_test`'s convention.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn subscribe_herdr_queue_tick(&self) -> watch::Receiver<Option<IsoTimestamp>> {
        self.herdr_queue_tick.subscribe()
    }

    pub fn record_queue_message_drained(&self) {
        let mut state = self.lock();
        state.queue_messages_drained_total = state.queue_messages_drained_total.saturating_add(1);
    }

    pub fn record_queue_drain_failure(&self) {
        let mut state = self.lock();
        state.queue_drain_failures_total = state.queue_drain_failures_total.saturating_add(1);
    }

    /// Record an already-authorized local heartbeat.
    ///
    /// The brief mutex protects only in-memory status fields; storage and
    /// process work remain outside this critical section.
    pub fn record_heartbeat(
        &self,
        request: &TeamMemberHeartbeatRequest,
    ) -> (TeamMemberHeartbeatResponse, Option<RuntimeMemberState>) {
        let mut state = self.lock();
        let key = MemberKey {
            team: request.team.clone(),
            agent: request.member.clone(),
        };
        if !state.members.contains_key(&key) && state.members.len() == MAX_RUNTIME_STATUS_MEMBERS {
            // Observability must never become an unbounded in-process cache.
            // Prefer evicting an inactive observation; all entries are
            // non-authoritative and can be restored by the next heartbeat.
            if let Some(evicted) = state
                .members
                .iter()
                .min_by_key(|(_, record)| record.last_active_at.or(record.state_changed_at))
                .map(|(key, _)| key.clone())
            {
                state.members.remove(&evicted);
            }
        }
        let record = state.members.entry(key).or_insert(MemberRecord {
            pid: None,
            session_id: None,
            state: RuntimeMemberState::Unknown,
            last_active_at: None,
            state_changed_at: None,
            session_changed_at: None,
            state_source: RuntimeObservationSource::Heartbeat,
        });
        let next_state = match request.activity {
            HeartbeatActivity::ActiveToolUse => RuntimeMemberState::Active,
            HeartbeatActivity::Idle => RuntimeMemberState::Idle,
            HeartbeatActivity::SessionEnded => RuntimeMemberState::Offline,
        };
        let previous_state = record.state;
        let transitioned_to_idle = (previous_state != RuntimeMemberState::Idle
            && next_state == RuntimeMemberState::Idle)
            .then_some(previous_state);
        if previous_state != next_state {
            record.state = next_state;
            record.state_changed_at = Some(request.observed_at);
        }
        record.state_source = RuntimeObservationSource::Heartbeat;
        if next_state == RuntimeMemberState::Active {
            record.last_active_at = Some(request.observed_at);
        }
        let previous_pid = record.pid;
        record.pid = Some(request.pid);
        if request.session_id.is_some() && record.session_id != request.session_id {
            record.session_id = request.session_id.clone();
            record.session_changed_at = Some(request.observed_at);
        }
        (
            TeamMemberHeartbeatResponse {
                team: request.team.clone(),
                member: request.member.clone(),
                pid: request.pid,
                pid_changed: previous_pid.is_some_and(|pid| pid != request.pid),
                state: next_state,
                last_active_at: record.last_active_at,
                session_id: record.session_id.clone(),
            },
            transitioned_to_idle,
        )
    }

    /// Records a Herdr poll observation without changing heartbeat-owned
    /// process or session identity fields.
    pub fn record_observed_state(
        &self,
        member: &MemberKey,
        next_state: RuntimeMemberState,
        source: RuntimeObservationSource,
    ) {
        let mut state = self.lock();
        if !state.members.contains_key(member)
            && state.members.len() == MAX_RUNTIME_STATUS_MEMBERS
            && let Some(evicted) = state
                .members
                .iter()
                .min_by_key(|(_, record)| record.last_active_at.or(record.state_changed_at))
                .map(|(key, _)| key.clone())
        {
            state.members.remove(&evicted);
        }
        let record = state.members.entry(member.clone()).or_insert(MemberRecord {
            pid: None,
            session_id: None,
            state: RuntimeMemberState::Unknown,
            last_active_at: None,
            state_changed_at: None,
            session_changed_at: None,
            state_source: source,
        });
        let observed_at = IsoTimestamp::now();
        if record.state != next_state {
            record.state = next_state;
            record.state_changed_at = Some(observed_at);
        }
        record.state_source = source;
        if next_state == RuntimeMemberState::Active {
            record.last_active_at = Some(observed_at);
        }
    }

    pub fn record_herdr_poll_state(&self, member: &MemberKey, next_state: RuntimeMemberState) {
        self.record_observed_state(member, next_state, RuntimeObservationSource::HerdrPoll);
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeStatusSnapshot {
        let state = self.lock();
        let mut members: Vec<_> = state
            .members
            .iter()
            .map(|(key, record)| RuntimeMemberObservation {
                team: key.team.clone(),
                member: key.agent.clone(),
                state: record.state,
                session_id: record.session_id.clone(),
                pid: record.pid,
                last_active_at: record.last_active_at,
                state_changed_by: Some(record.state_source),
                state_changed_at: record.state_changed_at,
                session_changed_by: record
                    .session_changed_at
                    .map(|_| RuntimeObservationSource::Heartbeat),
                session_changed_at: record.session_changed_at,
            })
            .collect();
        members.sort_by(|left, right| {
            left.team
                .as_str()
                .cmp(right.team.as_str())
                .then_with(|| left.member.as_str().cmp(right.member.as_str()))
        });
        let mut counts = RuntimeStatusCounts::default();
        for member in &members {
            match member.state {
                RuntimeMemberState::Active => counts.active_members += 1,
                RuntimeMemberState::Idle => counts.idle_members += 1,
                RuntimeMemberState::Offline => counts.offline_members += 1,
                RuntimeMemberState::Unknown | RuntimeMemberState::IdentityConflict => {
                    counts.unknown_members += 1;
                }
            }
        }
        let (liveness, readiness) = match state.lifecycle {
            Lifecycle::Ready => (RuntimeLivenessState::Running, RuntimeReadinessState::Ready),
            Lifecycle::NotReady | Lifecycle::Draining => (
                RuntimeLivenessState::Running,
                RuntimeReadinessState::Unavailable,
            ),
            Lifecycle::Stopped => (
                RuntimeLivenessState::Unavailable,
                RuntimeReadinessState::Unavailable,
            ),
        };
        RuntimeStatusSnapshot {
            liveness,
            readiness,
            detail: state.detail.clone(),
            singleton_owner_pid: state.owner_pid,
            degraded_ingest: false,
            member_counts: counts,
            members,
            graft_queue_handoff_failures_total: state.graft_queue_handoff_failures_total,
            graft_queue_marker_clear_failures_total: state.graft_queue_marker_clear_failures_total,
            bare_cli_queue_full_drops_total: 0,
            queue_marker_set_failures_total: state.queue_marker_set_failures_total,
            herdr_queue_last_tick_at: state.herdr_queue_last_tick_at,
            queue_messages_drained_total: state.queue_messages_drained_total,
            queue_drain_failures_total: state.queue_drain_failures_total,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RuntimeHealthState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeHealth;
    use atm_core::boundary::MemberKey;
    use atm_core::protocol::{
        HeartbeatActivity, RuntimeLivenessState, RuntimeMemberState, RuntimeObservationSource,
        RuntimeReadinessState, TeamMemberHeartbeatRequest,
    };
    use atm_core::types::{AgentName, IsoTimestamp, SessionId, TeamName};

    fn heartbeat(pid: u32, activity: HeartbeatActivity) -> TeamMemberHeartbeatRequest {
        TeamMemberHeartbeatRequest {
            team: TeamName::from_validated("runtime-team"),
            member: AgentName::from_validated("runtime-agent"),
            pid,
            observed_at: IsoTimestamp::now(),
            activity,
            session_id: None,
        }
    }

    #[test]
    fn readiness_tracks_listener_lifecycle_not_member_activity() {
        let health = RuntimeHealth::with_owner(42);
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );
        health.mark_ready();
        let (first, transition) =
            health.record_heartbeat(&heartbeat(10, HeartbeatActivity::ActiveToolUse));
        assert!(!first.pid_changed);
        assert!(transition.is_none());
        let (second, transition) =
            health.record_heartbeat(&heartbeat(11, HeartbeatActivity::SessionEnded));
        assert!(second.pid_changed);
        assert_eq!(second.state, RuntimeMemberState::Offline);
        assert!(transition.is_none());
        assert_eq!(health.snapshot().readiness, RuntimeReadinessState::Ready);
        health.begin_drain();
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );
        health.mark_stopped();
        assert_eq!(
            health.snapshot().liveness,
            RuntimeLivenessState::Unavailable
        );
    }

    #[test]
    fn queue_graft_failures_are_cumulative_health_observations() {
        let health = RuntimeHealth::default();
        health.record_graft_queue_handoff_failure();
        health.record_graft_queue_handoff_failure();
        health.record_graft_queue_marker_clear_failure();
        assert_eq!(health.snapshot().graft_queue_handoff_failures_total, 2);
        assert_eq!(health.snapshot().graft_queue_marker_clear_failures_total, 1);
    }

    #[test]
    fn herdr_poll_preserves_heartbeat_identity_and_tags_provenance() {
        let health = RuntimeHealth::default();
        let request = TeamMemberHeartbeatRequest {
            team: TeamName::from_validated("runtime-team"),
            member: AgentName::from_validated("runtime-agent"),
            pid: 42,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: Some(SessionId::new("session-a").expect("session")),
        };
        health.record_heartbeat(&request);
        health.record_observed_state(
            &MemberKey::new(request.team.clone(), request.member.clone()),
            RuntimeMemberState::Idle,
            RuntimeObservationSource::HerdrPoll,
        );
        let member = &health.snapshot().members[0];
        assert_eq!(member.pid, Some(42));
        assert_eq!(member.session_id, request.session_id);
        assert_eq!(member.state, RuntimeMemberState::Idle);
        assert_eq!(
            member.state_changed_by,
            Some(RuntimeObservationSource::HerdrPoll)
        );
        assert_eq!(
            member.session_changed_by,
            Some(RuntimeObservationSource::Heartbeat)
        );
    }

    #[test]
    fn herdr_queue_tick_is_visible_to_runtime_status() {
        let health = RuntimeHealth::default();
        let tick = IsoTimestamp::now();
        health.record_herdr_queue_tick(Some(tick));
        assert_eq!(health.snapshot().herdr_queue_last_tick_at, Some(tick));
    }

    #[tokio::test]
    async fn herdr_queue_tick_subscribers_observe_the_recorded_value_without_polling() {
        let health = RuntimeHealth::default();
        let mut subscriber = health.subscribe_herdr_queue_tick();
        assert_eq!(*subscriber.borrow(), None, "no tick has been recorded yet");

        let tick = IsoTimestamp::now();
        health.record_herdr_queue_tick(Some(tick));

        subscriber
            .changed()
            .await
            .expect("the sender stays alive for the health handle's lifetime");
        assert_eq!(*subscriber.borrow_and_update(), Some(tick));
    }

    #[test]
    fn heartbeat_reports_only_genuine_transition_into_idle() {
        let health = RuntimeHealth::default();
        let (response, transition) =
            health.record_heartbeat(&heartbeat(10, HeartbeatActivity::ActiveToolUse));
        assert_eq!(response.state, RuntimeMemberState::Active);
        assert_eq!(transition, None);

        let (response, transition) =
            health.record_heartbeat(&heartbeat(10, HeartbeatActivity::Idle));
        assert_eq!(response.state, RuntimeMemberState::Idle);
        assert_eq!(transition, Some(RuntimeMemberState::Active));

        let (_, transition) = health.record_heartbeat(&heartbeat(10, HeartbeatActivity::Idle));
        assert_eq!(transition, None);
    }
}
