use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use atm_core::doctor::{DoctorFinding, DoctorSeverity};
use atm_core::error::AtmError;
use atm_core::protocol::{
    HeartbeatActivity, RuntimeLivenessState, RuntimeMemberObservation, RuntimeMemberState,
    RuntimeObservationSource, RuntimeReadinessState, RuntimeStatusCounts, RuntimeStatusSnapshot,
    TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
};
use atm_core::types::{AgentName, IsoTimestamp, SessionId, TeamName};
use atm_storage::RosterStore;

use crate::{DaemonSubsystem, SubsystemObservability};

pub(crate) const MAX_STATUS_CACHE_ENTRIES: usize = 4096;
const MAX_RELOAD_TEAMS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuntimeMemberKey {
    team: TeamName,
    member: AgentName,
}

#[derive(Debug, Clone)]
struct RuntimeMemberRecord {
    pid: Option<u32>,
    session_id: Option<SessionId>,
    state: RuntimeMemberState,
    last_active_at: Option<IsoTimestamp>,
    state_changed_by: Option<RuntimeObservationSource>,
    state_changed_at: Option<IsoTimestamp>,
    session_changed_by: Option<RuntimeObservationSource>,
    session_changed_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ObservationMergeOutcome {
    pub(crate) pid_changed: bool,
    pub(crate) session_changed: bool,
    pub(crate) state_changed: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeStatusCacheState {
    members: HashMap<RuntimeMemberKey, RuntimeMemberRecord>,
    degraded_ingest: bool,
}

impl RuntimeStatusCacheState {
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeStatusCache {
    state: Arc<ArcSwap<RuntimeStatusCacheState>>,
    observability: SubsystemObservability,
}

impl Default for RuntimeStatusCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeStatusCache {
    pub(crate) fn new() -> Self {
        Self::new_with_observability(SubsystemObservability::disabled(
            DaemonSubsystem::RuntimeStatusCache,
        ))
    }

    pub(crate) fn new_with_observability(observability: SubsystemObservability) -> Self {
        Self {
            state: Arc::new(ArcSwap::from_pointee(RuntimeStatusCacheState {
                members: HashMap::new(),
                degraded_ingest: false,
            })),
            observability,
        }
    }

    pub(crate) fn clone_state(&self) -> RuntimeStatusCacheState {
        self.state.load().as_ref().clone()
    }

    pub(crate) fn publish_state(&self, next: RuntimeStatusCacheState) {
        self.state.store(Arc::new(next));
    }

    pub(crate) fn mark_degraded_ingest(&self) {
        let mut state = self.clone_state();
        state.degraded_ingest = true;
        self.publish_state(state);
    }

    pub(crate) fn record_heartbeat(
        &self,
        request: &TeamMemberHeartbeatRequest,
    ) -> TeamMemberHeartbeatResponse {
        let state = match request.activity {
            HeartbeatActivity::ActiveToolUse => RuntimeMemberState::Active,
            HeartbeatActivity::Idle => RuntimeMemberState::Idle,
            HeartbeatActivity::SessionEnded => RuntimeMemberState::Offline,
        };
        let key = RuntimeMemberKey {
            team: request.team.clone(),
            member: request.member.clone(),
        };
        let outcome = self.merge_observation(
            &key,
            RuntimeObservationSource::Heartbeat,
            state,
            request.session_id.as_ref(),
            Some(request.pid),
            request.observed_at,
        );
        let _ = (outcome.session_changed, outcome.state_changed);
        let record = self.state.load().members.get(&key).cloned();
        TeamMemberHeartbeatResponse {
            team: request.team.clone(),
            member: request.member.clone(),
            pid: request.pid,
            pid_changed: outcome.pid_changed,
            state,
            last_active_at: record.as_ref().and_then(|record| record.last_active_at),
            session_id: record.and_then(|record| record.session_id),
        }
    }

    pub(crate) fn touch_member(
        &self,
        observation: &crate::runtime_health::dispatch::TrustedActivityObservation,
        observed_at: IsoTimestamp,
    ) {
        let observation = observation.observation();
        let key = RuntimeMemberKey {
            team: observation.team.clone(),
            member: observation.member.clone(),
        };
        self.merge_observation(
            &key,
            RuntimeObservationSource::LocalCommand,
            RuntimeMemberState::Active,
            observation.session_id.as_ref(),
            observation.pid,
            observed_at,
        );
    }

    fn merge_observation(
        &self,
        key: &RuntimeMemberKey,
        source: RuntimeObservationSource,
        state: RuntimeMemberState,
        session_id: Option<&SessionId>,
        pid: Option<u32>,
        observed_at: IsoTimestamp,
    ) -> ObservationMergeOutcome {
        let mut cache = self.clone_state();
        evict_status_cache_entry_if_needed(&mut cache, key, &self.observability);
        let record = cache
            .members
            .entry(key.clone())
            .or_insert(RuntimeMemberRecord {
                pid: None,
                session_id: None,
                state: RuntimeMemberState::Unknown,
                last_active_at: None,
                state_changed_by: None,
                state_changed_at: None,
                session_changed_by: None,
                session_changed_at: None,
            });
        let state_changed = record.state != state;
        if state_changed {
            record.state = state;
            record.state_changed_by = Some(source);
            record.state_changed_at = Some(observed_at);
        }
        if state == RuntimeMemberState::Active {
            record.last_active_at = Some(observed_at);
        }
        let previous_pid = record.pid;
        let pid_mutated = pid.is_some_and(|pid| previous_pid != Some(pid));
        if let Some(pid) = pid {
            record.pid = Some(pid);
        }
        let previous_session = record.session_id.clone();
        let session_changed =
            session_id.is_some_and(|session_id| previous_session.as_ref() != Some(session_id));
        if session_changed {
            let session_id = session_id.expect("session_changed requires a session value");
            record.session_id = Some(session_id.clone());
            record.session_changed_by = Some(source);
            record.session_changed_at = Some(observed_at);
        }
        self.publish_state(cache);
        if pid_mutated || session_changed {
            let event = self
                .observability
                .event(
                    "runtime_observation_metadata_changed",
                    "success",
                    "runtime observation metadata changed",
                )
                .with_team(key.team.clone())
                .with_agent(key.member.clone())
                .with_extra_string_field("source", format!("{source:?}"))
                .with_extra_string_field("observed_at", observed_at.to_string())
                .with_extra_string_field("previous_pid", format!("{previous_pid:?}"))
                .with_extra_string_field("new_pid", format!("{:?}", pid))
                .with_extra_string_field("previous_session_id", format!("{previous_session:?}"))
                .with_extra_string_field("new_session_id", format!("{session_id:?}"));
            self.observability.emit_event_or_warn(event);
        }
        ObservationMergeOutcome {
            pid_changed: previous_pid
                .is_some_and(|previous| pid.is_some_and(|next| previous != next)),
            session_changed,
            state_changed,
        }
    }

    #[cfg(test)]
    pub(crate) fn cached_session_id(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Option<SessionId> {
        let cache = self.state.load();
        cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .and_then(|record| record.session_id.clone())
    }

    pub(crate) fn snapshot(&self) -> RuntimeStatusSnapshot {
        let cache = self.state.load();
        build_runtime_snapshot_all(&cache)
    }

    pub(crate) fn snapshot_for_members(
        &self,
        members: impl IntoIterator<Item = (TeamName, AgentName)>,
    ) -> RuntimeStatusSnapshot {
        let cache = self.state.load();
        build_runtime_snapshot_scoped(&cache, members)
    }
}

fn evict_status_cache_entry_if_needed(
    cache: &mut RuntimeStatusCacheState,
    incoming_key: &RuntimeMemberKey,
    observability: &SubsystemObservability,
) {
    let is_new_key = !cache.members.contains_key(incoming_key);
    if !is_new_key || cache.members.len() < MAX_STATUS_CACHE_ENTRIES {
        return;
    }
    let eviction_candidate = cache
        .members
        .iter()
        .min_by_key(|(_, record)| {
            (
                record.state != RuntimeMemberState::Unknown,
                record.last_active_at.or(record.state_changed_at),
            )
        })
        .map(|(key, record)| (key.clone(), record.clone()));
    if let Some((evicted_key, evicted_record)) = eviction_candidate {
        cache.members.remove(&evicted_key);
        let event = observability
            .event(
                "evict_entry",
                "degraded",
                "runtime status cache evicted an entry at the bounded cap",
            )
            .with_team(evicted_key.team.clone())
            .with_agent(evicted_key.member.clone());
        observability.emit_event_or_warn(event);
        tracing::warn!(
            subsystem = "runtime_status_cache",
            action = "evict_entry",
            outcome = "cap_exceeded",
            team = %evicted_key.team,
            member = %evicted_key.member,
            pid = evicted_record.pid,
            state = ?evicted_record.state,
            cap = MAX_STATUS_CACHE_ENTRIES,
            "evicted daemon runtime status-cache entry after reaching the bounded cap"
        );
    }
}

fn build_runtime_snapshot_all(cache: &RuntimeStatusCacheState) -> RuntimeStatusSnapshot {
    let mut counts = RuntimeStatusCounts::default();
    for record in cache.members.values() {
        match record.state {
            RuntimeMemberState::Active => counts.active_members += 1,
            RuntimeMemberState::Idle => counts.idle_members += 1,
            RuntimeMemberState::Offline => counts.offline_members += 1,
            RuntimeMemberState::Unknown | RuntimeMemberState::IdentityConflict => {
                counts.unknown_members += 1
            }
        }
    }
    let members = cache.members.iter().map(observation_from_record).collect();
    finish_runtime_snapshot(cache, counts, members)
}

fn build_runtime_snapshot_scoped(
    cache: &RuntimeStatusCacheState,
    scope: impl IntoIterator<Item = (TeamName, AgentName)>,
) -> RuntimeStatusSnapshot {
    let mut counts = RuntimeStatusCounts::default();
    let mut observations = Vec::new();
    for (team, member) in scope {
        let key = RuntimeMemberKey { team, member };
        match cache.members.get(&key) {
            Some(record) => {
                observations.push(observation_from_record((&key, record)));
                match record.state {
                    RuntimeMemberState::Active => counts.active_members += 1,
                    RuntimeMemberState::Idle => counts.idle_members += 1,
                    RuntimeMemberState::Offline => counts.offline_members += 1,
                    RuntimeMemberState::Unknown | RuntimeMemberState::IdentityConflict => {
                        counts.unknown_members += 1
                    }
                }
            }
            None => {
                observations.push(RuntimeMemberObservation {
                    team: key.team,
                    member: key.member,
                    state: RuntimeMemberState::Unknown,
                    session_id: None,
                    pid: None,
                    last_active_at: None,
                    state_changed_by: None,
                    state_changed_at: None,
                    session_changed_by: None,
                    session_changed_at: None,
                });
                counts.unknown_members += 1;
            }
        }
    }
    finish_runtime_snapshot(cache, counts, observations)
}

fn observation_from_record(
    (key, record): (&RuntimeMemberKey, &RuntimeMemberRecord),
) -> RuntimeMemberObservation {
    RuntimeMemberObservation {
        team: key.team.clone(),
        member: key.member.clone(),
        state: record.state,
        session_id: record.session_id.clone(),
        pid: record.pid,
        last_active_at: record.last_active_at,
        state_changed_by: record.state_changed_by,
        state_changed_at: record.state_changed_at,
        session_changed_by: record.session_changed_by,
        session_changed_at: record.session_changed_at,
    }
}

fn finish_runtime_snapshot(
    cache: &RuntimeStatusCacheState,
    counts: RuntimeStatusCounts,
    members: Vec<RuntimeMemberObservation>,
) -> RuntimeStatusSnapshot {
    let tracked_members = counts.active_members
        + counts.idle_members
        + counts.offline_members
        + counts.unknown_members;
    let all_tracked_members_offline = tracked_members > 0
        && counts.active_members == 0
        && counts.idle_members == 0
        && counts.unknown_members == 0
        && counts.offline_members > 0;
    let readiness = if all_tracked_members_offline {
        RuntimeReadinessState::Unavailable
    } else if cache.degraded_ingest {
        RuntimeReadinessState::Degraded
    } else {
        RuntimeReadinessState::Ready
    };
    let mut details = Vec::new();
    if cache.degraded_ingest {
        details.push("runtime heartbeat ingest is degraded".to_string());
    }
    if all_tracked_members_offline {
        details.push("all tracked daemon members are offline".to_string());
    }
    let detail = (!details.is_empty()).then(|| details.join("; "));
    RuntimeStatusSnapshot {
        liveness: RuntimeLivenessState::Running,
        readiness,
        detail,
        singleton_owner_pid: Some(std::process::id()),
        degraded_ingest: cache.degraded_ingest,
        member_counts: counts,
        members,
    }
}

pub(crate) fn build_runtime_status_cache_state(
    current_state: Option<&RuntimeStatusCacheState>,
    roster_store: &dyn RosterStore,
) -> Result<RuntimeStatusCacheState, AtmError> {
    let mut next_state = build_empty_runtime_status_cache_state(current_state);
    let teams = roster_store.list_teams()?;
    if teams.len() > MAX_RELOAD_TEAMS {
        return Err(AtmError::config(format!(
            "daemon runtime reload rejected because persisted roster state contains more than {MAX_RELOAD_TEAMS} teams"
        )));
    }
    for team in teams {
        hydrate_runtime_status_cache_team(&mut next_state, current_state, roster_store, team)?;
    }
    Ok(next_state)
}

fn build_empty_runtime_status_cache_state(
    current_state: Option<&RuntimeStatusCacheState>,
) -> RuntimeStatusCacheState {
    RuntimeStatusCacheState {
        members: HashMap::new(),
        degraded_ingest: current_state.is_some_and(|state| state.degraded_ingest),
    }
}

fn hydrate_runtime_status_cache_team(
    next_state: &mut RuntimeStatusCacheState,
    current_state: Option<&RuntimeStatusCacheState>,
    roster_store: &dyn RosterStore,
    team: TeamName,
) -> Result<(), AtmError> {
    let roster = roster_store.load_roster(&team)?;
    for member in roster.members {
        if next_state.members.len() >= MAX_STATUS_CACHE_ENTRIES {
            return Err(AtmError::config(format!(
                "daemon runtime reload rejected because status-cache capacity {MAX_STATUS_CACHE_ENTRIES} would be exceeded while loading roster for team {}; reduce the tracked roster or restart with a fresh runtime cache",
                team
            )));
        }
        let member_name = member.agent_name;
        let key = RuntimeMemberKey {
            team: team.clone(),
            member: member_name,
        };
        let existing = current_state.and_then(|state| state.members.get(&key));
        next_state.members.insert(
            key,
            RuntimeMemberRecord {
                pid: existing.and_then(|record| record.pid),
                session_id: existing.and_then(|record| record.session_id.clone()),
                state: existing
                    .map(|record| record.state)
                    .unwrap_or(RuntimeMemberState::Unknown),
                last_active_at: existing.and_then(|record| record.last_active_at),
                state_changed_by: existing.and_then(|record| record.state_changed_by),
                state_changed_at: existing.and_then(|record| record.state_changed_at),
                session_changed_by: existing.and_then(|record| record.session_changed_by),
                session_changed_at: existing.and_then(|record| record.session_changed_at),
            },
        );
    }
    Ok(())
}

pub(crate) fn runtime_status_finding(snapshot: &RuntimeStatusSnapshot) -> DoctorFinding {
    let summary = format!(
        "daemon runtime liveness is {:?}; readiness is {:?}; owner_pid={:?}; degraded_ingest={}; active={}, idle={}, offline={}, unknown={}",
        snapshot.liveness,
        snapshot.readiness,
        snapshot.singleton_owner_pid,
        snapshot.degraded_ingest,
        snapshot.member_counts.active_members,
        snapshot.member_counts.idle_members,
        snapshot.member_counts.offline_members,
        snapshot.member_counts.unknown_members,
    );
    match snapshot.readiness {
        RuntimeReadinessState::Ready => DoctorFinding {
            severity: DoctorSeverity::Info,
            code: atm_core::error_codes::AtmErrorCode::ObservabilityHealthOk,
            message: summary,
            remediation: None,
        },
        RuntimeReadinessState::Degraded => DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: atm_core::error_codes::AtmErrorCode::WarningSendAlertStateDegraded,
            message: summary,
            remediation: snapshot.detail.clone().or(Some(
                "Restore daemon runtime backing services and rerun `atm doctor`.".to_string(),
            )),
        },
        RuntimeReadinessState::Unavailable => DoctorFinding {
            severity: DoctorSeverity::Error,
            code: atm_core::error_codes::AtmErrorCode::DaemonUnavailable,
            message: summary,
            remediation: snapshot.detail.clone().or(Some(
                "Restore daemon runtime availability and rerun `atm doctor`.".to_string(),
            )),
        },
    }
}

#[cfg(test)]
impl RuntimeStatusCache {
    pub(crate) fn member_state_for_test(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Option<RuntimeMemberState> {
        let cache = self.state.load();
        cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .map(|record| record.state)
    }

    pub(crate) fn insert_member_for_test(
        &self,
        team: TeamName,
        member: AgentName,
        pid: Option<u32>,
        state: RuntimeMemberState,
        last_active_at: Option<IsoTimestamp>,
    ) {
        let mut cache = self.clone_state();
        cache.members.insert(
            RuntimeMemberKey { team, member },
            RuntimeMemberRecord {
                pid,
                session_id: None,
                state,
                last_active_at,
                state_changed_by: None,
                state_changed_at: None,
                session_changed_by: None,
                session_changed_at: None,
            },
        );
        self.publish_state(cache);
    }

    pub(crate) fn member_count_for_test(&self) -> usize {
        let cache = self.state.load();
        cache.members.len()
    }

    pub(crate) fn snapshot_for_members_for_test(
        &self,
        members: impl IntoIterator<Item = (TeamName, AgentName)>,
    ) -> RuntimeStatusSnapshot {
        self.snapshot_for_members(members)
    }

    pub(crate) fn record_heartbeat_for_test(
        &self,
        request: &TeamMemberHeartbeatRequest,
        _legacy_pid_changed: bool,
    ) -> TeamMemberHeartbeatResponse {
        self.record_heartbeat(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::protocol::{
        HeartbeatActivity, RuntimeMemberState, RuntimeReadinessState, TeamMemberHeartbeatRequest,
    };
    use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_QA, TEST_RECIPIENT, TEST_SENDER};
    use atm_core::types::AgentName;

    fn test_team() -> TeamName {
        "qa-team".parse().expect("team")
    }

    #[test]
    fn runtime_status_cache_heartbeat_publish_is_atomically_visible() {
        let status_cache = RuntimeStatusCache::new();
        let team = test_team();
        let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");

        status_cache.record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: member.clone(),
                pid: std::process::id(),
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
                session_id: None,
            },
            false,
        );

        let snapshot = status_cache.snapshot();
        assert_eq!(snapshot.readiness, RuntimeReadinessState::Ready);
        assert!(!snapshot.degraded_ingest);
        assert_eq!(snapshot.member_counts.active_members, 1);
        assert_eq!(snapshot.member_counts.idle_members, 0);
        assert_eq!(snapshot.member_counts.offline_members, 0);
        assert_eq!(snapshot.member_counts.unknown_members, 0);

        let scoped = status_cache.snapshot_for_members_for_test([(team, member)]);
        assert_eq!(scoped.member_counts.active_members, 1);
        assert_eq!(scoped.member_counts.unknown_members, 0);
    }

    #[test]
    fn heartbeat_session_metadata_is_retained_when_a_later_observation_is_absent() {
        let status_cache = RuntimeStatusCache::new();
        let team = test_team();
        let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
        let session_id = SessionId::new("session-a").expect("session id");
        let request = TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 41,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: Some(session_id.clone()),
        };
        status_cache.record_heartbeat_for_test(&request, false);
        status_cache.record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                session_id: None,
                ..request
            },
            false,
        );
        assert_eq!(
            status_cache.cached_session_id(&team, &member),
            Some(session_id)
        );
    }

