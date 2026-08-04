use crate::daemon_runtime_observability::{DaemonRuntimeObservability, SubsystemObservability};
use crate::host_ownership::HostOwnershipAdapter;
#[cfg(not(windows))]
use crate::local_ipc_transport::{PreparedRuntimeServer, RuntimeServeHooks, SocketEndpointGuard};
#[cfg(windows)]
use crate::local_tcp_transport::{PreparedRuntimeServer, RuntimeServeHooks, SocketEndpointGuard};
use crate::non_claude_outbound_runtime::DaemonNonClaudeOutbound;
use crate::runtime_health::DaemonRequestDispatcher;
use crate::runtime_health::{DaemonStatusSource, RuntimeStatusCache};
#[cfg(test)]
use crate::worker_support::retain_join_helper;
use crate::{AtmHomeDir, DaemonSubsystem, LocalIpcServerTransportAdapter};
use atm_core::ApiRouter;
use atm_core::error::AtmError;
use atm_daemon_bootstrap::assemble_host_runtime;
use atm_runtime::RuntimeAssembly;
use std::fs::OpenOptions;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(test)]
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RuntimeLifecycleState {
    Starting,
    Running,
    Draining,
    #[default]
    Stopped,
}

/// Serializes legal daemon runtime ownership transitions.
#[derive(Debug, Default)]
pub(crate) struct RuntimeLifecycle {
    /// A single mutex is sufficient here because lifecycle transitions are
    /// serialized control-plane events, not a high-frequency data path.
    state: Mutex<RuntimeLifecycleState>,
}

impl RuntimeLifecycle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn state(&self) -> RuntimeLifecycleState {
        *self.state.lock().expect("runtime lifecycle state lock")
    }

    /// Transition the daemon runtime lifecycle to `next`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] with
    /// [`atm_core::error_codes::AtmErrorCode::Validation`] when `next` would
    /// violate the documented state machine, or
    /// [`atm_core::error_codes::AtmErrorCode::DaemonUnavailable`] when the
    /// lifecycle lock is poisoned.
    pub(crate) fn transition(
        &self,
        next: RuntimeLifecycleState,
    ) -> Result<RuntimeLifecycleState, AtmError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime lifecycle state lock poisoned"))?;
        let current = *state;
        if !matches!(
            (current, next),
            (
                RuntimeLifecycleState::Stopped,
                RuntimeLifecycleState::Starting
            ) | (
                RuntimeLifecycleState::Starting,
                RuntimeLifecycleState::Running
            ) | (
                RuntimeLifecycleState::Starting,
                RuntimeLifecycleState::Stopped
            ) | (
                RuntimeLifecycleState::Running,
                RuntimeLifecycleState::Draining
            ) | (
                RuntimeLifecycleState::Draining,
                RuntimeLifecycleState::Stopped
            )
        ) {
            return Err(AtmError::validation(format!(
                "illegal daemon runtime lifecycle transition: {current:?} -> {next:?}"
            )));
        }
        *state = next;
        Ok(next)
    }

    /// Force the daemon runtime lifecycle back to `Stopped`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] with
    /// [`atm_core::error_codes::AtmErrorCode::DaemonUnavailable`] when the
    /// lifecycle lock is poisoned while resetting the runtime state.
    pub(crate) fn force_stopped(&self) -> Result<(), AtmError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime lifecycle state lock poisoned"))?;
        *state = RuntimeLifecycleState::Stopped;
        Ok(())
    }
}

/// Internal root for Phase R daemon runtime wiring.
pub(crate) struct RuntimeComposition {
    lifecycle: Arc<RuntimeLifecycle>,
    // Holding the ownership adapter in the composition keeps host-runtime ownership tied to the
    // full daemon runtime lifetime even though the field is not read after construction.
    _host_ownership_adapter: HostOwnershipAdapter,
    // Endpoint cleanup ownership moves between startup, serve, and teardown transitions, so this
    // mutex protects exclusive handoff/drop of the guard rather than a simple ready flag.
    endpoint_guard: Mutex<Option<SocketEndpointGuard>>,
    server_transport: LocalIpcServerTransportAdapter,
    request_dispatcher: Arc<DaemonRequestDispatcher>,
    composition_observability: SubsystemObservability,
    _production_runtime: atm_core::LocalServiceRuntime,
    _status_source: DaemonStatusSource,
}

