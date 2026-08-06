use crate::AtmHomeDir;
use crate::daemon_runtime_observability::{
    DaemonRuntimeObservability, DaemonSubsystem, SubsystemObservability,
};
use crate::https_transport::{HttpsMessageTransport, SharedHttpsTransport};
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryProjection};
use atm_core::{
    ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, LocalServiceRuntime, RequestDeadline,
    RequestEnvelope, ResponseEnvelope, boundary,
    clear::clear_mail_with_runtime,
    doctor::{self, DaemonRuntimeDoctorReport, DoctorExecutionContext, DoctorQuery, DoctorReport},
    error::{AtmError, AtmErrorCode},
    list::list_mail,
    protocol::{
        CompatibilityVerdict, PeerSyncDisposition, PeerSyncOutcome, PeerSyncRequest,
        ReleaseVersion, RuntimeLivenessState, RuntimeStatusSnapshot, SendResponseEnvelope,
        TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    },
    provenance::{WriteIngress, WriteProvenance, validate_write_provenance},
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    send::{PreparedWrite, WriteOutcome, WriteRequest},
};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;
mod admission_view;
use admission_view::AdmissionRuntimeView;
pub(crate) mod dispatch;
pub(crate) use dispatch::{MessageRecord, PostWriteRouter};
mod doctor_reporting;
mod post_commit_work;
use crate::peer_drain_coordinator::{
    PEER_SYNC_REQUEST_DEADLINE, PeerDeliveryCoordinator, PeerDrainCoordinator,
    PeerSyncOutcome as DrainPeerSyncOutcome,
};
use post_commit_work::{PeerPostCommitWorkQueue, PostCommitWorkKey, PostCommitWorkQueue};
pub(crate) mod peer_authority;
#[cfg(test)]
pub(crate) use crate::runtime_status_cache::MAX_STATUS_CACHE_ENTRIES;
pub(crate) use crate::runtime_status_cache::RuntimeStatusCache;
use crate::runtime_status_cache::{build_runtime_status_cache_state, runtime_status_finding};
use atm_runtime::RuntimeAssembly;
use atm_storage::PeerConfigStore;
use atm_storage::RosterStore;
use doctor_reporting::{daemon_observability_finding, finalize_doctor_report};
mod peer_delivery_router;
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
fn lock_runtime_mutex<'a, T>(
    mutex: &'a Mutex<T>,
    resource: &'static str,
) -> Result<MutexGuard<'a, T>, AtmError> {
    mutex.lock().map_err(|_| {
        AtmError::daemon_unavailable(format!(
            "{resource} lock poisoned; restart atm-daemon to restore runtime coordination"
        ))
    })
}
pub(crate) struct DaemonRequestDispatcher {
    // Invariant: this is the validated ATM_HOME root for the running daemon,
    // not an arbitrary workspace path.
    home_dir: AtmHomeDir,
    observability: Arc<dyn DaemonRuntimeObservability>,
    runtime_health_observability: SubsystemObservability,
    status_cache: RuntimeStatusCache,
    service_runtime: LocalServiceRuntime,
    admission_runtime_view: AdmissionRuntimeView,
    doctor_ports: atm_core::doctor::RuntimeDoctorPorts,
    roster_store: Option<Arc<dyn RosterStore + Send + Sync>>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    https_transport: SharedHttpsTransport,
    peer_delivery_coordinator: Arc<dyn PeerDeliveryCoordinator>,
    post_commit_work_queue: Arc<dyn PostCommitWorkQueue>,
    runtime_reload_hook: std::sync::Mutex<Option<RuntimeReloadHook>>,
    peer_delivery_projection: Arc<PeerDeliveryProjection>,
}

type RuntimeReloadHook = Arc<dyn Fn() -> Result<(), AtmError> + Send + Sync>;

fn build_peer_delivery_coordinator(
    peer_config_store: &Arc<dyn PeerConfigStore + Send + Sync>,
    outbound_message_query: &Arc<dyn atm_storage::OutboundMessageQuery + Send + Sync>,
    https_transport: &SharedHttpsTransport,
    projection: &Arc<PeerDeliveryProjection>,
    observability: &SubsystemObservability,
) -> Arc<dyn PeerDeliveryCoordinator> {
    let coordinator_projection = Arc::clone(projection);
    let coordinator_observability = observability.clone();
    Arc::new(PeerDrainCoordinator::new(
        Arc::clone(peer_config_store),
        Arc::clone(outbound_message_query),
        Arc::clone(https_transport),
        Arc::new(move |event| coordinator_projection.record(event, &coordinator_observability)),
    ))
}

