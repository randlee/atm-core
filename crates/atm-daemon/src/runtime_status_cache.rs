use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use atm_core::boundary;
use atm_core::doctor::{DoctorFinding, DoctorSeverity};
use atm_core::error::AtmError;
use atm_core::protocol::{
    HeartbeatActivity, RuntimeLivenessState, RuntimeMemberState, RuntimeReadinessState,
    RuntimeStatusCounts, RuntimeStatusSnapshot, TeamMemberHeartbeatRequest,
    TeamMemberHeartbeatResponse,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

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
    state: RuntimeMemberState,
    last_active_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeStatusCacheState {
    // Request handlers and doctor/status readers must observe one coherent
    // live-status projection. One mutex covers the full cache state so member
    // inserts/evictions and sqlite/degraded-ingest flips cannot interleave into
    // torn doctor snapshots.
    members: HashMap<RuntimeMemberKey, RuntimeMemberRecord>,
    sqlite_ready: bool,
    sqlite_detail: Option<String>,
    degraded_ingest: bool,
}

impl RuntimeStatusCacheState {
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeStatusCache {
    state: Arc<Mutex<RuntimeStatusCacheState>>,
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
            state: Arc::new(Mutex::new(RuntimeStatusCacheState {
                members: HashMap::new(),
                sqlite_ready: true,
                sqlite_detail: None,
                degraded_ingest: false,
            })),
            observability,
        }
    }

    pub(crate) fn clone_state(&self) -> Result<RuntimeStatusCacheState, AtmError> {
        self.state
            .lock()
            .map(|cache| cache.clone())
            .map_err(|_| runtime_status_cache_lock_poisoned())
    }

    pub(crate) fn replace_state(&self, next: RuntimeStatusCacheState) -> Result<(), AtmError> {
        let mut cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        *cache = next;
        Ok(())
    }

    pub(crate) fn record_heartbeat(
        &self,
        request: &TeamMemberHeartbeatRequest,
        pid_changed: bool,
    ) -> Result<TeamMemberHeartbeatResponse, AtmError> {
        let state = match request.activity {
            HeartbeatActivity::ActiveToolUse => RuntimeMemberState::Active,
            HeartbeatActivity::Idle => RuntimeMemberState::Idle,
            HeartbeatActivity::SessionEnded => RuntimeMemberState::Offline,
        };
        let key = RuntimeMemberKey {
            team: request.team.clone(),
            member: request.member.clone(),
        };
        let last_active_at = Some(request.observed_at);
        let mut cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        evict_status_cache_entry_if_needed(&mut cache, &key, &self.observability);
        cache.members.insert(
            key,
            RuntimeMemberRecord {
                pid: Some(request.pid),
                state,
                last_active_at,
            },
        );
        Ok(TeamMemberHeartbeatResponse {
            team: request.team.clone(),
            member: request.member.clone(),
            pid: request.pid,
            pid_changed,
            state,
            last_active_at,
        })
    }

    pub(crate) fn record_identity_conflict(
        &self,
        request: &TeamMemberHeartbeatRequest,
        existing_pid: u32,
    ) -> Result<(), AtmError> {
        let key = RuntimeMemberKey {
            team: request.team.clone(),
            member: request.member.clone(),
        };
        let mut cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        evict_status_cache_entry_if_needed(&mut cache, &key, &self.observability);
        let last_active_at = cache
            .members
            .get(&key)
            .and_then(|record| record.last_active_at);
        cache.members.insert(
            key,
            RuntimeMemberRecord {
                pid: Some(existing_pid),
                state: RuntimeMemberState::IdentityConflict,
                last_active_at,
            },
        );
        let event = self
            .observability
            .event(
                "record_identity_conflict",
                "degraded",
                "runtime status cache recorded an identity conflict",
            )
            .with_team(request.team.clone())
            .with_agent(request.member.clone());
        self.observability.emit_event_or_warn(event);
        Ok(())
    }

    pub(crate) fn cached_pid(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<Option<u32>, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        Ok(cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .and_then(|record| record.pid))
    }

    pub(crate) fn mark_sqlite_unavailable(&self) {
        let mut cache = self.state.lock().unwrap_or_else(|_| {
            tracing::error!(
                subsystem = "runtime_status_cache",
                action = "mark_sqlite_unavailable",
                outcome = "lock_poisoned",
                "runtime status cache lock poisoned while marking sqlite unavailable"
            );
            panic!("runtime status cache lock poisoned while marking sqlite unavailable");
        });
        cache.sqlite_ready = false;
        drop(cache);
        // Emit failure is intentionally best-effort here because the in-memory sqlite
        // readiness downgrade is the source of truth for doctor/status consumers.
        self.observability.emit_or_warn(
            "mark_sqlite_unavailable",
            "degraded",
            "runtime status cache marked sqlite unavailable",
        );
    }

    /// Record SQLite degradation detail after the caller has decided which
    /// higher-level subsystem event to emit. Callers must emit the associated
    /// observability event independently; this helper only updates the cached
    /// doctor/runtime-health projection state.
    pub(crate) fn mark_sqlite_unavailable_with_detail(&self, detail: impl Into<String>) {
        match self.state.lock() {
            Ok(mut cache) => {
                cache.sqlite_ready = false;
                cache.sqlite_detail = Some(detail.into());
            }
            Err(_) => {
                tracing::error!(
                    subsystem = "runtime_status_cache",
                    action = "mark_sqlite_unavailable_with_detail",
                    outcome = "lock_poisoned",
                    "runtime status cache lock poisoned while recording sqlite degradation detail"
                );
            }
        }
    }

    pub(crate) fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        Ok(build_runtime_snapshot_all(&cache))
    }

    pub(crate) fn snapshot_for_members(
        &self,
        members: impl IntoIterator<Item = (TeamName, AgentName)>,
    ) -> Result<RuntimeStatusSnapshot, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        Ok(build_runtime_snapshot_scoped(&cache, members))
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
        .filter(|(_, record)| record.state != RuntimeMemberState::IdentityConflict)
        .min_by_key(|(_, record)| {
            (
                record.state != RuntimeMemberState::Unknown,
                record.last_active_at,
            )
        })
        .or_else(|| {
            cache.members.iter().min_by_key(|(_, record)| {
                (
                    record.state != RuntimeMemberState::IdentityConflict,
                    record.last_active_at,
                )
            })
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
    finish_runtime_snapshot(cache, counts)
}

fn build_runtime_snapshot_scoped(
    cache: &RuntimeStatusCacheState,
    scope: impl IntoIterator<Item = (TeamName, AgentName)>,
) -> RuntimeStatusSnapshot {
    let mut counts = RuntimeStatusCounts::default();
    for (team, member) in scope {
        let key = RuntimeMemberKey { team, member };
        match cache.members.get(&key).map(|record| record.state) {
            Some(RuntimeMemberState::Active) => counts.active_members += 1,
            Some(RuntimeMemberState::Idle) => counts.idle_members += 1,
            Some(RuntimeMemberState::Offline) => counts.offline_members += 1,
            Some(RuntimeMemberState::Unknown) | Some(RuntimeMemberState::IdentityConflict) => {
                counts.unknown_members += 1
            }
            None => counts.unknown_members += 1,
        }
    }
    finish_runtime_snapshot(cache, counts)
}

fn finish_runtime_snapshot(
    cache: &RuntimeStatusCacheState,
    counts: RuntimeStatusCounts,
) -> RuntimeStatusSnapshot {
    let conflict_count = cache
        .members
        .values()
        .filter(|record| record.state == RuntimeMemberState::IdentityConflict)
        .count();
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
    } else if !cache.sqlite_ready || cache.degraded_ingest || conflict_count > 0 {
        RuntimeReadinessState::Degraded
    } else {
        RuntimeReadinessState::Ready
    };
    let mut details = Vec::new();
    if !cache.sqlite_ready {
        details.push(cache.sqlite_detail.clone().unwrap_or_else(|| {
            "sqlite-backed roster truth is unavailable; runtime cache updates are degraded"
                .to_string()
        }));
    }
    if cache.degraded_ingest {
        details.push("runtime heartbeat ingest is degraded".to_string());
    }
    if conflict_count > 0 {
        details.push(format!(
            "{conflict_count} runtime member identity conflict(s) require admin takeover or dead-pid retry"
        ));
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
        sqlite_ready: cache.sqlite_ready,
        degraded_ingest: cache.degraded_ingest,
        member_counts: counts,
    }
}

