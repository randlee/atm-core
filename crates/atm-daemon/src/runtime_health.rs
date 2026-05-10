use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::ack_mail,
    boundary,
    clear::clear_mail,
    doctor::{
        self, DoctorFinding, DoctorQuery, DoctorReport, DoctorSeverity, DoctorStatus, DoctorSummary,
    },
    error::AtmError,
    list::list_mail,
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

use crate::AtmHomeDir;
use crate::daemon_runtime_observability::DaemonRuntimeObservability;

const MAX_STATUS_CACHE_ENTRIES: usize = 4096;
const MAX_RELOAD_TEAMS: usize = 256;
const SHUTDOWN_WAL_CHECKPOINT_DEADLINE: Duration = Duration::from_secs(2);
// The retained observability flush is best-effort during shutdown; Phase S records this bounded
// 2-second deadline as an accepted production exception in the anti-flake contract docs.
const SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE: Duration = Duration::from_secs(2);

// Timed-out shutdown workers are retained in one process-wide registry instead of being dropped
// orphaned; tests drain the registry explicitly and production keeps the handles reachable until
// process shutdown completes.
static SHUTDOWN_FINALIZER_THREADS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

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

    fn clone_state(&self) -> Result<RuntimeStatusCacheState, AtmError> {
        self.state
            .lock()
            .map(|cache| cache.clone())
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))
    }

    fn replace_state(&self, next: RuntimeStatusCacheState) -> Result<(), AtmError> {
        let mut cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        *cache = next;
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

pub(crate) struct DaemonRequestDispatcher {
    // Invariant: this is the validated ATM_HOME root for the running daemon,
    // not an arbitrary workspace path.
    home_dir: PathBuf,
    observability: Arc<dyn DaemonRuntimeObservability>,
    status_cache: RuntimeStatusCache,
    sqlite_boundary: Option<SqliteBoundaryAssembly>,
}

impl std::fmt::Debug for DaemonRequestDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRequestDispatcher")
            .field("home_dir", &self.home_dir)
            .field("status_cache", &self.status_cache)
            .field("sqlite_boundary_present", &self.sqlite_boundary.is_some())
            .finish()
    }
}

impl DaemonRequestDispatcher {
    #[cfg(test)]
    pub(crate) fn drain_shutdown_finalizer_threads_for_test() {
        let handles = {
            let mut handles = SHUTDOWN_FINALIZER_THREADS
                .lock()
                .expect("shutdown finalizer thread registry lock");
            std::mem::take(&mut *handles)
        };
        for handle in handles {
            let (result_tx, result_rx) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = result_tx.send(handle.join());
            });
            match result_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    panic!("shutdown finalizer thread failed to join within 5s")
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("shutdown finalizer join helper exited before reporting completion")
                }
            }
        }
    }

    fn run_bounded_shutdown_step(
        label: &'static str,
        deadline: Duration,
        step: impl FnOnce() -> Result<(), AtmError> + Send + 'static,
    ) {
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let shutdown_handle = std::thread::spawn(move || {
            let _ = result_tx.send(step());
        });
        match result_rx.recv_timeout(deadline) {
            Ok(Ok(())) => {
                let _ = shutdown_handle.join();
            }
            Ok(Err(error)) => {
                let _ = shutdown_handle.join();
                tracing::warn!(%error, step = label, "daemon shutdown finalizer step failed");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // SQLite WAL checkpoint timeout is the highest-risk caller here: a checkpoint can
                // outlive the bounded shutdown window, but retaining the JoinHandle is still safer
                // than dropping it orphaned because tests and orderly process teardown can join it
                // later once the blocking storage step finishes.
                SHUTDOWN_FINALIZER_THREADS
                    .lock()
                    .expect("shutdown finalizer thread registry lock")
                    .push(shutdown_handle);
                tracing::warn!(
                    step = label,
                    timeout_ms = deadline.as_millis(),
                    "daemon shutdown finalizer step exceeded its deadline; worker retained for later join"
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = shutdown_handle.join();
                tracing::warn!(
                    step = label,
                    "daemon shutdown finalizer step exited before reporting a result"
                );
            }
        }
    }

    pub(crate) fn new(
        // Must be the validated ATM home dir for this daemon runtime.
        home_dir: AtmHomeDir,
        status_cache: RuntimeStatusCache,
        observability: Arc<dyn DaemonRuntimeObservability>,
    ) -> Self {
        let home_dir = home_dir.into_inner();
        let sqlite_boundary = match assemble_default_boundary() {
            Ok(boundary) => {
                if let Err(error) =
                    build_runtime_status_cache_state(None, &home_dir, boundary.roster_store())
                        .and_then(|state| status_cache.replace_state(state))
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
            home_dir: home_dir.clone(),
            observability,
            status_cache,
            sqlite_boundary,
        }
    }

    pub(crate) fn emit_runtime_event(
        &self,
        action: &'static str,
        outcome: &'static str,
        message: &'static str,
    ) {
        if let Err(error) = self
            .observability
            .emit_runtime_event(action, outcome, message)
        {
            tracing::warn!(%error, action, outcome, "daemon runtime lifecycle emission failed");
        }
    }
}

impl boundary::sealed::Sealed for DaemonRequestDispatcher {}

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_mail(request, self.observability.as_ref())?,
                )))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail(request, self.observability.as_ref())?,
                )))
            }
            RequestEnvelope::Heartbeat(request) => {
                Ok(ResponseEnvelope::Heartbeat(self.record_heartbeat(request)?))
            }
            RequestEnvelope::List(query) => Ok(ResponseEnvelope::List(list_mail(
                query,
                self.observability.as_ref(),
            )?)),
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(read_mail(
                query,
                self.observability.as_ref(),
            )?)),
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(clear_mail(
                query,
                self.observability.as_ref(),
            )?)),
            RequestEnvelope::Doctor(query) => {
                Ok(ResponseEnvelope::Doctor(self.project_doctor_report(query)?))
            }
        }
    }
}