impl std::fmt::Debug for DaemonRequestDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRequestDispatcher")
            .field("home_dir", &self.home_dir.as_path())
            .field("status_cache", &self.status_cache)
            .field("service_runtime", &self.service_runtime)
            .field("doctor_ports", &self.doctor_ports)
            .field("roster_store_present", &self.roster_store.is_some())
            .field("peer_config_store", &"dyn PeerConfigStore")
            .field("https_transport", &"dyn HttpsMessageTransport")
            .field("runtime_reload_hook", &"runtime reload callback")
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
                        subsystem = "runtime_health",
                        action = "shutdown_finalizer_step",
                        outcome = "failed",
                        %error,
                        step = label,
                        "daemon shutdown finalizer step failed; restart atm-daemon and inspect the retained observability log before retrying shutdown-sensitive work"
                    );
                });
            })
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer step `{label}`: {source}"
                ))
            })
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
                subsystem = "runtime_health",
                action = "shutdown_retain",
                outcome = "capacity_exceeded",
                step = label,
                cap = MAX_SHUTDOWN_FINALIZER_THREADS,
                "shutdown finalizer thread cap reached; dropping retained worker handle"
            );
        } else {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_retain",
                outcome = "deadline_exceeded",
                step = label,
                timeout_ms = deadline.as_millis(),
                "daemon shutdown finalizer step exceeded its deadline; worker retained for later join"
            );
        }
    }

    fn spawn_shutdown_join_helper(
        label: &'static str,
        shutdown_handle: std::thread::JoinHandle<()>,
    ) -> Result<
        (
            std::sync::mpsc::Receiver<std::thread::Result<()>>,
            std::thread::JoinHandle<()>,
        ),
        AtmError,
    > {
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let join_helper = std::thread::Builder::new()
            .name(format!("shutdown-finalizer-join-{label}"))
            .spawn(move || {
                let _ = result_tx.send(shutdown_handle.join());
            })
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer join helper `{label}`: {source}"
                ))
            })?;
        Ok((result_rx, join_helper))
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
                    subsystem = "runtime_health",
                    action = "shutdown_spawn",
                    outcome = "failed",
                    %error,
                    step = label,
                    "daemon shutdown finalizer step could not start; restart atm-daemon because shutdown cleanup could not be scheduled"
                );
                return;
            }
        };
        let (result_rx, join_helper) = match Self::spawn_shutdown_join_helper(
            label,
            shutdown_handle,
        ) {
            Ok(helper) => helper,
            Err(error) => {
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "shutdown_join_helper_spawn",
                    outcome = "failed",
                    %error,
                    step = label,
                    "daemon shutdown finalizer step could not start its bounded join helper; restart atm-daemon because shutdown cleanup could not be monitored"
                );
                return;
            }
        };
        Self::handle_bounded_shutdown_result(label, deadline, result_rx, join_helper);
    }

    fn handle_bounded_shutdown_result(
        label: &'static str,
        deadline: Duration,
        result_rx: std::sync::mpsc::Receiver<std::thread::Result<()>>,
        join_helper: JoinHandle<()>,
    ) {
        match result_rx.recv_timeout(deadline) {
            Ok(join_result) => {
                Self::finalize_bounded_shutdown_result(label, join_result, join_helper)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // SQLite WAL checkpoint timeout is the highest-risk caller here: a checkpoint can
                // outlive the bounded shutdown window, but retaining the worker JoinHandle is still
                // safer than dropping it orphaned because tests and orderly process teardown can
                // join it later once the blocking storage step finishes.
                Self::retain_shutdown_step(label, join_helper, deadline);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Self::report_bounded_shutdown_disconnect(label, join_helper);
            }
        }
    }

    fn finalize_bounded_shutdown_result(
        label: &'static str,
        join_result: std::thread::Result<()>,
        join_helper: JoinHandle<()>,
    ) {
        if join_helper.join().is_err() {
            Self::warn_shutdown_join_helper(label, "panic");
        }
        if join_result.is_err() {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_finalize",
                outcome = "panic",
                step = label,
                "daemon shutdown finalizer step panicked before reporting completion; restart atm-daemon and inspect the retained observability log for the failing shutdown step"
            );
        }
    }

    fn report_bounded_shutdown_disconnect(label: &'static str, join_helper: JoinHandle<()>) {
        if join_helper.join().is_err() {
            Self::warn_shutdown_join_helper(label, "panic");
        } else {
            Self::warn_shutdown_join_helper(label, "disconnected");
        }
    }

    fn warn_shutdown_join_helper(label: &'static str, outcome: &'static str) {
        tracing::warn!(
            subsystem = "runtime_health",
            action = "shutdown_join_helper",
            outcome,
            step = label,
            "daemon shutdown finalizer join helper failed before reporting completion"
        );
    }

    pub(crate) fn new(
        // Must be the validated ATM home dir for this daemon runtime.
        home_dir: AtmHomeDir,
        status_cache: RuntimeStatusCache,
        observability: Arc<dyn DaemonRuntimeObservability>,
        runtime_assembly: RuntimeAssembly,
    ) -> Self {
        let runtime_health_observability =
            SubsystemObservability::new(DaemonSubsystem::RuntimeHealth, Arc::clone(&observability));
        let roster_store = runtime_assembly.shared_roster_store_arc();
        let peer_config_store = runtime_assembly.peer_config_store();
        let outbound_message_query = runtime_assembly.outbound_message_query();
        match build_runtime_status_cache_state(None, roster_store.as_ref()) {
            Ok(state) => status_cache.publish_state(state),
            Err(error) => {
                status_cache.mark_degraded_ingest();
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "sqlite_cache_hydration",
                    outcome = "degraded",
                    %error,
                    "failed to hydrate runtime status cache from runtime-bound roster state"
                );
                runtime_health_observability.emit_or_warn(
                    "sqlite_cache_hydration",
                    "degraded",
                    "failed to hydrate runtime status cache from runtime-bound roster state",
                );
            }
        }
        let https_transport: SharedHttpsTransport = Arc::new(Mutex::new(None));
        let peer_delivery_projection = Arc::new(PeerDeliveryProjection::default());
        let peer_delivery_coordinator = build_peer_delivery_coordinator(
            &peer_config_store,
            &outbound_message_query,
            &https_transport,
            &peer_delivery_projection,
            &runtime_health_observability,
        );
        let service_runtime = runtime_assembly.service_runtime;
        let post_commit_work_queue: Arc<dyn PostCommitWorkQueue> = Arc::new(
            PeerPostCommitWorkQueue::new(Arc::clone(&peer_delivery_coordinator)),
        );
        let admission_runtime_view = AdmissionRuntimeView::new(service_runtime.clone());
        Self {
            home_dir,
            observability: Arc::clone(&observability),
            runtime_health_observability: runtime_health_observability.clone(),
            status_cache,
            service_runtime,
            admission_runtime_view,
            doctor_ports: runtime_assembly.doctor_ports,
            roster_store: Some(roster_store),
            peer_config_store,
            https_transport,
            peer_delivery_coordinator,
            post_commit_work_queue,
            runtime_reload_hook: std::sync::Mutex::new(None),
            peer_delivery_projection,
        }
    }

    pub(crate) fn install_https_transport(
        &self,
        transport: Arc<dyn HttpsMessageTransport>,
    ) -> Result<(), AtmError> {
        let mut slot = lock_runtime_mutex(&self.https_transport, "HTTPS peer transport slot")?;
        *slot = Some(transport);
        Ok(())
    }

    pub(crate) fn clear_https_transport(&self) -> Result<(), AtmError> {
        let mut slot = lock_runtime_mutex(&self.https_transport, "HTTPS peer transport slot")?;
        *slot = None;
        Ok(())
    }

    pub(crate) fn start_peer_drain_coordinator(&self) -> Result<(), AtmError> {
        self.peer_delivery_coordinator.start()
    }

    pub(crate) fn stop_peer_drain_coordinator(&self) -> Result<(), AtmError> {
        self.peer_delivery_coordinator.stop()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn home_dir_for_test(&self) -> &std::path::Path {
        self.home_dir.as_path()
    }
}