pub(crate) fn build_runtime_status_cache_state(
    current_state: Option<&RuntimeStatusCacheState>,
    roster_store: &dyn boundary::RosterStore,
) -> Result<RuntimeStatusCacheState, AtmError> {
    let mut next_state = build_empty_runtime_status_cache_state(current_state);
    let teams = roster_store
        .list_teams(atm_core::boundary::RosterStoreListTeamsRequest)?
        .teams;
    if teams.len() > MAX_RELOAD_TEAMS {
        return Err(AtmError::config(format!(
            "daemon runtime reload rejected because sqlite-backed roster truth contains more than {MAX_RELOAD_TEAMS} teams"
        ))
        .with_recovery(
            "Reduce the number of persisted ATM teams or raise the documented reload cap before retrying SIGHUP.",
        ));
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
        // This constructor always resets SQLite readiness to the optimistic
        // baseline; callers that detect assembly/open failure afterward own
        // re-applying mark_sqlite_unavailable(...) before publishing state.
        sqlite_ready: true,
        sqlite_detail: None,
        degraded_ingest: current_state.is_some_and(|state| state.degraded_ingest),
    }
}

fn hydrate_runtime_status_cache_team(
    next_state: &mut RuntimeStatusCacheState,
    current_state: Option<&RuntimeStatusCacheState>,
    roster_store: &dyn boundary::RosterStore,
    team: TeamName,
) -> Result<(), AtmError> {
    let roster = roster_store
        .load_roster(atm_core::boundary::RosterStoreLoadRosterRequest { team: team.clone() })?;
    for member in roster.members {
        if next_state.members.len() >= MAX_STATUS_CACHE_ENTRIES {
            return Err(AtmError::config(format!(
                "daemon runtime reload rejected because status-cache capacity {MAX_STATUS_CACHE_ENTRIES} would be exceeded while loading roster for team {}",
                team
            ))
            .with_recovery(
                "Reduce configured roster size or increase the documented status-cache budget before retrying SIGHUP.",
            ));
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
                state: existing
                    .map(|record| record.state)
                    .unwrap_or(RuntimeMemberState::Unknown),
                last_active_at: existing.and_then(|record| record.last_active_at),
            },
        );
    }
    Ok(())
}

