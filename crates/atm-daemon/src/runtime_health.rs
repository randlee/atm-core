use std::path::PathBuf;
use std::sync::Arc;
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
    graft::{
        AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryFetchRequest, AdvisoryFetchResponse,
        AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse,
        AdvisorySessionUnregistrationRequest, AdvisorySessionUnregistrationResponse,
        AdvisoryStreamRequest,
    },
    list::list_mail,
    process::process_is_alive,
    protocol::{
        RuntimeStatusSnapshot, SendRequestEnvelope, SendResponseEnvelope,
        TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    },
    read::read_mail,
    send::send_mail,
};
use atm_rusqlite::{
    SqliteBoundaryAssembly, SqliteObservability, assemble_default_boundary_with_observability,
};

use crate::AtmHomeDir;
use crate::advisory_runtime::AdvisoryRuntime;
use crate::daemon_runtime_observability::{
    DaemonRuntimeObservability, DaemonSubsystem, SubsystemObservability,
};
#[cfg(test)]
pub(crate) use crate::runtime_status_cache::MAX_STATUS_CACHE_ENTRIES;
pub(crate) use crate::runtime_status_cache::RuntimeStatusCache;
use crate::runtime_status_cache::{build_runtime_status_cache_state, runtime_status_finding};
const SHUTDOWN_WAL_CHECKPOINT_DEADLINE: Duration = Duration::from_secs(2);
// The retained observability flush is best-effort during shutdown; Phase S records this bounded
// 2-second deadline as an accepted production exception in the anti-flake contract docs.
const SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE: Duration = Duration::from_secs(2);
const MAX_SHUTDOWN_FINALIZER_THREADS: usize = 16;

// Timed-out shutdown workers are retained in one process-wide registry instead of being dropped
// orphaned; this must be static because the bounded finalizer helper can outlive any one
// dispatcher instance after timeout, while orderly shutdown and serial tests still need one place
// to recover and join those retained workers later.
static SHUTDOWN_FINALIZER_THREADS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

pub(crate) struct DaemonRequestDispatcher {
    // Invariant: this is the validated ATM_HOME root for the running daemon,
    // not an arbitrary workspace path.
    home_dir: PathBuf,
    observability: Arc<dyn DaemonRuntimeObservability>,
    advisory_runtime_observability: SubsystemObservability,
    runtime_health_observability: SubsystemObservability,
    status_cache: RuntimeStatusCache,
    sqlite_boundary: Option<SqliteBoundaryAssembly>,
    advisory_runtime: AdvisoryRuntime,
}

impl std::fmt::Debug for DaemonRequestDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRequestDispatcher")
            .field("home_dir", &self.home_dir)
            .field("status_cache", &self.status_cache)
            .field("sqlite_boundary_present", &self.sqlite_boundary.is_some())
            .field("advisory_runtime", &"AdvisoryRuntime")
            .finish()
    }
}

impl DaemonRequestDispatcher {
    #[cfg(test)]
    pub(crate) fn drain_shutdown_finalizer_threads_for_test() {
        let mut deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let handle = with_shutdown_finalizer_registry(|handles| {
                handles
                    .iter()
                    .position(std::thread::JoinHandle::is_finished)
                    .map(|index| handles.swap_remove(index))
            });
            if let Some(handle) = handle {
                handle.join().expect("join shutdown finalizer thread");
                deadline = std::time::Instant::now() + Duration::from_secs(5);
                continue;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let has_pending = with_shutdown_finalizer_registry(|handles| !handles.is_empty());
            if !has_pending {
                break;
            }
            assert!(
                !remaining.is_zero(),
                "shutdown finalizer thread failed to join within 5s"
            );
            std::thread::park_timeout(remaining.min(Duration::from_millis(10)));
        }
        let still_pending = with_shutdown_finalizer_registry(|handles| handles.len());
        assert_eq!(
            still_pending, 0,
            "shutdown finalizer join helper left retained worker handles behind"
        );
    }

