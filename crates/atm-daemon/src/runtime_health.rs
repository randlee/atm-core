use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::ack_mail,
    boundary,
    clear::clear_mail,
    doctor::{
        self, DoctorFinding, DoctorQuery, DoctorReport, DoctorSeverity, DoctorStatus, DoctorSummary,
    },
    error::AtmError,
    observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    },
    process::process_is_alive,
    protocol::{
        HeartbeatActivity, RuntimeLivenessState, RuntimeMemberState, RuntimeReadinessState,
        RuntimeStatusCounts, RuntimeStatusSnapshot, SendRequestEnvelope, SendResponseEnvelope,
        TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    },
    read::read_mail,
    schema::TeamConfig,
    send::send_mail,
    types::{AgentName, IsoTimestamp, TeamName},
};
use atm_rusqlite::{SqliteBoundaryAssembly, assemble_default_boundary};

const MAX_STATUS_CACHE_ENTRIES: usize = 4096;

#[derive(Debug, Clone)]
struct DaemonObservability {
    home_dir: PathBuf,
    retained_sink_fault: RetainedSinkFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedSinkFault {
    Healthy,
    Degraded,
    Unavailable,
}

impl DaemonObservability {
    fn new(home_dir: PathBuf) -> Self {
        let retained_sink_fault = match std::env::var("ATM_OBSERVABILITY_RETAINED_SINK_FAULT")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("degraded") => RetainedSinkFault::Degraded,
            Some("unavailable") => RetainedSinkFault::Unavailable,
            _ => RetainedSinkFault::Healthy,
        };
        Self {
            home_dir,
            retained_sink_fault,
        }
    }
}

impl boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        // R.15 keeps retained-log emission owned by the shared observability
        // stack, so the daemon-side health adapter is intentionally a no-op.
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        // Runtime health only needs the shared observability surface to be
        // queryable; command-log projection still lives in atm-core.
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        // Doctor health does not tail logs directly in R.15, so the daemon
        // adapter exposes an empty tail session instead of a second log owner.
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        #[cfg(unix)]
        let active_log_path = self
            .home_dir
            .join(".local")
            .join("share")
            .join("logs")
            .join("atm.log.jsonl");
        #[cfg(not(unix))]
        let active_log_path = self.home_dir.join("logs").join("atm.log.jsonl");
        let logging_state = match self.retained_sink_fault {
            RetainedSinkFault::Healthy => AtmObservabilityHealthState::Healthy,
            RetainedSinkFault::Degraded => AtmObservabilityHealthState::Degraded,
            RetainedSinkFault::Unavailable => AtmObservabilityHealthState::Unavailable,
        };
        Ok(AtmObservabilityHealth {
            active_log_path: Some(active_log_path),
            logging_state,
            query_state: Some(AtmObservabilityHealthState::Healthy),
            detail: None,
        })
    }
}

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

#[derive(Debug, Default)]
struct RuntimeStatusCacheState {
    // Request handlers and doctor/status readers update and snapshot the cache
    // concurrently, so one mutex protects the whole live-status projection.
    members: HashMap<RuntimeMemberKey, RuntimeMemberRecord>,
    sqlite_ready: bool,
    degraded_ingest: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeStatusCache {
    state: Arc<Mutex<RuntimeStatusCacheState>>,
}

impl RuntimeStatusCache {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RuntimeStatusCacheState {
                members: HashMap::new(),
                sqlite_ready: true,
                degraded_ingest: false,
            })),
        }
    }

    fn hydrate_member(
        &self,
        team: TeamName,
        member: AgentName,
        pid: Option<u32>,
    ) -> Result<(), AtmError> {
        let mut cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        let key = RuntimeMemberKey { team, member };
        cache.members.entry(key).or_insert(RuntimeMemberRecord {
            pid,
            state: RuntimeMemberState::Unknown,
            last_active_at: None,
        });
        Ok(())
    }

    fn record_heartbeat(
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
        let current_key = key.clone();
        let last_active_at = Some(request.observed_at);
        let mut cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        cache.members.insert(
            key,
            RuntimeMemberRecord {
                pid: Some(request.pid),
                state,
                last_active_at,
            },
        );
        if cache.members.len() > MAX_STATUS_CACHE_ENTRIES
            && let Some((evicted_key, evicted_record)) = cache
                .members
                .iter()
                .filter(|(candidate, record)| {
                    **candidate != current_key
                        && record.state != RuntimeMemberState::IdentityConflict
                        && record.state != RuntimeMemberState::Unknown
                })
                .min_by_key(|(_, record)| record.last_active_at)
                .map(|(key, record)| (key.clone(), record.clone()))
            && let Some(record) = cache.members.get_mut(&evicted_key)
        {
            record.state = RuntimeMemberState::Unknown;
            tracing::warn!(
                team = %evicted_key.team,
                member = %evicted_key.member,
                pid = evicted_record.pid,
                "demoted daemon runtime status-cache entry to explicit unknown after reaching the bounded cap"
            );
        }
        cache.sqlite_ready = true;
        Ok(TeamMemberHeartbeatResponse {
            team: request.team.clone(),
            member: request.member.clone(),
            pid: request.pid,
            pid_changed,
            state,
            last_active_at,
        })
    }

    fn record_identity_conflict(
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
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
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
        Ok(())
    }

    fn cached_pid(&self, team: &TeamName, member: &AgentName) -> Result<Option<u32>, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        Ok(cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .and_then(|record| record.pid))
    }

    pub(crate) fn mark_sqlite_unavailable(&self) {
        match self.state.lock() {
            Ok(mut cache) => cache.sqlite_ready = false,
            Err(_) => {
                tracing::error!(
                    "runtime status cache lock poisoned while marking sqlite unavailable"
                );
            }
        }
    }

    pub(crate) fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        Ok(build_runtime_snapshot_all(&cache))
    }

    fn snapshot_for_members(
        &self,
        members: impl IntoIterator<Item = (TeamName, AgentName)>,
    ) -> Result<RuntimeStatusSnapshot, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        Ok(build_runtime_snapshot_scoped(&cache, members))
    }

    #[cfg(test)]
    pub(crate) fn member_state_for_test(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<Option<RuntimeMemberState>, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        Ok(cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .map(|record| record.state))
    }

    #[cfg(test)]
    pub(crate) fn hydrate_member_for_test(
        &self,
        team: TeamName,
        member: AgentName,
        pid: Option<u32>,
    ) -> Result<(), AtmError> {
        self.hydrate_member(team, member, pid)
    }

    #[cfg(test)]
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
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
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
        details.push(
            "sqlite-backed durable pid continuity is unavailable; runtime cache updates are degraded"
                .to_string(),
        );
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