pub(crate) fn runtime_status_finding(snapshot: &RuntimeStatusSnapshot) -> DoctorFinding {
    let summary = format!(
        "daemon runtime liveness is {:?}; readiness is {:?}; owner_pid={:?}; sqlite_ready={}; degraded_ingest={}; active={}, idle={}, offline={}, unknown={}",
        snapshot.liveness,
        snapshot.readiness,
        snapshot.singleton_owner_pid,
        snapshot.sqlite_ready,
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
            // SQLite readiness takes priority over ingest degradation in the combined case so
            // doctor surfaces the most actionable operator code first.
            code: if !snapshot.sqlite_ready {
                atm_core::error_codes::AtmErrorCode::WarningSqliteHealthDegraded
            } else {
                atm_core::error_codes::AtmErrorCode::WarningObservabilityHealthDegraded
            },
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
    ) -> Result<Option<RuntimeMemberState>, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        Ok(cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .map(|record| record.state))
    }

    pub(crate) fn insert_member_for_test(
        &self,
        team: TeamName,
        member: AgentName,
        pid: Option<u32>,
        state: RuntimeMemberState,
        last_active_at: Option<IsoTimestamp>,
    ) -> Result<(), AtmError> {
        let mut cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        cache.members.insert(
            RuntimeMemberKey { team, member },
            RuntimeMemberRecord {
                pid,
                state,
                last_active_at,
            },
        );
        Ok(())
    }

    pub(crate) fn member_count_for_test(&self) -> Result<usize, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| runtime_status_cache_lock_poisoned())?;
        Ok(cache.members.len())
    }

    pub(crate) fn snapshot_for_members_for_test(
        &self,
        members: impl IntoIterator<Item = (TeamName, AgentName)>,
    ) -> Result<RuntimeStatusSnapshot, AtmError> {
        self.snapshot_for_members(members)
    }

    pub(crate) fn record_heartbeat_for_test(
        &self,
        request: &TeamMemberHeartbeatRequest,
        pid_changed: bool,
    ) -> Result<TeamMemberHeartbeatResponse, AtmError> {
        self.record_heartbeat(request, pid_changed)
    }

    pub(crate) fn record_identity_conflict_for_test(
        &self,
        request: &TeamMemberHeartbeatRequest,
        existing_pid: u32,
    ) -> Result<(), AtmError> {
        self.record_identity_conflict(request, existing_pid)
    }
}

fn runtime_status_cache_lock_poisoned() -> AtmError {
    AtmError::daemon_unavailable("runtime status cache lock poisoned")
        .with_recovery("Restart the daemon; runtime status cache state can no longer be trusted.")
}