impl DaemonRequestDispatcher {
    pub(crate) fn reload_runtime_view(&self) -> Result<(), AtmError> {
        let roster_store = self
            .sqlite_boundary
            .as_ref()
            .map(SqliteBoundaryAssembly::roster_store)
            .ok_or_else(|| {
                self.status_cache.mark_sqlite_unavailable();
                AtmError::daemon_unavailable(
                    "sqlite-backed daemon runtime reload is unavailable because the sqlite boundary is not assembled",
                )
                .with_recovery(
                    "Restore the host-scoped ATM SQLite durable-state database and restart atm-daemon before retrying SIGHUP reload.",
                )
            })?;
        let current_state = self.status_cache.clone_state()?;
        let next_state =
            build_runtime_status_cache_state(Some(&current_state), &self.home_dir, roster_store)?;
        let reloaded_members = next_state.members.len();
        self.status_cache.replace_state(next_state)?;
        tracing::info!(
            reloaded_members,
            "bounded daemon config/roster reload applied successfully"
        );
        Ok(())
    }

    pub(crate) fn finalize_shutdown(&self) {
        if let Some(boundary) = self.sqlite_boundary.clone() {
            Self::run_bounded_shutdown_step(
                "sqlite_wal_checkpoint",
                SHUTDOWN_WAL_CHECKPOINT_DEADLINE,
                move || boundary.checkpoint_wal(),
            );
        }
        let observability = self.observability.clone();
        Self::run_bounded_shutdown_step(
            "observability_flush",
            SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE,
            // The finalizer step already runs on a dedicated shutdown thread,
            // so the retained-log flush remains in a sync context.
            move || observability.best_effort_flush_blocking(),
        );
    }

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
                .with_recovery(
                    "Restore the host-scoped ATM SQLite database and restart atm-daemon before retrying heartbeat traffic.",
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
            )
            .with_recovery(
                "Stop the conflicting ATM process, confirm the stale PID is gone, then retry the heartbeat from the active runtime owner.",
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
        let mut report = doctor::run_doctor(query, self.observability.as_ref())?;
        let daemon_observability_finding = match self.observability.health() {
            Ok(health) => daemon_observability_finding(&health),
            Err(error) => doctor::health::observability_finding_from_error(&error),
        };
        report.findings.push(daemon_observability_finding);
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

fn build_runtime_status_cache_state(
    current_state: Option<&RuntimeStatusCacheState>,
    home_dir: &std::path::Path,
    roster_store: &dyn boundary::RosterStore,
) -> Result<RuntimeStatusCacheState, AtmError> {
    let mut next_state = RuntimeStatusCacheState {
        members: HashMap::new(),
        sqlite_ready: true,
        degraded_ingest: current_state.is_some_and(|state| state.degraded_ingest),
    };
    let teams_root = home_dir.join(".claude").join("teams");
    if !teams_root.is_dir() {
        return Ok(next_state);
    }
    let entries = std::fs::read_dir(&teams_root).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to enumerate daemon team configs under {}",
            teams_root.display()
        ))
        .with_recovery(
            "Restore read access to the daemon team configuration tree under ATM_HOME before retrying atm-daemon startup.",
        )
        .with_source(error)
    })?;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_RELOAD_TEAMS {
            return Err(AtmError::config(format!(
                "daemon runtime reload rejected because {} contains more than {MAX_RELOAD_TEAMS} team configs",
                teams_root.display()
            ))
            .with_recovery(
                "Reduce the number of configured ATM teams or raise the documented reload cap before retrying SIGHUP.",
            ));
        }
        let entry = entry.map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read daemon team-config entry under {}",
                teams_root.display()
            ))
            .with_recovery(
                "Repair the daemon team configuration directory entries under ATM_HOME before retrying atm-daemon startup.",
            )
            .with_source(error)
        })?;
        let team_name = entry.file_name().to_string_lossy().into_owned();
        let team: TeamName = team_name.parse().map_err(|error| {
            AtmError::config(format!(
                "runtime_health: invalid team name from storage under {}: {team_name}",
                teams_root.display()
            ))
            .with_recovery(
                "Remove or rename the malformed team directory under the ATM teams root before retrying.",
            )
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
            .with_recovery(
                "Restore the daemon team config file or fix its read permissions before retrying atm-daemon startup.",
            )
            .with_source(error)
        })?;
        let config: TeamConfig = serde_json::from_slice(&raw).map_err(|error| {
            AtmError::config(format!(
                "failed to parse daemon team config {}: {error}",
                config_path.display()
            ))
            .with_recovery(
                "Repair or remove the malformed team config file and restart atm-daemon or send SIGHUP.",
            )
            .with_source(error)
        })?;
        for member in config.members {
            if next_state.members.len() >= MAX_STATUS_CACHE_ENTRIES {
                return Err(AtmError::config(format!(
                    "daemon runtime reload rejected because status-cache capacity {MAX_STATUS_CACHE_ENTRIES} would be exceeded while reading {}",
                    config_path.display()
                ))
                .with_recovery(
                    "Reduce configured roster size or increase the documented status-cache budget before retrying SIGHUP.",
                ));
            }
            let member_name = member.name;
            let membership = roster_store.query_membership(
                atm_core::boundary::RosterStoreQueryMembershipRequest {
                    team: team.clone(),
                    member: member_name.clone(),
                },
            )?;
            let key = RuntimeMemberKey {
                team: team.clone(),
                member: member_name,
            };
            let existing = current_state.and_then(|state| state.members.get(&key));
            next_state.members.insert(
                key,
                RuntimeMemberRecord {
                    pid: membership.pid,
                    state: existing
                        .map(|record| record.state)
                        .unwrap_or(RuntimeMemberState::Unknown),
                    last_active_at: existing.and_then(|record| record.last_active_at),
                },
            );
        }
    }
    Ok(next_state)
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