    #[test]
    fn session_ended_preserves_last_known_session() {
        let status_cache = RuntimeStatusCache::new();
        let team = test_team();
        let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
        let session_id = SessionId::new("s-1").expect("session id");
        status_cache.record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: member.clone(),
                pid: 42,
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
                session_id: Some(session_id.clone()),
            },
            false,
        );
        let response = status_cache.record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                team,
                member,
                pid: 42,
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::SessionEnded,
                session_id: None,
            },
            false,
        );
        assert_eq!(response.state, RuntimeMemberState::Offline);
        assert_eq!(response.session_id, Some(session_id));
    }

    #[test]
    fn runtime_status_cache_scoped_snapshot_reads_do_not_require_shared_locking() {
        let status_cache = RuntimeStatusCache::new();
        let team = test_team();
        let active: AgentName = TEST_QA.parse().expect("member");
        let idle: AgentName = TEST_RECIPIENT.parse().expect("member");
        let missing: AgentName = TEST_SENDER.parse().expect("member");

        status_cache.insert_member_for_test(
            team.clone(),
            active.clone(),
            Some(100),
            RuntimeMemberState::Active,
            Some(IsoTimestamp::now()),
        );
        status_cache.insert_member_for_test(
            team.clone(),
            idle.clone(),
            Some(101),
            RuntimeMemberState::Idle,
            Some(IsoTimestamp::now()),
        );

        let snapshot = status_cache.snapshot_for_members_for_test([
            (team.clone(), active),
            (team.clone(), idle),
            (team, missing),
        ]);

        assert_eq!(snapshot.readiness, RuntimeReadinessState::Ready);
        assert_eq!(snapshot.member_counts.active_members, 1);
        assert_eq!(snapshot.member_counts.idle_members, 1);
        assert_eq!(snapshot.member_counts.offline_members, 0);
        assert_eq!(snapshot.member_counts.unknown_members, 1);
    }

    #[test]
    fn runtime_status_cache_all_tracked_members_offline_are_unavailable() {
        let status_cache = RuntimeStatusCache::new();
        let team = test_team();
        let member: AgentName = TEST_SENDER.parse().expect("member");

        status_cache.insert_member_for_test(
            team,
            member,
            Some(200),
            RuntimeMemberState::Offline,
            Some(IsoTimestamp::now()),
        );

        let snapshot = status_cache.snapshot();
        assert_eq!(snapshot.readiness, RuntimeReadinessState::Unavailable);
        assert_eq!(snapshot.member_counts.active_members, 0);
        assert_eq!(snapshot.member_counts.offline_members, 1);
        assert_eq!(snapshot.member_counts.unknown_members, 0);
        assert_eq!(
            snapshot.detail.as_deref(),
            Some("all tracked daemon members are offline")
        );
    }
}