impl std::fmt::Debug for RuntimeComposition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeComposition")
            .field("lifecycle", &self.lifecycle)
            .field("server_transport", &self.server_transport)
            .field("request_dispatcher", &self.request_dispatcher)
            .finish_non_exhaustive()
    }
}

impl RuntimeComposition {
    #[cfg(test)]
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn new(home_dir: PathBuf) -> Result<Self, AtmError> {
        Self::new_with_runtime_db_path(
            AtmHomeDir::from_path_for_test(home_dir.clone()),
            atm_core::home::host_mail_db_path_from_home(&home_dir),
            Arc::new(crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )?),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_runtime_db_path(
        home_dir: AtmHomeDir,
        runtime_db_path: PathBuf,
        observability: Arc<dyn DaemonRuntimeObservability>,
    ) -> Result<Self, AtmError> {
        let composition_observability =
            SubsystemObservability::new(DaemonSubsystem::Composition, Arc::clone(&observability));
        let runtime_assembly =
            crate::test_support::sqlite_runtime_assembly_for_test(&runtime_db_path)
                .map_err(|error| runtime_assembly_failed(error, &composition_observability))?;
        Self::new_with_runtime_assembly(home_dir, observability, runtime_assembly)
    }

    fn new_with_runtime_assembly(
        home_dir: AtmHomeDir,
        observability: Arc<dyn DaemonRuntimeObservability>,
        runtime_assembly: RuntimeAssembly,
    ) -> Result<Self, AtmError> {
        let runtime_assembly = runtime_assembly.for_daemon();
        let composition_observability =
            SubsystemObservability::new(DaemonSubsystem::Composition, Arc::clone(&observability));
        // Runtime status snapshots are read on the hot doctor/status path, so
        // the cache uses ArcSwap for lock-free reads while writes stay explicit.
        let status_cache = RuntimeStatusCache::new_with_observability(SubsystemObservability::new(
            DaemonSubsystem::RuntimeStatusCache,
            Arc::clone(&observability),
        ));
        atm_core::runtime_install_hooks::install_retained_runtime_instance_for_daemon(
            runtime_assembly.service_runtime.clone(),
        );
        let server_transport = build_server_transport(&observability);
        let request_dispatcher = build_request_dispatcher(
            home_dir,
            &status_cache,
            &observability,
            runtime_assembly.clone(),
        )?;
        let host_ownership_adapter = build_host_ownership_adapter(&observability);
        Ok(Self {
            lifecycle: Arc::new(RuntimeLifecycle::new()),
            _host_ownership_adapter: host_ownership_adapter,
            endpoint_guard: Mutex::new(None),
            server_transport,
            request_dispatcher,
            composition_observability,
            _production_runtime: runtime_assembly.service_runtime,
            _status_source: DaemonStatusSource::new(status_cache),
        })
    }

    fn request_dispatcher(&self) -> Arc<dyn ApiRouter + Send + Sync> {
        self.request_dispatcher.clone()
    }

    fn replace_endpoint_guard(&self, guard: Option<SocketEndpointGuard>) -> Result<(), AtmError> {
        let mut slot = self.endpoint_guard.lock().map_err(|_| {
            AtmError::daemon_unavailable("runtime endpoint guard slot lock poisoned")
        })?;
        *slot = guard;
        Ok(())
    }

    fn take_endpoint_guard(&self) -> Result<SocketEndpointGuard, AtmError> {
        let mut slot = self.endpoint_guard.lock().map_err(|_| {
            AtmError::daemon_unavailable("runtime endpoint guard slot lock poisoned")
        })?;
        slot.take().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "runtime endpoint guard was missing during daemon serve startup",
            )
        })
    }

    fn begin_shutdown(&self) -> Result<(), AtmError> {
        self.composition_observability.emit_or_warn(
            "shutdown_requested",
            "ok",
            "daemon shutdown requested",
        );
        self.lifecycle.transition(RuntimeLifecycleState::Draining)?;
        self.request_dispatcher.stop_local_post_write_executor()?;
        Ok(())
    }

    fn begin_startup(&self) -> Result<(), AtmError> {
        self.composition_observability.emit_or_warn(
            "start_requested",
            "ok",
            "daemon start requested",
        );
        self.lifecycle.transition(RuntimeLifecycleState::Starting)?;
        Ok(())
    }

    fn rollback_failed_startup<T>(&self, error: AtmError) -> Result<T, AtmError> {
        self.composition_observability.emit_or_warn(
            "startup_failed",
            "failed",
            "daemon startup failed",
        );
        self.lifecycle.force_stopped()?;
        Err(error)
    }

    fn prepare_runtime_with<F>(&self, prepare_runtime: F) -> Result<PreparedRuntimeServer, AtmError>
    where
        F: FnOnce(&LocalIpcServerTransportAdapter) -> Result<PreparedRuntimeServer, AtmError>,
    {
        prepare_runtime(&self.server_transport)
    }

    fn activate_runtime(
        &self,
        runtime: &mut PreparedRuntimeServer,
    ) -> Result<SocketEndpointGuard, AtmError> {
        self.replace_endpoint_guard(Some(runtime.take_endpoint_guard()?))?;
        self.lifecycle.transition(RuntimeLifecycleState::Running)?;
        self.composition_observability.emit_or_warn(
            "startup_completed",
            "ok",
            "daemon startup completed",
        );
        self.take_endpoint_guard()
    }

    fn serve_runtime<P>(
        &self,
        mut runtime: PreparedRuntimeServer,
        publish_ready: P,
    ) -> Result<(), AtmError>
    where
        P: Fn() -> Result<(), AtmError>,
    {
        let endpoint_guard = self.activate_runtime(&mut runtime)?;
        let result = runtime.serve_with_runtime_hooks(
            self.request_dispatcher(),
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: super::GRACEFUL_DRAIN_DEADLINE,
                force_cancel_deadline: super::FORCE_CANCEL_DEADLINE,
                begin_shutdown: || self.begin_shutdown(),
                reload_runtime_view: || self.request_dispatcher.reload_runtime_view(),
                publish_ready,
            },
        );
        self.finish_runtime(result)
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.begin_startup()
            .or_else(|error| self.rollback_failed_startup(error))?;
        let runtime = self
            .prepare_runtime_with(|server_transport| server_transport.prepare_runtime())
            .or_else(|error| self.rollback_failed_startup(error))?;
        self.request_dispatcher
            .start_local_post_write_executor()
            .or_else(|error| self.rollback_failed_startup(error))?;
        self.serve_runtime(runtime, || Ok(()))
    }

    #[cfg(test)]
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn start_with_socket_path_for_test(
        &self,
        socket_path: PathBuf,
        ready_signal: Option<std::sync::mpsc::SyncSender<()>>,
    ) -> Result<(), AtmError> {
        self.begin_startup()
            .or_else(|error| self.rollback_failed_startup(error))?;
        let runtime = self
            .prepare_runtime_with(|server_transport| {
                server_transport.prepare_runtime_at_socket_path_for_home(
                    socket_path,
                    self.request_dispatcher.home_dir_for_test(),
                )
            })
            .or_else(|error| self.rollback_failed_startup(error))?;
        self.request_dispatcher
            .start_local_post_write_executor()
            .or_else(|error| self.rollback_failed_startup(error))?;
        self.serve_runtime(runtime, move || {
            self.request_dispatcher.preflush_observability_shutdown();
            if let Some(signal) = ready_signal.as_ref() {
                signal.send(()).map_err(|_| {
                    AtmError::daemon_unavailable(
                        "test runtime failed to publish the daemon ready signal",
                    )
                })?;
            }
            Ok(())
        })
    }

    #[cfg(test)]
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn lifecycle_state(&self) -> RuntimeLifecycleState {
        self.lifecycle.state()
    }

    fn finish_runtime(&self, result: Result<(), AtmError>) -> Result<(), AtmError> {
        let state_result = self
            .lifecycle
            .transition(RuntimeLifecycleState::Stopped)
            .map(|_| ());
        if let Err(state_error) = state_result
            && let Err(force_error) = self.lifecycle.force_stopped()
        {
            tracing::error!(
                subsystem = "composition",
                action = "force_lifecycle_stop",
                outcome = "failed",
                state_error = %state_error,
                force_error = %force_error,
                serve_error = result.as_ref().err().map(|error| error.to_string()),
                "daemon runtime failed while forcing lifecycle back to stopped"
            );
            return match result {
                Err(error) => Err(error),
                Ok(()) => Err(force_error),
            };
        }
        // Preflush any queued retained-log work before emitting the terminal
        // shutdown event so the final status update does not race older queue
        // entries during shutdown.
        self.request_dispatcher.preflush_observability_shutdown();
        match result.as_ref() {
            Ok(()) => {
                self.composition_observability.emit_or_warn(
                    "shutdown_completed",
                    "ok",
                    "daemon shutdown completed",
                );
            }
            Err(_) => {
                self.composition_observability.emit_or_warn(
                    "shutdown_failed",
                    "failed",
                    "daemon shutdown failed",
                );
            }
        }
        self.request_dispatcher.finalize_observability_shutdown();
        result
    }
}

