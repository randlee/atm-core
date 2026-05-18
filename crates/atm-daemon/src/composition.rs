use crate::boundary_adapters::{
    DaemonConfigIngress, DaemonInboxExport, DaemonInboxIngress, DaemonNotificationSink,
    DaemonReconcileCoordinator, FileWatchEventSource,
};
use crate::daemon_runtime_observability::{DaemonRuntimeObservability, SubsystemObservability};
use crate::host_ownership::HostOwnershipAdapter;
use crate::local_ipc_transport::{PreparedRuntimeServer, RuntimeServeHooks, SocketEndpointGuard};
use crate::runtime_health::DaemonRequestDispatcher;
use crate::runtime_health::{DaemonStatusSource, RuntimeStatusCache};
use crate::sqlite_observability::DaemonSqliteObservability;
use crate::{
    AtmHomeDir, DaemonSubsystem, LocalIpcServerTransportAdapter, PeerTransportRuntime,
    peer_transport::PeerTransportConfig, sqlite_remote_replay_store_from_path_with_observability,
};
use atm_core::boundary::{ConfigIngress, ConfigLoadRequest, RequestDispatcher};
use atm_core::error::AtmError;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

const BACKGROUND_LANE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);

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
            .map_err(|_| {
                AtmError::daemon_unavailable("runtime lifecycle state lock poisoned")
                    .with_recovery(
                        "Restart atm-daemon; runtime lifecycle transitions can no longer be trusted after the poisoned state lock.",
                    )
            })?;
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
            ))
            .with_recovery("Enter daemon exclusively through RuntimeComposition::start()."));
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
            .map_err(|_| {
                AtmError::daemon_unavailable("runtime lifecycle state lock poisoned")
                    .with_recovery(
                        "Restart atm-daemon; runtime lifecycle transitions can no longer be trusted after the poisoned state lock.",
                    )
            })?;
        *state = RuntimeLifecycleState::Stopped;
        Ok(())
    }
}

/// Internal root for Phase R daemon runtime wiring.
#[derive(Debug)]
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
    _notification_sink: DaemonNotificationSink,
    _status_source: DaemonStatusSource,
    _watch_event_source: FileWatchEventSource,
    _reconcile_coordinator: DaemonReconcileCoordinator,
    _config_ingress: DaemonConfigIngress,
    _inbox_ingress: DaemonInboxIngress,
    _inbox_export: DaemonInboxExport,
    peer_transport_runtime: PeerTransportRuntime,
}