fn daemon_observability_finding(
    health: &atm_core::observability::AtmObservabilityHealth,
) -> DoctorFinding {
    let path = health
        .active_log_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string());
    let detail = health
        .detail
        .as_ref()
        .map(|detail| format!(" Detail: {detail}"))
        .unwrap_or_default();
    match health.logging_state {
        atm_core::observability::AtmObservabilityHealthState::Healthy => DoctorFinding {
            severity: DoctorSeverity::Info,
            code: atm_core::error_codes::AtmErrorCode::ObservabilityHealthOk,
            message: format!(
                "daemon retained observability sink is healthy at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: None,
        },
        atm_core::observability::AtmObservabilityHealthState::Degraded => DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: atm_core::error_codes::AtmErrorCode::WarningObservabilityHealthDegraded,
            message: format!(
                "daemon retained observability sink is degraded at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: Some(
                "Inspect the daemon retained log path and sink errors, then re-run `atm doctor`."
                    .to_string(),
            ),
        },
        atm_core::observability::AtmObservabilityHealthState::Unavailable => DoctorFinding {
            severity: DoctorSeverity::Error,
            code: atm_core::error_codes::AtmErrorCode::ObservabilityHealthFailed,
            message: format!(
                "daemon retained observability sink is unavailable at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: Some(
                "Restore the daemon retained-log path and confirm it is writable before re-running `atm doctor`."
                    .to_string(),
            ),
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

#[cfg(test)]
mod tests {
    use super::DaemonRequestDispatcher;
    use serial_test::serial;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    #[serial]
    fn bounded_shutdown_step_returns_after_deadline() {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let blocker = Arc::clone(&release);
        let started = Instant::now();
        DaemonRequestDispatcher::run_bounded_shutdown_step(
            "blocking_test_step",
            Duration::from_millis(10),
            move || {
                let (released, wake) = &*blocker;
                let mut released = released.lock().expect("released");
                while !*released {
                    let wait = wake
                        .wait_timeout(released, Duration::from_secs(5))
                        .expect("released wait");
                    released = wait.0;
                    assert!(!wait.1.timed_out(), "released wait timed out");
                }
                Ok(())
            },
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "bounded shutdown step should return promptly after its deadline"
        );

        let (released, wake) = &*release;
        *released.lock().expect("released") = true;
        wake.notify_all();
        DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
    }
}

#[cfg(test)]
#[path = "runtime_health_test_support.rs"]
mod runtime_health_test_support;