fn build_host_ownership_adapter(
    observability: &Arc<dyn DaemonRuntimeObservability>,
) -> HostOwnershipAdapter {
    HostOwnershipAdapter::new_with_observability(SubsystemObservability::new(
        DaemonSubsystem::HostOwnership,
        Arc::clone(observability),
    ))
}

#[cfg(test)]
pub(crate) fn build_production_runtime(
    assembly: &RuntimeAssembly,
    non_claude_outbound: Arc<dyn atm_core::boundary::NonClaudeOutbound + Send + Sync>,
) -> atm_core::LocalServiceRuntime {
    atm_core::LocalServiceRuntime::new_with_delivery_boundaries(
        assembly.message_store_arc(),
        assembly.shared_roster_store_arc(),
        assembly.nudge_template_override_store.clone(),
        non_claude_outbound,
    )
}

fn runtime_assembly_failed(error: AtmError, observability: &SubsystemObservability) -> AtmError {
    tracing::error!(
        subsystem = "composition",
        action = "runtime_assembly",
        outcome = "failed",
        %error,
        "daemon runtime assembly failed"
    );
    observability.emit_or_warn(
        "runtime_assembly",
        "failed",
        "daemon runtime assembly failed",
    );
    AtmError::daemon_unavailable(
        "daemon runtime assembly is unavailable; atm-daemon startup is blocked",
    )
}

