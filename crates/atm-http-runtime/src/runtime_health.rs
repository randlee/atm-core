//! Process-health counters and canonical-roster projection for the replacement runtime.
//!
//! This is intentionally small: it retains no listener, storage, or harness
//! implementation. Listener lifecycle drives readiness; authenticated local
//! heartbeats enrich the existing doctor/status payload and make a best-effort
//! status payloads project caller-supplied canonical roster observations;
//! this module never owns member state. Durable pending-nudge state and the
//! recovery sweep remain the correctness backstop.

use std::sync::{Arc, Mutex};

use atm_core::boundary::IdleOpportunity;
use atm_core::protocol::{
    RosterRuntimeObservation, RuntimeLivenessState, RuntimeMemberObservation, RuntimeMemberState,
    RuntimeReadinessState, RuntimeStatusCounts, RuntimeStatusSnapshot,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use tokio::sync::watch;

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
/// The callback is invoked after the canonical roster mutation lock is
/// released. Implementations must keep storage and process work off the
/// heartbeat task.
pub trait MemberStateTransitionSink: atm_core::boundary::sealed::Sealed + Send + Sync {
    fn on_transition(
        &self,
        member: &atm_core::boundary::MemberKey,
        from: RuntimeMemberState,
        to: RuntimeMemberState,
    );
}

/// Receives an accepted canonical roster revision that is eligible for
/// attention scheduling. The source (heartbeat or Herdr poll) is deliberately
/// absent: the receiver gets only the post-commit member identity and revision
/// it must revalidate before dispatch.
pub trait IdleOpportunitySink: atm_core::boundary::sealed::Sealed + Send + Sync {
    fn on_idle_opportunity(&self, opportunity: IdleOpportunity);
}

#[derive(Default)]
struct RuntimeHealthState {
    lifecycle: Lifecycle,
    detail: Option<String>,
    owner_pid: Option<u32>,
    graft_queue_handoff_failures_total: u64,
    graft_queue_marker_clear_failures_total: u64,
    queue_marker_set_failures_total: u64,
    herdr_queue_last_tick_at: Option<IsoTimestamp>,
    queue_messages_drained_total: u64,
    queue_drain_failures_total: u64,
    blocking_core_bridge_stalls_total: u64,
    write_source_preflight_stalls_total: u64,
    detached_received_hook_warnings_total: u64,
    idle_opportunity_dispatches_total: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Lifecycle {
    #[default]
    NotReady,
    Ready,
    Draining,
    Stopped,
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

    #[must_use]
    pub(crate) fn is_draining(&self) -> bool {
        self.lock().lifecycle == Lifecycle::Draining
    }

    pub(crate) fn record_idle_opportunity_dispatch(&self) {
        let mut state = self.lock();
        state.idle_opportunity_dispatches_total =
            state.idle_opportunity_dispatches_total.saturating_add(1);
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

    /// Records one receiver-hook warning raised by a peer write whose hook
    /// runs after the response was already returned.
    ///
    /// Peer ingress answers once the message is durably persisted, so the
    /// hook cannot report through the response envelope. The warning is
    /// logged with the originating request id; this counter is the numeric
    /// signal that such warnings occurred rather than being discarded.
    pub fn record_detached_received_hook_warning(&self) {
        let mut state = self.lock();
        state.detached_received_hook_warnings_total = state
            .detached_received_hook_warnings_total
            .saturating_add(1);
    }

    /// Returns how many detached receiver-hook warnings have been observed.
    ///
    /// Production observes these warnings in the log record written next to
    /// this counter; only harnesses read the count directly, so the accessor
    /// follows `subscribe_herdr_queue_tick`'s gating convention.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn detached_received_hook_warnings_total(&self) -> u64 {
        self.lock().detached_received_hook_warnings_total
    }

    /// Records one blocking-core-bridge job that ran longer than the
    /// remaining request budget it was dispatched with.
    ///
    /// The bridge does not cancel the outrunning `spawn_blocking` job (doing
    /// so would abandon a durable storage write mid-flight); this counter is
    /// the observable signal that a job outlived its budget so `atm doctor`
    /// can surface a stalled blocking bridge instead of it being silently
    /// invisible.
    pub fn record_blocking_core_bridge_stall(&self) {
        let mut state = self.lock();
        state.blocking_core_bridge_stalls_total =
            state.blocking_core_bridge_stalls_total.saturating_add(1);
    }

    /// Records a caller-owned file or template preflight that outlived its
    /// request budget while retaining one bounded blocking permit.
    pub fn record_write_source_preflight_stall(&self) {
        let mut state = self.lock();
        state.write_source_preflight_stalls_total =
            state.write_source_preflight_stalls_total.saturating_add(1);
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeStatusSnapshot {
        self.snapshot_with_member_observations(&TeamName::from_validated("runtime"), &[])
    }

    /// Projects canonical master-roster state into the runtime status DTO.
    /// This object owns process health counters only; it never retains or
    /// mutates member lifecycle state.
    #[must_use]
    pub fn snapshot_with_member_observations(
        &self,
        team: &TeamName,
        observations: &[(AgentName, RosterRuntimeObservation)],
    ) -> RuntimeStatusSnapshot {
        let state = self.lock();
        let members = observations
            .iter()
            .map(|(agent, observation)| project_member(team, agent, observation))
            .collect::<Vec<_>>();
        let mut counts = RuntimeStatusCounts::default();
        for member in &members {
            match member.state {
                RuntimeMemberState::Active => counts.active_members += 1,
                RuntimeMemberState::Idle => counts.idle_members += 1,
                RuntimeMemberState::Offline => counts.offline_members += 1,
                RuntimeMemberState::Unknown
                | RuntimeMemberState::IdentityConflict
                | RuntimeMemberState::Blocked => {
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
            blocking_core_bridge_stalls_total: state.blocking_core_bridge_stalls_total,
            write_source_preflight_stalls_total: state.write_source_preflight_stalls_total,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RuntimeHealthState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn project_member(
    team: &TeamName,
    agent: &AgentName,
    record: &RosterRuntimeObservation,
) -> RuntimeMemberObservation {
    RuntimeMemberObservation {
        team: team.clone(),
        member: agent.clone(),
        state: record.state,
        revision: record.revision,
        availability: record.availability,
        last_observation_attempt_by: record.last_observation_attempt_by,
        last_observation_attempt_at: record.last_observation_attempt_at,
        last_observed_by: record.last_observed_by,
        last_observed_at: record.last_observed_at,
        session_id: record.session_id.clone(),
        pid: record.pid,
        last_active_at: record.last_active_at,
        state_changed_by: record.state_changed_by,
        state_changed_at: record.state_changed_at,
        session_changed_by: record.session_changed_by,
        session_changed_at: record.session_changed_at,
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeHealth;
    use atm_core::protocol::RosterRuntimeObservation;
    use atm_core::protocol::{
        RuntimeLivenessState, RuntimeMemberState, RuntimeObservationSource, RuntimeReadinessState,
    };
    use atm_core::types::{AgentName, IsoTimestamp, TeamName};

    #[test]
    fn readiness_tracks_listener_lifecycle() {
        let health = RuntimeHealth::with_owner(42);
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );
        health.mark_ready();
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
    fn canonical_roster_observations_are_projected_without_becoming_health_state() {
        let health = RuntimeHealth::default();
        let team = TeamName::from_validated("runtime-team");
        let agent = AgentName::from_validated("runtime-agent");
        let changed_at = IsoTimestamp::now();
        let observation = RosterRuntimeObservation {
            state: RuntimeMemberState::Idle,
            pid: Some(42),
            state_changed_by: Some(RuntimeObservationSource::HerdrPoll),
            state_changed_at: Some(changed_at),
            ..RosterRuntimeObservation::default()
        };

        let snapshot =
            health.snapshot_with_member_observations(&team, &[(agent.clone(), observation)]);
        let member = &snapshot.members[0];
        assert_eq!(member.team, team);
        assert_eq!(member.member, agent);
        assert_eq!(member.pid, Some(42));
        assert_eq!(member.state, RuntimeMemberState::Idle);
        assert_eq!(
            member.state_changed_by,
            Some(RuntimeObservationSource::HerdrPoll)
        );
        assert!(health.snapshot().members.is_empty());
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
}