    fn spawn_shutdown_step(
        label: &'static str,
        step: impl FnOnce() -> Result<(), AtmError> + Send + 'static,
    ) -> Result<std::thread::JoinHandle<()>, AtmError> {
        std::thread::Builder::new()
            .name(format!("shutdown-finalizer-{label}"))
            .spawn(move || {
                step().unwrap_or_else(|error| {
                    tracing::warn!(
                        %error,
                        step = label,
                        "daemon shutdown finalizer step failed; restart atm-daemon and inspect the retained observability log before retrying shutdown-sensitive work"
                    );
                });
            })
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer step `{label}`"
                ))
                .with_recovery(
                    "Restart atm-daemon; the bounded shutdown finalizer helper could not be created.",
                )
                .with_source(source)
            })
    }

    fn complete_shutdown_step(label: &'static str, shutdown_handle: std::thread::JoinHandle<()>) {
        if shutdown_handle.join().is_err() {
            tracing::warn!(
                step = label,
                "daemon shutdown finalizer step panicked before reporting completion; restart atm-daemon and inspect the retained observability log for the failing shutdown step"
            );
        }
    }

    fn retain_shutdown_step(
        label: &'static str,
        shutdown_handle: std::thread::JoinHandle<()>,
        deadline: Duration,
    ) {
        let retained = with_shutdown_finalizer_registry(|handles| {
            if handles.len() < MAX_SHUTDOWN_FINALIZER_THREADS {
                handles.push(shutdown_handle);
                true
            } else {
                false
            }
        });
        if !retained {
            tracing::warn!(
                step = label,
                cap = MAX_SHUTDOWN_FINALIZER_THREADS,
                "shutdown finalizer thread cap reached; dropping retained worker handle"
            );
        }
        tracing::warn!(
            step = label,
            timeout_ms = deadline.as_millis(),
            "daemon shutdown finalizer step exceeded its deadline; worker retained for later join"
        );
    }

    fn run_bounded_shutdown_step(
        label: &'static str,
        deadline: Duration,
        step: impl FnOnce() -> Result<(), AtmError> + Send + 'static,
    ) {
        let shutdown_handle = match Self::spawn_shutdown_step(label, step) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::warn!(
                    %error,
                    step = label,
                    "daemon shutdown finalizer step could not start; restart atm-daemon because shutdown cleanup could not be scheduled"
                );
                return;
            }
        };
        let started = std::time::Instant::now();
        loop {
            if shutdown_handle.is_finished() {
                Self::complete_shutdown_step(label, shutdown_handle);
                return;
            }
            if started.elapsed() >= deadline {
                // SQLite WAL checkpoint timeout is the highest-risk caller here: a checkpoint can
                // outlive the bounded shutdown window, but retaining the JoinHandle is still safer
                // than dropping it orphaned because tests and orderly process teardown can join it
                // later once the blocking storage step finishes.
                Self::retain_shutdown_step(label, shutdown_handle, deadline);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(crate) fn new(
        // Must be the validated ATM home dir for this daemon runtime.
        home_dir: AtmHomeDir,
        status_cache: RuntimeStatusCache,
        observability: Arc<dyn DaemonRuntimeObservability>,
        sqlite_observability: Arc<dyn SqliteObservability>,
    ) -> Self {
        let home_dir = home_dir.into_inner();
        let advisory_runtime_observability = SubsystemObservability::new(
            DaemonSubsystem::AdvisoryRuntime,
            Arc::clone(&observability),
        );
        let runtime_health_observability =
            SubsystemObservability::new(DaemonSubsystem::RuntimeHealth, Arc::clone(&observability));
        let sqlite_boundary = match assemble_default_boundary_with_observability(
            sqlite_observability,
        ) {
            Ok(boundary) => {
                if let Err(error) = build_runtime_status_cache_state(None, boundary.roster_store())
                    .and_then(|state| status_cache.replace_state(state))
                {
                    tracing::warn!(%error, "failed to hydrate runtime status cache from sqlite roster state");
                    runtime_health_observability.emit_or_warn(
                        "sqlite_cache_hydration",
                        "degraded",
                        "failed to hydrate runtime status cache from sqlite roster state",
                    );
                    status_cache.mark_sqlite_unavailable();
                }
                Some(boundary)
            }
            Err(error) => {
                tracing::warn!(%error, "failed to assemble default sqlite boundary for daemon runtime health");
                runtime_health_observability.emit_or_warn(
                    "sqlite_boundary_assembly",
                    "failed",
                    "failed to assemble sqlite boundary for daemon runtime health",
                );
                status_cache.mark_sqlite_unavailable();
                None
            }
        };
        Self {
            home_dir: home_dir.clone(),
            observability: Arc::clone(&observability),
            advisory_runtime_observability: advisory_runtime_observability.clone(),
            runtime_health_observability: runtime_health_observability.clone(),
            status_cache,
            sqlite_boundary,
            advisory_runtime: AdvisoryRuntime::new_with_observability(
                advisory_runtime_observability,
            ),
        }
    }
}

fn with_shutdown_finalizer_registry<R>(
    f: impl FnOnce(&mut Vec<std::thread::JoinHandle<()>>) -> R,
) -> R {
    match SHUTDOWN_FINALIZER_THREADS.lock() {
        Ok(mut handles) => f(&mut handles),
        Err(poisoned) => {
            tracing::warn!(
                "shutdown finalizer thread registry lock poisoned; recovering retained worker handles"
            );
            // The registry only owns JoinHandles for timed-out shutdown helpers; recovering the
            // inner vector preserves later joins instead of dropping retained worker ownership.
            let mut handles = poisoned.into_inner();
            f(&mut handles)
        }
    }
}

impl boundary::sealed::Sealed for DaemonRequestDispatcher {}

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                let outcome = send_mail(request, self.observability.as_ref())?;
                if let Err(error) = self.advisory_runtime.enqueue_nudge_for_recipient(&outcome) {
                    self.advisory_runtime_observability.emit_or_warn(
                        "advisory_enqueue",
                        "degraded",
                        "advisory queue overflowed",
                    );
                    return Err(error);
                }
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)))
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
            RequestEnvelope::AdvisoryRegister(request) => Ok(ResponseEnvelope::AdvisoryRegister(
                self.register_advisory_session(request)?,
            )),
            RequestEnvelope::AdvisoryUnregister(request) => Ok(ResponseEnvelope::AdvisoryUnregister(
                self.unregister_advisory_session(request)?,
            )),
            RequestEnvelope::AdvisoryFetch(request) => Ok(ResponseEnvelope::AdvisoryFetch(
                self.fetch_advisory_events(request)?,
            )),
            RequestEnvelope::AdvisoryDrain(request) => Ok(ResponseEnvelope::AdvisoryDrain(
                self.drain_advisory_events(request)?,
            )),
            RequestEnvelope::AdvisoryStream(_) => Err(AtmError::validation(
                "advisory stream requests require the same-host streaming transport path",
            )
            .with_recovery(
                "Open the dedicated same-host advisory stream connection instead of routing advisory delivery through unary request dispatch.",
            )),
        }
    }

    fn dispatch_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
        sink: &mut dyn boundary::AdvisoryStreamSink,
    ) -> Result<(), AtmError> {
        self.advisory_runtime.stream_nudges(request, sink)
    }
}