#[derive(Debug)]
pub(crate) struct DaemonRequestDispatcher {
    observability: DaemonObservability,
    status_cache: RuntimeStatusCache,
    sqlite_boundary: Option<SqliteBoundaryAssembly>,
}

impl DaemonRequestDispatcher {
    pub(crate) fn new(home_dir: PathBuf, status_cache: RuntimeStatusCache) -> Self {
        let sqlite_boundary = match assemble_default_boundary() {
            Ok(boundary) => {
                if let Err(error) =
                    hydrate_runtime_status_cache(&status_cache, &home_dir, boundary.roster_store())
                {
                    tracing::warn!(%error, "failed to hydrate runtime status cache from sqlite roster state");
                    status_cache.mark_sqlite_unavailable();
                }
                Some(boundary)
            }
            Err(error) => {
                tracing::warn!(%error, "failed to assemble default sqlite boundary for daemon runtime health");
                status_cache.mark_sqlite_unavailable();
                None
            }
        };
        Self {
            observability: DaemonObservability::new(home_dir),
            status_cache,
            sqlite_boundary,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        home_dir: PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: PathBuf,
    ) -> Self {
        let sqlite_boundary = match atm_rusqlite::assemble_boundary(&roster_db_path) {
            Ok(boundary) => {
                if let Err(error) =
                    hydrate_runtime_status_cache(&status_cache, &home_dir, boundary.roster_store())
                {
                    tracing::warn!(%error, "failed to hydrate test runtime status cache from sqlite roster state");
                    status_cache.mark_sqlite_unavailable();
                }
                Some(boundary)
            }
            Err(error) => {
                tracing::warn!(%error, path = %roster_db_path.display(), "failed to assemble sqlite boundary for test daemon runtime health");
                status_cache.mark_sqlite_unavailable();
                None
            }
        };
        Self {
            observability: DaemonObservability::new(home_dir),
            status_cache,
            sqlite_boundary,
        }
    }
}

impl boundary::sealed::Sealed for DaemonRequestDispatcher {}

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_mail(request, &self.observability)?,
                )))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail(request, &self.observability)?,
                )))
            }
            RequestEnvelope::Heartbeat(request) => {
                Ok(ResponseEnvelope::Heartbeat(self.record_heartbeat(request)?))
            }
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(read_mail(
                query,
                &self.observability,
            )?)),
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(clear_mail(
                query,
                &self.observability,
            )?)),
            RequestEnvelope::Doctor(query) => {
                Ok(ResponseEnvelope::Doctor(self.project_doctor_report(query)?))
            }
        }
    }
}