fn build_request_dispatcher(
    home_dir: AtmHomeDir,
    status_cache: &RuntimeStatusCache,
    observability: &Arc<dyn DaemonRuntimeObservability>,
    runtime_assembly: RuntimeAssembly,
) -> Result<Arc<DaemonRequestDispatcher>, AtmError> {
    Ok(Arc::new(DaemonRequestDispatcher::new(
        home_dir,
        status_cache.clone(),
        Arc::clone(observability),
        runtime_assembly,
    )?))
}

fn build_server_transport(
    observability: &Arc<dyn DaemonRuntimeObservability>,
) -> LocalIpcServerTransportAdapter {
    LocalIpcServerTransportAdapter::new_with_observability(
        SubsystemObservability::new(
            DaemonSubsystem::LocalIpcTransport,
            Arc::clone(observability),
        ),
        SubsystemObservability::new(DaemonSubsystem::HostOwnership, Arc::clone(observability)),
        SubsystemObservability::new(DaemonSubsystem::LifecycleControl, Arc::clone(observability)),
    )
}

#[cfg(test)]
fn shutdown_lane_with_deadline<T, F>(
    lane_name: &'static str,
    deadline: Duration,
    lane: T,
    shutdown: F,
) -> Result<(), AtmError>
where
    T: Send + 'static,
    F: FnOnce(T) -> Result<(), AtmError> + Send + 'static,
{
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let shutdown_handle = std::thread::Builder::new()
        .name("shutdown-lane-deadline".to_string())
        .spawn(move || {
            let _ = result_tx.send(shutdown(lane));
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to spawn daemon {lane_name} shutdown deadline helper"
            ))
        })?;
    let shutdown_thread_id = shutdown_handle.thread().id();
    match result_rx.recv_timeout(deadline) {
        Ok(result) => {
            shutdown_handle.join().map_err(|_| {
                AtmError::daemon_unavailable(format!(
                    "daemon {lane_name} shutdown worker panicked unexpectedly"
                ))
            })?;
            result
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            retain_join_helper(lane_name, shutdown_handle, deadline);
            tracing::warn!(
                subsystem = "composition",
                action = "shutdown_lane_deadline",
                outcome = "deadline_exceeded",
                lane = lane_name,
                timeout_ms = deadline.as_millis(),
                thread_id = ?shutdown_thread_id,
                "daemon shutdown lane timed out; join worker left detached after deadline"
            );
            Err(AtmError::daemon_unavailable(format!(
                "daemon {lane_name} shutdown exceeded the {deadline:?} per-lane deadline"
            )))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => shutdown_handle.join().map_or_else(
            |_| {
                Err(AtmError::daemon_unavailable(format!(
                    "daemon {lane_name} shutdown worker panicked unexpectedly"
                )))
            },
            |_| {
                Err(AtmError::daemon_unavailable(format!(
                    "daemon {lane_name} shutdown worker disconnected unexpectedly"
                )))
            },
        ),
    }
}