impl DaemonRequestDispatcher {
    pub(crate) fn reload_runtime_view(&self) -> Result<(), AtmError> {
        let roster_store = self
            .sqlite_boundary
            .as_ref()
            .map(SqliteBoundaryAssembly::roster_store)
            .ok_or_else(|| {
                self.runtime_health_observability.emit_or_warn(
                    "reload_unavailable",
                    "failed",
                    "sqlite-backed daemon runtime reload is unavailable because the sqlite boundary is not assembled",
                );
                self.status_cache.mark_sqlite_unavailable();
                AtmError::daemon_unavailable(
                    "sqlite-backed daemon runtime reload is unavailable because the sqlite boundary is not assembled",
                )
                .with_recovery(
                    "Restore the host-scoped ATM SQLite durable-state database and restart atm-daemon before retrying SIGHUP reload.",
                )
            })?;
        let current_state = self.status_cache.clone_state()?;
        let next_state = build_runtime_status_cache_state(Some(&current_state), roster_store)?;
        let reloaded_members = next_state.member_count();
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
                self.runtime_health_observability.emit_or_warn(
                    "heartbeat_unavailable",
                    "failed",
                    "sqlite-backed roster truth is unavailable for daemon heartbeats",
                );
                self.status_cache.mark_sqlite_unavailable();
                AtmError::daemon_unavailable(
                    "sqlite-backed roster truth is unavailable for daemon heartbeats",
                )
                .with_recovery(
                    "Restore the host-scoped ATM SQLite database and restart atm-daemon before retrying heartbeat traffic.",
                )
            })?;
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
        let cached_pid = self
            .status_cache
            .cached_pid(&request.team, &request.member)?;
        if let Some(existing_pid) = cached_pid.filter(|pid| *pid != request.pid)
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
        self.status_cache
            .record_heartbeat(&request, cached_pid.is_some_and(|pid| pid != request.pid))
    }

    fn register_advisory_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        self.advisory_runtime.register_session(request)
    }

    fn unregister_advisory_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        self.advisory_runtime.unregister_session(request)
    }

    fn fetch_advisory_events(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        self.advisory_runtime.fetch_nudges(request)
    }

    fn drain_advisory_events(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        self.advisory_runtime.drain_nudges(request)
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
    use super::{
        DaemonRequestDispatcher, MAX_SHUTDOWN_FINALIZER_THREADS, SHUTDOWN_FINALIZER_THREADS,
    };
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    struct ShutdownFinalizerDrainGuard;

    impl Drop for ShutdownFinalizerDrainGuard {
        fn drop(&mut self) {
            DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
        }
    }

    #[test]
    #[serial_test::serial(env)]
    fn bounded_shutdown_step_returns_after_deadline() {
        let _drain_guard = ShutdownFinalizerDrainGuard;
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
            started.elapsed() < Duration::from_secs(5),
            "bounded shutdown step should return promptly after its deadline"
        );

        let (released, wake) = &*release;
        *released.lock().expect("released") = true;
        wake.notify_all();
    }

    #[test]
    #[serial_test::serial(env)]
    fn bounded_shutdown_step_does_not_exceed_retained_finalizer_cap() {
        let _drain_guard = ShutdownFinalizerDrainGuard;
        DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();

        let retained_release = Arc::new((Mutex::new(false), Condvar::new()));
        {
            let mut handles = SHUTDOWN_FINALIZER_THREADS
                .lock()
                .expect("shutdown finalizer thread registry lock");
            for _ in 0..MAX_SHUTDOWN_FINALIZER_THREADS {
                let retained_release = Arc::clone(&retained_release);
                handles.push(std::thread::spawn(move || {
                    let (released, wake) = &*retained_release;
                    let mut released = released.lock().expect("retained release");
                    while !*released {
                        let wait = wake
                            .wait_timeout(released, Duration::from_secs(5))
                            .expect("retained release wait");
                        released = wait.0;
                        assert!(!wait.1.timed_out(), "retained release wait timed out");
                    }
                }));
            }
        }

        let overflow_release = Arc::new((Mutex::new(false), Condvar::new()));
        let blocker = Arc::clone(&overflow_release);
        DaemonRequestDispatcher::run_bounded_shutdown_step(
            "blocking_cap_test_step",
            Duration::from_millis(10),
            move || {
                let (released, wake) = &*blocker;
                let mut released = released.lock().expect("overflow release");
                while !*released {
                    let wait = wake
                        .wait_timeout(released, Duration::from_secs(5))
                        .expect("overflow release wait");
                    released = wait.0;
                    assert!(!wait.1.timed_out(), "overflow release wait timed out");
                }
                Ok(())
            },
        );

        assert_eq!(
            SHUTDOWN_FINALIZER_THREADS
                .lock()
                .expect("shutdown finalizer thread registry lock")
                .len(),
            MAX_SHUTDOWN_FINALIZER_THREADS,
            "cap-exceeded path should not retain more than the documented shutdown finalizer thread budget"
        );

        let (released, wake) = &*overflow_release;
        *released.lock().expect("overflow release") = true;
        wake.notify_all();
        let (released, wake) = &*retained_release;
        *released.lock().expect("retained release") = true;
        wake.notify_all();
    }

    #[test]
    fn heartbeat_only_insert_evicts_oldest_member_when_cache_is_full() {
        use atm_core::protocol::{
            HeartbeatActivity, RuntimeMemberState, TeamMemberHeartbeatRequest,
        };
        use atm_core::types::{AgentName, IsoTimestamp, TeamName};
        use chrono::{Duration as ChronoDuration, Utc};

        let status_cache = super::RuntimeStatusCache::new();
        let team: TeamName = "test-team".parse().expect("team");
        let oldest_member: AgentName = "heartbeat-oldest".parse().expect("member");
        let trigger_member: AgentName = "heartbeat-trigger".parse().expect("member");
        let base = Utc::now();

        for index in 0..super::MAX_STATUS_CACHE_ENTRIES {
            let member_name: AgentName = if index == 0 {
                oldest_member.clone()
            } else {
                format!("heartbeat-{index}").parse().expect("member")
            };
            status_cache
                .record_heartbeat_for_test(
                    &TeamMemberHeartbeatRequest {
                        team: team.clone(),
                        member: member_name,
                        pid: index as u32 + 1,
                        observed_at: IsoTimestamp::from_datetime(
                            base + ChronoDuration::seconds(index as i64),
                        ),
                        activity: HeartbeatActivity::Idle,
                    },
                    false,
                )
                .expect("seed heartbeat member");
        }

        let response = status_cache
            .record_heartbeat_for_test(
                &TeamMemberHeartbeatRequest {
                    team: team.clone(),
                    member: trigger_member.clone(),
                    pid: std::process::id(),
                    observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(1)),
                    activity: HeartbeatActivity::ActiveToolUse,
                },
                false,
            )
            .expect("trigger heartbeat");
        assert_eq!(response.state, RuntimeMemberState::Active);

        assert_eq!(
            status_cache.member_count_for_test().expect("member count"),
            super::MAX_STATUS_CACHE_ENTRIES
        );
        assert_eq!(
            status_cache
                .member_state_for_test(&team, &oldest_member)
                .expect("oldest member state"),
            None
        );
        assert_eq!(
            status_cache
                .member_state_for_test(&team, &trigger_member)
                .expect("trigger member state"),
            Some(RuntimeMemberState::Active)
        );
    }
}

#[cfg(test)]
#[path = "runtime_health_test_support.rs"]
mod runtime_health_test_support;