impl DaemonRequestDispatcher {
    fn record_heartbeat(
        &self,
        request: TeamMemberHeartbeatRequest,
    ) -> Result<TeamMemberHeartbeatResponse, AtmError> {
        let roster_store = self
            .sqlite_boundary
            .as_ref()
            .map(SqliteBoundaryAssembly::roster_store)
            .ok_or_else(|| {
                self.status_cache.mark_sqlite_unavailable();
                AtmError::daemon_unavailable(
                    "sqlite-backed durable pid continuity is unavailable for daemon heartbeats",
                )
            })?;
        let cached_pid = self
            .status_cache
            .cached_pid(&request.team, &request.member)?;
        let durable_pid = if cached_pid.is_some() {
            None
        } else {
            roster_store
                .query_membership(atm_core::boundary::RosterStoreQueryMembershipRequest {
                    team: request.team.clone(),
                    member: request.member.clone(),
                })?
                .pid
        };
        if let Some(existing_pid) = cached_pid.or(durable_pid).filter(|pid| *pid != request.pid)
            && process_is_alive(existing_pid)
        {
            self.status_cache
                .record_identity_conflict(&request, existing_pid)?;
            return Err(AtmError::identity_conflict(
                "ATM_IDENTITY_CONFLICT: stop and report to user immediately",
            ));
        }
        let durable = roster_store.record_heartbeat(
            atm_core::boundary::RosterStoreRecordHeartbeatRequest {
                team: request.team.clone(),
                member: request.member.clone(),
                pid: request.pid,
                observed_at: request.observed_at,
            },
        )?;
        self.status_cache
            .record_heartbeat(&request, durable.pid_changed)
    }

    fn project_doctor_report(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        let mut report = doctor::run_doctor(query, &self.observability)?;
        let runtime_status = match &report.member_roster {
            Some(roster) => self.status_cache.snapshot_for_members(
                roster
                    .members
                    .iter()
                    .map(|member| (roster.team.clone(), member.name.clone())),
            )?,
            None => self.status_cache.snapshot()?,
        };
        report
            .findings
            .push(runtime_status_finding(&runtime_status));
        report.recommendations = report
            .findings
            .iter()
            .filter_map(|finding| finding.remediation.clone())
            .collect();
        let status = doctor::health::status_from_findings(&report.findings);
        let (info_count, warning_count, error_count) = report.findings.iter().fold(
            (0usize, 0usize, 0usize),
            |(info, warning, error), finding| match finding.severity {
                DoctorSeverity::Info => (info + 1, warning, error),
                DoctorSeverity::Warning => (info, warning + 1, error),
                DoctorSeverity::Error => (info, warning, error + 1),
            },
        );
        let message = match status {
            DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
            DoctorStatus::Warning => "ATM doctor completed with warnings",
            DoctorStatus::Error => "ATM doctor found critical issues",
        };
        report.summary = DoctorSummary {
            status,
            message: message.to_string(),
            info_count,
            warning_count,
            error_count,
        };
        report.runtime_status = Some(runtime_status);
        Ok(report)
    }
}

fn hydrate_runtime_status_cache(
    status_cache: &RuntimeStatusCache,
    home_dir: &std::path::Path,
    roster_store: &dyn boundary::RosterStore,
) -> Result<(), AtmError> {
    let teams_root = home_dir.join(".claude").join("teams");
    if !teams_root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&teams_root).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to enumerate daemon team configs under {}",
            teams_root.display()
        ))
        .with_source(error)
    })? {
        let entry = entry.map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read daemon team-config entry under {}",
                teams_root.display()
            ))
            .with_source(error)
        })?;
        let team_name = entry.file_name().to_string_lossy().into_owned();
        let team: TeamName = team_name.parse().map_err(|error| {
            AtmError::config(format!(
                "runtime_health: invalid team name from storage under {}: {team_name}",
                teams_root.display()
            ))
            .with_source(error)
        })?;
        let config_path = entry.path().join("config.json");
        if !config_path.is_file() {
            continue;
        }
        let raw = std::fs::read(&config_path).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read daemon team config {}",
                config_path.display()
            ))
            .with_source(error)
        })?;
        let config: TeamConfig = serde_json::from_slice(&raw).map_err(|error| {
            AtmError::config(format!(
                "failed to parse daemon team config {}: {error}",
                config_path.display()
            ))
            .with_source(error)
        })?;
        for member in config.members {
            let member_name = member.name;
            let membership = roster_store.query_membership(
                atm_core::boundary::RosterStoreQueryMembershipRequest {
                    team: team.clone(),
                    member: member_name.clone(),
                },
            )?;
            status_cache.hydrate_member(team.clone(), member_name, membership.pid)?;
        }
    }
    Ok(())
}

fn runtime_status_finding(snapshot: &RuntimeStatusSnapshot) -> DoctorFinding {
    let summary = format!(
        "daemon runtime liveness is {:?}; readiness is {:?}; active={}, idle={}, offline={}, unknown={}",
        snapshot.liveness,
        snapshot.readiness,
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
            code: atm_core::error_codes::AtmErrorCode::WarningObservabilityHealthDegraded,
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

#[derive(Debug, Clone)]
pub(crate) struct DaemonStatusSource {
    status_cache: RuntimeStatusCache,
}

impl DaemonStatusSource {
    pub(crate) fn new(status_cache: RuntimeStatusCache) -> Self {
        Self { status_cache }
    }
}

impl boundary::sealed::Sealed for DaemonStatusSource {}

impl boundary::StatusSource for DaemonStatusSource {
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        self.status_cache.snapshot()
    }
}
