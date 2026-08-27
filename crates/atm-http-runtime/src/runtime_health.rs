//! In-memory lifecycle and heartbeat projection for the replacement runtime.
//!
//! This is intentionally small: it retains no listener, storage, or harness
//! implementation.  Listener lifecycle drives readiness; authenticated local
//! heartbeats only enrich the existing doctor/status payload.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use atm_core::protocol::{
    HeartbeatActivity, RuntimeLivenessState, RuntimeMemberObservation, RuntimeMemberState,
    RuntimeObservationSource, RuntimeReadinessState, RuntimeStatusCounts, RuntimeStatusSnapshot,
    TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
};
use atm_core::types::{AgentName, IsoTimestamp, SessionId, TeamName};

/// The bounded runtime-member projection is observability, not durable state.
pub const MAX_RUNTIME_STATUS_MEMBERS: usize = 4096;

#[derive(Clone, Default)]
pub struct RuntimeHealth {
    inner: Arc<Mutex<RuntimeHealthState>>,
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
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Lifecycle {
    #[default]
    NotReady,
    Ready,
    Draining,
    Stopped,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct MemberKey {
    team: TeamName,
    member: AgentName,
}

#[derive(Clone)]
struct MemberRecord {
    pid: Option<u32>,
    session_id: Option<SessionId>,
    state: RuntimeMemberState,
    last_active_at: Option<IsoTimestamp>,
    state_changed_at: Option<IsoTimestamp>,
    session_changed_at: Option<IsoTimestamp>,
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

    /// Record an already-authorized local heartbeat.
    ///
    /// The brief mutex protects only in-memory status fields; storage and
    /// process work remain outside this critical section.
    pub fn record_heartbeat(
        &self,
        request: &TeamMemberHeartbeatRequest,
    ) -> TeamMemberHeartbeatResponse {
        let mut state = self.lock();
        let key = MemberKey {
            team: request.team.clone(),
            member: request.member.clone(),
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
        });
        let next_state = match request.activity {
            HeartbeatActivity::ActiveToolUse => RuntimeMemberState::Active,
            HeartbeatActivity::Idle => RuntimeMemberState::Idle,
            HeartbeatActivity::SessionEnded => RuntimeMemberState::Offline,
        };
        if record.state != next_state {
            record.state = next_state;
            record.state_changed_at = Some(request.observed_at);
        }
        if next_state == RuntimeMemberState::Active {
            record.last_active_at = Some(request.observed_at);
        }
        let previous_pid = record.pid;
        record.pid = Some(request.pid);
        if request.session_id.is_some() && record.session_id != request.session_id {
            record.session_id = request.session_id.clone();
            record.session_changed_at = Some(request.observed_at);
        }
        TeamMemberHeartbeatResponse {
            team: request.team.clone(),
            member: request.member.clone(),
            pid: request.pid,
            pid_changed: previous_pid.is_some_and(|pid| pid != request.pid),
            state: next_state,
            last_active_at: record.last_active_at,
            session_id: record.session_id.clone(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeStatusSnapshot {
        let state = self.lock();
        let mut members: Vec<_> = state
            .members
            .iter()
            .map(|(key, record)| RuntimeMemberObservation {
                team: key.team.clone(),
                member: key.member.clone(),
                state: record.state,
                session_id: record.session_id.clone(),
                pid: record.pid,
                last_active_at: record.last_active_at,
                state_changed_by: Some(RuntimeObservationSource::Heartbeat),
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
            queue_marker_set_failures_total: state.queue_marker_set_failures_total,
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
    use atm_core::protocol::{
        HeartbeatActivity, RuntimeLivenessState, RuntimeMemberState, RuntimeReadinessState,
        TeamMemberHeartbeatRequest,
    };
    use atm_core::types::{AgentName, IsoTimestamp, TeamName};

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
        let first = health.record_heartbeat(&heartbeat(10, HeartbeatActivity::ActiveToolUse));
        assert!(!first.pid_changed);
        let second = health.record_heartbeat(&heartbeat(11, HeartbeatActivity::SessionEnded));
        assert!(second.pid_changed);
        assert_eq!(second.state, RuntimeMemberState::Offline);
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
}