fn with_shutdown_finalizer_registry<R>(
    f: impl FnOnce(&mut Vec<std::thread::JoinHandle<()>>) -> R,
) -> R {
    match SHUTDOWN_FINALIZER_THREADS.lock() {
        Ok(mut handles) => f(&mut handles),
        Err(poisoned) => {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_registry_lock",
                outcome = "poison_recovered",
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
    // The boundary contract is fallible even though the in-memory snapshot read
    // is currently infallible; keeping the `Result` preserves the shared
    // status-source seam for implementations that may need to surface IO or
    // transport failures.
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        let snapshot = self.status_cache.snapshot();
        if matches!(snapshot.liveness, RuntimeLivenessState::Unavailable)
            && snapshot.singleton_owner_pid.is_none()
        {
            return Err(AtmError::daemon_unavailable(
                "daemon runtime status snapshot is unavailable because no owner process is recorded",
            ));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
impl DaemonRequestDispatcher {
    pub(crate) fn new_for_test(
        home_dir: std::path::PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: std::path::PathBuf,
    ) -> Self {
        let observability = std::sync::Arc::new(
            crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )
            .expect("daemon test observability"),
        );
        let runtime_observability: std::sync::Arc<dyn crate::DaemonRuntimeObservability> =
            observability.clone();
        let runtime_assembly =
            crate::test_support::sqlite_runtime_assembly_for_test(&roster_db_path)
                .expect("assemble sqlite runtime for daemon dispatcher test");
        match build_runtime_status_cache_state(
            None,
            runtime_assembly.shared_roster_store_arc().as_ref(),
        ) {
            Ok(state) => status_cache.publish_state(state),
            Err(error) => {
                status_cache.mark_degraded_ingest();
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "sqlite_cache_hydration",
                    outcome = "degraded",
                    %error,
                    "failed to hydrate test runtime status cache from runtime-bound roster state"
                );
            }
        }
        let runtime_health_observability = crate::SubsystemObservability::new(
            crate::DaemonSubsystem::RuntimeHealth,
            std::sync::Arc::clone(&runtime_observability),
        );
        let peer_config_store = runtime_assembly.peer_config_store();
        let outbound_message_query = runtime_assembly.outbound_message_query();
        let https_transport: SharedHttpsTransport = Arc::new(Mutex::new(None));
        let peer_delivery_projection = Arc::new(PeerDeliveryProjection::default());
        let peer_delivery_coordinator = build_peer_delivery_coordinator(
            &peer_config_store,
            &outbound_message_query,
            &https_transport,
            &peer_delivery_projection,
            &runtime_health_observability,
        );
        let service_runtime = runtime_assembly.service_runtime.clone();
        let daemon_home = crate::AtmHomeDir::from_path_for_test(home_dir.clone());
        let post_commit_work_queue: Arc<dyn PostCommitWorkQueue> = Arc::new(
            PeerPostCommitWorkQueue::new(Arc::clone(&peer_delivery_coordinator)),
        );
        Self {
            home_dir: daemon_home,
            observability: runtime_observability,
            runtime_health_observability,
            status_cache,
            service_runtime: service_runtime.clone(),
            admission_runtime_view: AdmissionRuntimeView::new(service_runtime),
            doctor_ports: runtime_assembly.doctor_ports.clone(),
            roster_store: Some(runtime_assembly.shared_roster_store_arc()),
            peer_config_store,
            https_transport,
            peer_delivery_coordinator,
            post_commit_work_queue,
            runtime_reload_hook: std::sync::Mutex::new(None),
            peer_delivery_projection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonRequestDispatcher, MAX_SHUTDOWN_FINALIZER_THREADS, SHUTDOWN_FINALIZER_THREADS,
    };
    use atm_core::api::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline};
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn authenticated_local_runtime_reload_runs_the_installed_trust_refresh_hook() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let dispatcher = DaemonRequestDispatcher::new_for_test(
            tempdir.path().join("home"),
            super::RuntimeStatusCache::default(),
            tempdir.path().join("runtime.db"),
        );
        let refreshed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let refresh_flag = Arc::clone(&refreshed);
        dispatcher
            .install_runtime_reload_hook(Arc::new(move || {
                refresh_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }))
            .expect("install reload hook");

        let response = dispatcher
            .route(
                ApiRequest::new(RequestEnvelope::ReloadRuntimeView),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .expect("authenticated local reload routes")
            .into_inner();
        assert!(matches!(response, ResponseEnvelope::RuntimeViewReloaded));
        assert!(refreshed.load(std::sync::atomic::Ordering::SeqCst));

        let error = dispatcher
            .route(
                ApiRequest::new(RequestEnvelope::ReloadRuntimeView),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .expect_err("peer ingress must not control daemon reload");
        assert!(error.is_validation());
    }

    #[test]
    fn expired_router_deadline_prevents_downstream_dispatch() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let dispatcher = DaemonRequestDispatcher::new_for_test(
            tempdir.path().join("home"),
            super::RuntimeStatusCache::default(),
            tempdir.path().join("runtime.db"),
        );

        let error = dispatcher
            .dispatch_with_deadline(
                RequestEnvelope::Doctor(Default::default()),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::ZERO),
            )
            .expect_err("an expired ingress deadline must prevent dispatcher work");

        assert!(error.is_daemon_unavailable());
        assert!(error.message().contains("before dispatch work started"));
    }

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
            status_cache.record_heartbeat_for_test(
                &TeamMemberHeartbeatRequest {
                    team: team.clone(),
                    member: member_name,
                    pid: index as u32 + 1,
                    observed_at: IsoTimestamp::from_datetime(
                        base + ChronoDuration::seconds(index as i64),
                    ),
                    activity: HeartbeatActivity::Idle,
                    session_id: None,
                },
                false,
            );
        }

        let response = status_cache.record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: trigger_member.clone(),
                pid: std::process::id(),
                observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(1)),
                activity: HeartbeatActivity::ActiveToolUse,
                session_id: None,
            },
            false,
        );
        assert_eq!(response.state, RuntimeMemberState::Active);

        assert_eq!(
            status_cache.member_count_for_test(),
            super::MAX_STATUS_CACHE_ENTRIES
        );
        assert_eq!(
            status_cache.member_state_for_test(&team, &oldest_member),
            None
        );
        assert_eq!(
            status_cache.member_state_for_test(&team, &trigger_member),
            Some(RuntimeMemberState::Active)
        );
    }
}
