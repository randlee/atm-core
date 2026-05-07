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
    send::send_mail,
    types::{AgentName, IsoTimestamp, TeamName},
};
use atm_rusqlite::{assemble_boundary, assemble_default_boundary};

const MAX_STATUS_CACHE_ENTRIES: usize = 4096;

#[derive(Debug, Clone)]
struct DaemonObservability {
    home_dir: PathBuf,
}

impl DaemonObservability {
    fn new(home_dir: PathBuf) -> Self {
        Self { home_dir }
    }
}

impl boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
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
        let fault = std::env::var("ATM_OBSERVABILITY_RETAINED_SINK_FAULT")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        let logging_state = match fault.as_deref() {
            Some("degraded") => AtmObservabilityHealthState::Degraded,
            Some("unavailable") => AtmObservabilityHealthState::Unavailable,
            _ => AtmObservabilityHealthState::Healthy,
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
    pid: u32,
    state: RuntimeMemberState,
    last_active_at: Option<IsoTimestamp>,
}

#[derive(Debug, Default)]
struct RuntimeStatusCacheState {
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
                pid: request.pid,
                state,
                last_active_at,
            },
        );
        if cache.members.len() > MAX_STATUS_CACHE_ENTRIES
            && let Some((evicted_key, evicted_record)) = cache
                .members
                .iter()
                .filter(|(candidate, _)| **candidate != current_key)
                .min_by_key(|(_, record)| record.last_active_at)
                .map(|(key, record)| (key.clone(), record.clone()))
        {
            cache.members.remove(&evicted_key);
            tracing::warn!(
                team = %evicted_key.team,
                member = %evicted_key.member,
                pid = evicted_record.pid,
                "evicted daemon runtime status-cache entry after reaching the bounded cap"
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

    pub(crate) fn mark_sqlite_unavailable(&self) {
        if let Ok(mut cache) = self.state.lock() {
            cache.sqlite_ready = false;
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
}

fn build_runtime_snapshot_all(cache: &RuntimeStatusCacheState) -> RuntimeStatusSnapshot {
    let mut counts = RuntimeStatusCounts::default();
    for record in cache.members.values() {
        match record.state {
            RuntimeMemberState::Active => counts.active_members += 1,
            RuntimeMemberState::Idle => counts.idle_members += 1,
            RuntimeMemberState::Offline => counts.offline_members += 1,
            RuntimeMemberState::Unknown => counts.unknown_members += 1,
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
            Some(RuntimeMemberState::Unknown) | None => counts.unknown_members += 1,
        }
    }
    finish_runtime_snapshot(cache, counts)
}

fn finish_runtime_snapshot(
    cache: &RuntimeStatusCacheState,
    counts: RuntimeStatusCounts,
) -> RuntimeStatusSnapshot {
    let readiness = if cache.sqlite_ready {
        RuntimeReadinessState::Ready
    } else {
        RuntimeReadinessState::Degraded
    };
    let detail = (!cache.sqlite_ready).then_some(
        "sqlite-backed durable pid continuity is unavailable; runtime cache updates are degraded"
            .to_string(),
    );
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

#[derive(Debug, Clone)]
pub(crate) struct DaemonRequestDispatcher {
    observability: DaemonObservability,
    status_cache: RuntimeStatusCache,
    roster_db_path: Option<PathBuf>,
}

impl DaemonRequestDispatcher {
    pub(crate) fn new(home_dir: PathBuf, status_cache: RuntimeStatusCache) -> Self {
        Self {
            observability: DaemonObservability::new(home_dir),
            status_cache,
            roster_db_path: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        home_dir: PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: PathBuf,
    ) -> Self {
        Self {
            observability: DaemonObservability::new(home_dir),
            status_cache,
            roster_db_path: Some(roster_db_path),
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
        let assembly = match &self.roster_db_path {
            Some(path) => assemble_boundary(path),
            None => assemble_default_boundary(),
        }
        .inspect_err(|_| {
            self.status_cache.mark_sqlite_unavailable();
        })?;
        let roster_store = assembly.roster_store();
        let membership = roster_store.query_membership(
            atm_core::boundary::RosterStoreQueryMembershipRequest {
                team: request.team.clone(),
                member: request.member.clone(),
            },
        )?;
        if !membership.is_member {
            return Err(AtmError::agent_not_found(
                request.member.as_str(),
                request.team.as_str(),
            ));
        }
        if let Some(existing_pid) = membership.pid.filter(|pid| *pid != request.pid)
            && process_is_alive(existing_pid)
        {
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