fn validate_runtime_home_dir(home_dir: &std::path::Path) -> Result<(), AtmError> {
    std::fs::create_dir_all(home_dir).map_err(|_source| {
        AtmError::daemon_unavailable(format!(
            "failed to create atm-daemon home directory at {}",
            home_dir.display()
        ))
    })?;
    let probe_path = home_dir.join(format!(".atm-daemon-home-probe-{}", std::process::id()));
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe_path)
        .map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "atm-daemon home directory is not writable at {}",
                home_dir.display()
            ))
        })?;
    if let Err(error) = std::fs::remove_file(&probe_path) {
        tracing::warn!(
            subsystem = "composition",
            action = "home_probe_cleanup",
            outcome = "failed",
            path = %probe_path.display(),
            %error,
            "failed to remove atm-daemon home write probe file"
        );
    }
    Ok(())
}

pub(crate) fn compose_runtime(
    observability: Arc<dyn DaemonRuntimeObservability>,
) -> Result<RuntimeComposition, AtmError> {
    let home_dir = AtmHomeDir::resolve()?;
    validate_runtime_home_dir(home_dir.as_path())?;
    // The daemon snapshots the startup cwd once for config discovery and does
    // not refresh it on SIGHUP; restart atm-daemon to adopt a different
    // workspace root after changing the launch directory.
    let current_dir = std::env::current_dir().map_err(|_source| {
        AtmError::daemon_unavailable(
            "failed to resolve the current working directory for daemon runtime assembly",
        )
    })?;
    let runtime_assembly = assemble_host_runtime(
        current_dir.clone(),
        Arc::new(DaemonNonClaudeOutbound::new()),
    )
    .map_err(|error| {
        runtime_assembly_failed(
            error,
            &SubsystemObservability::new(DaemonSubsystem::Composition, Arc::clone(&observability)),
        )
    })?;
    RuntimeComposition::new_with_runtime_assembly(home_dir, observability, runtime_assembly)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[cfg(not(windows))]
    use crate::lifecycle_control::LifecycleControlSourceAdapter;
    #[cfg(not(windows))]
    use crate::test_support::LifecycleFlagResetGuard;

    use super::RuntimeComposition;
    use super::{RuntimeLifecycle, RuntimeLifecycleState, shutdown_lane_with_deadline};

    struct CwdGuard(PathBuf);

    impl CwdGuard {
        fn install() -> Self {
            Self(std::env::current_dir().expect("cwd"))
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).expect("restore cwd");
        }
    }

    #[test]
    fn runtime_lifecycle_allows_only_documented_transitions() {
        let lifecycle = RuntimeLifecycle::new();
        assert_eq!(lifecycle.state(), RuntimeLifecycleState::Stopped);
        lifecycle
            .transition(RuntimeLifecycleState::Starting)
            .expect("start");
        lifecycle
            .transition(RuntimeLifecycleState::Running)
            .expect("running");
        lifecycle
            .transition(RuntimeLifecycleState::Draining)
            .expect("draining");
        lifecycle
            .transition(RuntimeLifecycleState::Stopped)
            .expect("stopped");
    }

    #[test]
    fn runtime_lifecycle_happy_path_matches_documented_owner_sequence() {
        let lifecycle = RuntimeLifecycle::new();

        assert_eq!(
            lifecycle
                .transition(RuntimeLifecycleState::Starting)
                .expect("stopped -> starting"),
            RuntimeLifecycleState::Starting
        );
        assert_eq!(
            lifecycle
                .transition(RuntimeLifecycleState::Running)
                .expect("starting -> running"),
            RuntimeLifecycleState::Running
        );
        assert_eq!(
            lifecycle
                .transition(RuntimeLifecycleState::Draining)
                .expect("running -> draining"),
            RuntimeLifecycleState::Draining
        );
        assert_eq!(
            lifecycle
                .transition(RuntimeLifecycleState::Stopped)
                .expect("draining -> stopped"),
            RuntimeLifecycleState::Stopped
        );
    }

    #[test]
    fn runtime_lifecycle_rejects_illegal_transitions() {
        let lifecycle = RuntimeLifecycle::new();
        let stopped_to_running = lifecycle
            .transition(RuntimeLifecycleState::Running)
            .expect_err("illegal stopped -> running transition");
        assert!(
            stopped_to_running
                .to_string()
                .contains("illegal daemon runtime lifecycle transition")
        );

        lifecycle
            .transition(RuntimeLifecycleState::Starting)
            .expect("stopped -> starting");
        let starting_to_starting = lifecycle
            .transition(RuntimeLifecycleState::Starting)
            .expect_err("illegal starting -> starting transition");
        assert!(
            starting_to_starting
                .to_string()
                .contains("illegal daemon runtime lifecycle transition")
        );
    }

    #[test]
    fn startup_failure_path_can_transition_back_to_stopped() {
        let lifecycle = RuntimeLifecycle::new();
        lifecycle
            .transition(RuntimeLifecycleState::Starting)
            .expect("starting");
        lifecycle.force_stopped().expect("force stopped");
        assert_eq!(lifecycle.state(), RuntimeLifecycleState::Stopped);
    }

    #[cfg(not(windows))]
    #[test]
    #[serial_test::serial(env)]
    fn runtime_composition_failed_startup_returns_to_stopped() {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let _reset = LifecycleFlagResetGuard::install(lifecycle);
        let tempdir = TempDir::new().expect("tempdir");
        let parent_file = tempdir.path().join("not-a-dir");
        std::fs::write(&parent_file, "x").expect("parent file");
        let socket_path = parent_file.join("atm.sock");
        let runtime = RuntimeComposition::new(tempdir.path().to_path_buf()).expect("runtime");

        let error = runtime
            .start_with_socket_path_for_test(socket_path, None)
            .expect_err("startup should fail");

        assert!(error.is_daemon_unavailable());
        assert_eq!(runtime.lifecycle_state(), RuntimeLifecycleState::Stopped);
        assert!(
            atm_core::home::host_runtime_lock_path_from_home(
                tempdir.path(),
                crate::host_ownership::HOST_RUNTIME_OWNER_LOCK_FILE,
            )
            .exists(),
            "test startup should isolate owner.lock under the temp home root"
        );
        let retained_log_path =
            atm_core::home::host_log_dir_from_home(tempdir.path()).join("atm.log.jsonl");
        let retained_log = std::fs::read_to_string(retained_log_path).expect("retained log");
        assert!(retained_log.contains("daemon start requested"));
        assert!(retained_log.contains("daemon startup failed"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn runtime_composition_fails_closed_when_runtime_storage_cannot_open() {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&home_dir).expect("atm home");
        let storage_parent_file = tempdir.path().join("not-a-dir");
        std::fs::write(&storage_parent_file, "x").expect("parent file");
        let _cwd_guard = CwdGuard::install();
        std::env::set_current_dir(tempdir.path()).expect("set isolated cwd");
        let observability = std::sync::Arc::new(
            crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )
            .expect("test observability"),
        );

        let error = RuntimeComposition::new_with_runtime_db_path(
            crate::AtmHomeDir::from_path_for_test(home_dir),
            storage_parent_file.join("mail.db"),
            observability,
        )
        .expect_err("runtime storage assembly should fail closed");

        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(
            error
                .to_string()
                .contains("daemon runtime assembly is unavailable"),
            "{error}"
        );
    }

    #[test]
    fn shutdown_lane_timeout_returns_without_waiting_for_worker_completion() {
        let (release_tx, release_rx) = mpsc::channel();
        let started = Instant::now();
        let result =
            shutdown_lane_with_deadline("test lane", Duration::from_millis(20), release_rx, |rx| {
                let _ = rx.recv();
                Ok(())
            });
        let elapsed = started.elapsed();
        assert!(result.is_err(), "expected deadline error");
        assert!(
            elapsed < Duration::from_millis(200),
            "shutdown helper blocked too long after timeout: {elapsed:?}"
        );
        let _ = release_tx.send(());
    }
}