impl RuntimeComposition {
    #[cfg(test)]
    #[cfg_attr(windows, allow(dead_code))]
    fn new(home_dir: PathBuf) -> Result<Self, AtmError> {
        Self::new_with_replay_store_path(
            AtmHomeDir::from_path_for_test(home_dir.clone()),
            atm_core::home::host_mail_db_path_from_home(&home_dir),
            Arc::new(crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )?),
        )
    }

    fn new_with_replay_store_path(
        home_dir: AtmHomeDir,
        replay_store_path: PathBuf,
        observability: Arc<dyn DaemonRuntimeObservability>,
    ) -> Result<Self, AtmError> {
        let config_ingress = DaemonConfigIngress::new();
        let status_cache = RuntimeStatusCache::new_with_observability(SubsystemObservability::new(
            DaemonSubsystem::RuntimeStatusCache,
            Arc::clone(&observability),
        ));
        let sqlite_observability: Arc<dyn atm_rusqlite::SqliteObservability> = Arc::new(
            DaemonSqliteObservability::new(Arc::clone(&observability), status_cache.clone()),
        );
        let notification_sink =
            DaemonNotificationSink::new_with_observability(SubsystemObservability::new(
                DaemonSubsystem::NotificationRuntime,
                Arc::clone(&observability),
            ));
        let watch_event_source = FileWatchEventSource::new_with_observability(
            SubsystemObservability::new(DaemonSubsystem::WatchRuntime, Arc::clone(&observability)),
        );
        let inbox_ingress = DaemonInboxIngress::new();
        let composition_observability =
            SubsystemObservability::new(DaemonSubsystem::Composition, Arc::clone(&observability));
        let peer_transport_config =
            load_peer_transport_config(&config_ingress, &composition_observability)?;
        let server_transport = build_server_transport(&observability);
        let request_dispatcher = build_request_dispatcher(
            home_dir,
            &status_cache,
            &observability,
            Arc::clone(&sqlite_observability),
        );
        let replay_store = sqlite_remote_replay_store_from_path_with_observability(
            replay_store_path,
            Arc::clone(&sqlite_observability),
        )
        .map_err(|error| replay_store_assembly_failed(error, &composition_observability))?;
        Ok(Self {
            lifecycle: Arc::new(RuntimeLifecycle::new()),
            _host_ownership_adapter: HostOwnershipAdapter::new_with_observability(
                SubsystemObservability::new(
                    DaemonSubsystem::HostOwnership,
                    Arc::clone(&observability),
                ),
            ),
            endpoint_guard: Mutex::new(None),
            server_transport,
            request_dispatcher,
            composition_observability,
            _notification_sink: notification_sink.clone(),
            _status_source: DaemonStatusSource::new(status_cache),
            _watch_event_source: watch_event_source.clone(),
            _reconcile_coordinator: DaemonReconcileCoordinator::new_with_observability(
                watch_event_source,
                inbox_ingress.clone(),
                notification_sink,
                SubsystemObservability::new(
                    DaemonSubsystem::ReconcileRuntime,
                    Arc::clone(&observability),
                ),
            ),
            _config_ingress: config_ingress,
            _inbox_ingress: inbox_ingress,
            _inbox_export: DaemonInboxExport::new(),
            peer_transport_runtime: PeerTransportRuntime::new_with_observability(
                Some(replay_store),
                peer_transport_config,
                SubsystemObservability::new(DaemonSubsystem::PeerTransport, observability),
            ),
        })
    }

    fn request_dispatcher(&self) -> Arc<dyn RequestDispatcher + Send + Sync> {
        self.request_dispatcher.clone()
    }

    fn replace_endpoint_guard(&self, guard: Option<SocketEndpointGuard>) -> Result<(), AtmError> {
        let mut slot = self.endpoint_guard.lock().map_err(|_| {
            AtmError::daemon_unavailable("runtime endpoint guard slot lock poisoned").with_recovery(
                "Restart the daemon; same-host endpoint cleanup ownership can no longer be tracked safely.",
            )
        })?;
        *slot = guard;
        Ok(())
    }

    fn take_endpoint_guard(&self) -> Result<SocketEndpointGuard, AtmError> {
        let mut slot = self.endpoint_guard.lock().map_err(|_| {
            AtmError::daemon_unavailable("runtime endpoint guard slot lock poisoned").with_recovery(
                "Restart the daemon; same-host endpoint cleanup ownership can no longer be tracked safely.",
            )
        })?;
        slot.take().ok_or_else(|| {
            AtmError::daemon_unavailable("runtime endpoint guard was missing during daemon serve startup")
                .with_recovery(
                    "Restart the daemon; same-host endpoint cleanup ownership was lost before the listener entered serving state.",
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
        // Attempt every lane shutdown even if one lane fails so the runtime
        // still reaches checkpoint/flush finalization with the fullest cleanup
        // state possible.
        self.shutdown_background_lanes()?;
        Ok(())
    }

    fn finalize_shutdown(&self) {
        self.request_dispatcher.finalize_shutdown();
    }

    fn begin_startup(&self) -> Result<(), AtmError> {
        self.composition_observability.emit_or_warn(
            "start_requested",
            "ok",
            "daemon start requested",
        );
        self.lifecycle.transition(RuntimeLifecycleState::Starting)?;
        self.resume_startup_replay()?;
        self.start_background_lanes()
    }

    fn resume_startup_replay(&self) -> Result<(), AtmError> {
        // Startup replay must finish before the daemon binds its socket so
        // crash-recovered work cannot race newly accepted requests.
        let replay_summary = self.peer_transport_runtime.resume_pending_replay()?;
        if replay_summary.delivered > 0
            || replay_summary.retained > 0
            || replay_summary.purged_expired > 0
        {
            tracing::info!(
                replay_delivered = replay_summary.delivered,
                replay_retained = replay_summary.retained,
                replay_purged_expired = replay_summary.purged_expired,
                "daemon startup replay sweep completed"
            );
        }
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

    fn prepare_runtime_with<F>(
        &self,
        rollback_message: &'static str,
        prepare_runtime: F,
    ) -> Result<PreparedRuntimeServer, AtmError>
    where
        F: FnOnce(&LocalIpcServerTransportAdapter) -> Result<PreparedRuntimeServer, AtmError>,
    {
        prepare_runtime(&self.server_transport).inspect_err(|_| {
            if let Err(shutdown_error) = self.shutdown_background_lanes() {
                tracing::warn!(%shutdown_error, "{rollback_message}");
            }
        })
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
        let request_dispatcher = Arc::clone(&self.request_dispatcher);
        let endpoint_guard = self.activate_runtime(&mut runtime)?;
        let result = runtime.serve_with_runtime_hooks(
            self.request_dispatcher(),
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: super::GRACEFUL_DRAIN_DEADLINE,
                force_cancel_deadline: super::FORCE_CANCEL_DEADLINE,
                begin_shutdown: || self.begin_shutdown(),
                reload_runtime_view: move || request_dispatcher.reload_runtime_view(),
                finalize_shutdown: || self.finalize_shutdown(),
                publish_ready,
            },
        );
        self.finish_runtime(result)
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.begin_startup()
            .or_else(|error| self.rollback_failed_startup(error))?;
        let runtime = self
            .prepare_runtime_with(
                "daemon background lane shutdown failed during runtime preparation rollback",
                |server_transport| server_transport.prepare_runtime(),
            )
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
            .prepare_runtime_with(
                "daemon background lane shutdown failed during test runtime preparation rollback",
                |server_transport| server_transport.prepare_runtime_at_socket_path(socket_path),
            )
            .or_else(|error| self.rollback_failed_startup(error))?;
        self.serve_runtime(runtime, move || {
            if let Some(signal) = ready_signal.as_ref() {
                signal.send(()).map_err(|_| {
                    AtmError::daemon_unavailable(
                        "test runtime failed to publish the daemon ready signal",
                    )
                    .with_recovery(
                        "Restore the bounded ready-signal handshake before retrying the same-host daemon runtime test.",
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
        result
    }

    fn start_background_lanes(&self) -> Result<(), AtmError> {
        self._notification_sink.start()?;
        if let Err(error) = self._watch_event_source.start() {
            self.rollback_partially_started_lanes(StartedLanes {
                watch_started: false,
                notification_started: true,
            });
            return Err(error);
        }
        if let Err(error) = self._reconcile_coordinator.start() {
            self.rollback_partially_started_lanes(StartedLanes {
                watch_started: true,
                notification_started: true,
            });
            return Err(error);
        }
        Ok(())
    }

    fn rollback_partially_started_lanes(&self, started_lanes: StartedLanes) {
        if started_lanes.watch_started
            && let Err(error) = shutdown_lane_with_deadline(
                "watch event source",
                BACKGROUND_LANE_SHUTDOWN_DEADLINE,
                self._watch_event_source.clone(),
                |lane| lane.shutdown(),
            )
        {
            tracing::warn!(
                %error,
                lane = "watch event source",
                "daemon background lane rollback shutdown was incomplete"
            );
        }
        if started_lanes.notification_started
            && let Err(error) = shutdown_lane_with_deadline(
                "notification sink",
                BACKGROUND_LANE_SHUTDOWN_DEADLINE,
                self._notification_sink.clone(),
                |lane| lane.shutdown(),
            )
        {
            tracing::warn!(
                %error,
                lane = "notification sink",
                "daemon background lane rollback shutdown was incomplete"
            );
        }
    }

    fn shutdown_background_lanes(&self) -> Result<(), AtmError> {
        let mut first_error = None;
        for (lane_name, shutdown) in [
            (
                "reconcile coordinator",
                shutdown_lane_with_deadline(
                    "reconcile coordinator",
                    BACKGROUND_LANE_SHUTDOWN_DEADLINE,
                    self._reconcile_coordinator.clone(),
                    |lane| lane.shutdown(),
                ),
            ),
            (
                "watch event source",
                shutdown_lane_with_deadline(
                    "watch event source",
                    BACKGROUND_LANE_SHUTDOWN_DEADLINE,
                    self._watch_event_source.clone(),
                    |lane| lane.shutdown(),
                ),
            ),
            (
                "notification sink",
                shutdown_lane_with_deadline(
                    "notification sink",
                    BACKGROUND_LANE_SHUTDOWN_DEADLINE,
                    self._notification_sink.clone(),
                    |lane| lane.shutdown(),
                ),
            ),
        ] {
            if let Err(error) = shutdown {
                tracing::warn!(
                    subsystem = "composition",
                    action = "shutdown_lane",
                    outcome = "incomplete",
                    %error,
                    lane = lane_name,
                    "daemon background lane shutdown was incomplete"
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn replay_store_assembly_failed(
    error: AtmError,
    observability: &SubsystemObservability,
) -> AtmError {
    tracing::error!(
        %error,
        "remote replay store assembly failed; daemon startup is fail-closed"
    );
    observability.emit_or_warn(
        "sqlite_replay_store_assembly",
        "failed",
        "remote replay store assembly failed; daemon startup is fail-closed",
    );
    AtmError::daemon_unavailable(
        "remote replay store is unavailable; atm-daemon startup is blocked",
    )
    .with_recovery(
        "Restore the host-scoped ATM SQLite replay store before starting atm-daemon so the required bounded replay-resume sweep can run.",
    )
    .with_source(error)
}

fn load_peer_transport_config(
    config_ingress: &DaemonConfigIngress,
    observability: &SubsystemObservability,
) -> Result<PeerTransportConfig, AtmError> {
    let current_dir = std::env::current_dir().map_err(|source| {
        observability.emit_or_warn(
            "peer_transport_config_resolution",
            "failed",
            "daemon startup could not resolve the current working directory for peer transport config",
        );
        AtmError::daemon_unavailable(
            "failed to resolve the current working directory for daemon peer transport config",
        )
        .with_recovery(
            "Start atm-daemon from a readable ATM workspace so the daemon can load and validate peer transport settings before serving requests.",
        )
        .with_source(source)
    })?;
    let config = config_ingress
        .load_config(ConfigLoadRequest { current_dir })
        .inspect_err(|_| {
            observability.emit_or_warn(
                "peer_transport_config_load",
                "failed",
                "daemon startup could not load peer transport config through ConfigIngress",
            );
        })?
        .config;
    PeerTransportConfig::from_config(config.as_ref()).inspect_err(|_| {
        observability.emit_or_warn(
            "peer_transport_config_validation",
            "failed",
            "daemon startup rejected invalid peer transport configuration",
        );
    })
}

fn build_request_dispatcher(
    home_dir: AtmHomeDir,
    status_cache: &RuntimeStatusCache,
    observability: &Arc<dyn DaemonRuntimeObservability>,
    sqlite_observability: Arc<dyn atm_rusqlite::SqliteObservability>,
) -> Arc<DaemonRequestDispatcher> {
    Arc::new(DaemonRequestDispatcher::new(
        home_dir,
        status_cache.clone(),
        Arc::clone(observability),
        sqlite_observability,
    ))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct StartedLanes {
    watch_started: bool,
    notification_started: bool,
}

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
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to spawn daemon {lane_name} shutdown deadline helper"
            ))
            .with_recovery(
                "Restart atm-daemon; the bounded background-lane shutdown helper could not be created.",
            )
            .with_source(source)
        })?;
    let shutdown_thread_id = shutdown_handle.thread().id();
    match result_rx.recv_timeout(deadline) {
        Ok(result) => {
            shutdown_handle.join().map_err(|_| {
                AtmError::daemon_unavailable(format!(
                    "daemon {lane_name} shutdown worker panicked unexpectedly"
                ))
                .with_recovery(
                    "Restart atm-daemon; one shutdown lane crashed while the runtime was draining background work.",
                )
            })?;
            result
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
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
            ))
            .with_recovery(
                "Restart atm-daemon after the stalled background lane stops holding runtime shutdown open.",
            ))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => shutdown_handle.join().map_or_else(
            |_| {
                Err(AtmError::daemon_unavailable(format!(
                    "daemon {lane_name} shutdown worker panicked unexpectedly"
                ))
                .with_recovery(
                    "Restart atm-daemon; one shutdown lane crashed while the runtime was draining background work.",
                ))
            },
            |_| {
                Err(AtmError::daemon_unavailable(format!(
                    "daemon {lane_name} shutdown worker disconnected unexpectedly"
                ))
                .with_recovery(
                    "Restart atm-daemon; one shutdown lane stopped reporting progress during runtime teardown.",
                ))
            },
        ),
    }
}

fn validate_runtime_home_dir(home_dir: &std::path::Path) -> Result<(), AtmError> {
    std::fs::create_dir_all(home_dir).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to create atm-daemon home directory at {}",
            home_dir.display()
        ))
        .with_recovery(
            "Grant write access to ATM_HOME or choose a writable daemon home directory before starting atm-daemon.",
        )
        .with_source(source)
    })?;
    let probe_path = home_dir.join(format!(".atm-daemon-home-probe-{}", std::process::id()));
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe_path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "atm-daemon home directory is not writable at {}",
                home_dir.display()
            ))
            .with_recovery(
                "Grant write access to ATM_HOME or point ATM_HOME at a writable directory before retrying.",
            )
            .with_source(source)
        })?;
    if let Err(error) = std::fs::remove_file(&probe_path) {
        tracing::warn!(
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
    RuntimeComposition::new_with_replay_store_path(
        home_dir,
        atm_core::home::host_mail_db_path()?,
        observability,
    )
}

#[cfg(test)]
mod tests {
    use atm_core::boundary::ServerTransport;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

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

    #[test]
    #[serial_test::serial(env)]
    fn runtime_composition_failed_startup_returns_to_stopped() {
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
        let retained_log_path =
            atm_core::home::host_log_dir_from_home(tempdir.path()).join("atm.log.jsonl");
        let retained_log = std::fs::read_to_string(retained_log_path).expect("retained log");
        assert!(retained_log.contains("daemon start requested"));
        assert!(retained_log.contains("daemon startup failed"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn server_transport_cannot_bootstrap_outside_runtime_composition_start() {
        let tempdir = TempDir::new().expect("tempdir");
        let _cwd_guard = CwdGuard::install();
        std::env::set_current_dir(tempdir.path()).expect("set isolated cwd");

        let runtime = RuntimeComposition::new(tempdir.path().to_path_buf()).expect("runtime");

        let error = ServerTransport::serve(&runtime.server_transport, runtime.request_dispatcher())
            .expect_err("direct transport bootstrap should be rejected");

        assert!(error.is_daemon_unavailable());
        assert!(
            error
                .to_string()
                .contains("cannot bootstrap the daemon directly")
        );
        assert_eq!(runtime.lifecycle_state(), RuntimeLifecycleState::Stopped);
    }

    #[test]
    #[serial_test::serial(env)]
    fn runtime_composition_fails_closed_when_replay_store_cannot_open() {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&home_dir).expect("atm home");
        let replay_parent_file = tempdir.path().join("not-a-dir");
        std::fs::write(&replay_parent_file, "x").expect("parent file");
        let _cwd_guard = CwdGuard::install();
        std::env::set_current_dir(tempdir.path()).expect("set isolated cwd");
        let observability = std::sync::Arc::new(
            crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )
            .expect("test observability"),
        );

        let error = RuntimeComposition::new_with_replay_store_path(
            crate::AtmHomeDir::from_path_for_test(home_dir),
            replay_parent_file.join("mail.db"),
            observability,
        )
        .expect_err("replay-store assembly should fail closed");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(
            error
                .to_string()
                .contains("remote replay store is unavailable"),
            "{error}"
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn runtime_composition_fails_closed_when_peer_transport_config_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let workspace_dir = tempdir.path().join("workspace");
        let home_dir = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
        std::fs::create_dir_all(&home_dir).expect("atm home");
        std::fs::write(
            workspace_dir.join(".atm.toml"),
            "[daemon]\nremote_retry_budget = \"100ms\"\n",
        )
        .expect("invalid config");
        let _cwd_guard = CwdGuard::install();
        std::env::set_current_dir(&workspace_dir).expect("set workspace cwd");

        let result = RuntimeComposition::new(home_dir.clone());

        let error = result.expect_err("invalid peer transport config should fail closed");
        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::MessageValidationFailed
        );
        assert!(
            error
                .recovery
                .as_deref()
                .expect("recovery guidance")
                .contains("at least one second"),
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
