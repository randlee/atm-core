use crate::boundary_adapters::{
    DaemonConfigIngress, DaemonInboxExport, DaemonInboxIngress, DaemonNotificationSink,
    DaemonReconcileCoordinator, FileWatchEventSource,
};
use crate::runtime_health::{DaemonRequestDispatcher, DaemonStatusSource, RuntimeStatusCache};
use crate::{
    LocalSocketServerTransport, PeerTransportRuntime, sqlite_remote_replay_store_from_path,
};
use atm_core::{boundary::RequestDispatcher, error::AtmError};
#[cfg(unix)]
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
            .map_err(|_| AtmError::daemon_unavailable("runtime lifecycle state lock poisoned"))?;
        *state = RuntimeLifecycleState::Stopped;
        Ok(())
    }
}

/// Internal root for Phase R daemon runtime wiring.
#[derive(Debug)]
pub(crate) struct RuntimeComposition {
    lifecycle: Arc<RuntimeLifecycle>,
    server_transport: LocalSocketServerTransport,
    request_dispatcher: Arc<DaemonRequestDispatcher>,
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
    fn new(home_dir: PathBuf) -> Self {
        let status_cache = RuntimeStatusCache::new();
        let notification_sink = DaemonNotificationSink::new();
        let watch_event_source = FileWatchEventSource::new();
        let inbox_ingress = DaemonInboxIngress::new();
        let replay_store = match atm_core::home::host_mail_db_path() {
            Ok(db_path) => match sqlite_remote_replay_store_from_path(db_path) {
                Ok(store) => Some(store),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "remote replay store unavailable; outcome-unknown delivery cannot be persisted"
                    );
                    None
                }
            },
            Err(error) => {
                tracing::warn!(
                    %error,
                    "remote replay store path unavailable; crash-safe remote replay persistence is disabled"
                );
                None
            }
        };
        Self {
            lifecycle: Arc::new(RuntimeLifecycle::new()),
            server_transport: LocalSocketServerTransport::new(),
            request_dispatcher: Arc::new(DaemonRequestDispatcher::new(
                home_dir,
                status_cache.clone(),
            )),
            _notification_sink: notification_sink.clone(),
            _status_source: DaemonStatusSource::new(status_cache),
            _watch_event_source: watch_event_source.clone(),
            _reconcile_coordinator: DaemonReconcileCoordinator::new(
                watch_event_source,
                inbox_ingress.clone(),
                notification_sink,
            ),
            _config_ingress: DaemonConfigIngress::new(),
            _inbox_ingress: inbox_ingress,
            _inbox_export: DaemonInboxExport::new(),
            peer_transport_runtime: PeerTransportRuntime::new(replay_store),
        }
    }

    fn request_dispatcher(&self) -> Arc<dyn RequestDispatcher + Send + Sync> {
        self.request_dispatcher.clone()
    }

    fn begin_shutdown(&self) -> Result<(), AtmError> {
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

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.lifecycle.transition(RuntimeLifecycleState::Starting)?;
        // Startup replay must finish before the daemon binds its socket so
        // crash-recovered work cannot race newly accepted requests.
        let replay_summary = match self.peer_transport_runtime.resume_pending_replay() {
            Ok(summary) => summary,
            Err(error) => {
                self.lifecycle.force_stopped()?;
                return Err(error);
            }
        };
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
        if let Err(error) = self.start_background_lanes() {
            self.lifecycle.force_stopped()?;
            return Err(error);
        }
        let runtime = match self.server_transport.prepare_runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                if let Err(shutdown_error) = self.shutdown_background_lanes() {
                    tracing::warn!(
                        %shutdown_error,
                        "daemon background lane shutdown failed during runtime preparation rollback"
                    );
                }
                self.lifecycle.force_stopped()?;
                return Err(error);
            }
        };
        self.lifecycle.transition(RuntimeLifecycleState::Running)?;
        let request_dispatcher = Arc::clone(&self.request_dispatcher);
        let result = runtime.serve_with_runtime_hooks(
            self.request_dispatcher(),
            super::GRACEFUL_DRAIN_DEADLINE,
            super::FORCE_CANCEL_DEADLINE,
            || self.begin_shutdown(),
            move || request_dispatcher.reload_runtime_view(),
            || self.finalize_shutdown(),
        );
        self.finish_runtime(result)
    }

    #[cfg(test)]
    pub(crate) fn start_with_socket_path_for_test(
        &self,
        socket_path: PathBuf,
    ) -> Result<(), AtmError> {
        self.lifecycle.transition(RuntimeLifecycleState::Starting)?;
        let replay_summary = match self.peer_transport_runtime.resume_pending_replay() {
            Ok(summary) => summary,
            Err(error) => {
                self.lifecycle.force_stopped()?;
                return Err(error);
            }
        };
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
        if let Err(error) = self.start_background_lanes() {
            self.lifecycle.force_stopped()?;
            return Err(error);
        }
        let runtime = match self
            .server_transport
            .prepare_runtime_at_socket_path(socket_path)
        {
            Ok(runtime) => runtime,
            Err(error) => {
                if let Err(shutdown_error) = self.shutdown_background_lanes() {
                    tracing::warn!(
                        %shutdown_error,
                        "daemon background lane shutdown failed during test runtime preparation rollback"
                    );
                }
                self.lifecycle.force_stopped()?;
                return Err(error);
            }
        };
        self.lifecycle.transition(RuntimeLifecycleState::Running)?;
        let request_dispatcher = Arc::clone(&self.request_dispatcher);
        let result = runtime.serve_with_runtime_hooks(
            self.request_dispatcher(),
            super::GRACEFUL_DRAIN_DEADLINE,
            super::FORCE_CANCEL_DEADLINE,
            || self.begin_shutdown(),
            move || request_dispatcher.reload_runtime_view(),
            || self.finalize_shutdown(),
        );
        self.finish_runtime(result)
    }

    #[cfg(test)]
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
        result
    }

    fn start_background_lanes(&self) -> Result<(), AtmError> {
        self._notification_sink.start()?;
        self._watch_event_source.start()?;
        self._reconcile_coordinator.start()?;
        Ok(())
    }

    fn shutdown_background_lanes(&self) -> Result<(), AtmError> {
        let mut first_error = None;
        for (lane, result) in [
            ("reconcile", self._reconcile_coordinator.shutdown()),
            ("watch", self._watch_event_source.shutdown()),
            ("notification", self._notification_sink.shutdown()),
        ] {
            if let Err(error) = result {
                if first_error.is_none() {
                    first_error = Some(error);
                } else {
                    tracing::warn!(
                        lane,
                        %error,
                        "daemon background lane shutdown failed after an earlier lane error"
                    );
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(unix)]
fn validate_runtime_socket_path() -> Result<(), AtmError> {
    let socket_path = atm_core::protocol::daemon_socket_path()?;
    if socket_path.as_os_str().is_empty() {
        return Err(AtmError::daemon_unavailable("daemon socket path must not be empty")
            .with_recovery(
                "Set ATM_DAEMON_SOCKET or ATM_HOME so atm-daemon resolves a concrete socket path before startup.",
            ));
    }
    if socket_path.file_name().is_none() {
        return Err(AtmError::daemon_unavailable(
            "daemon socket path must include a socket file name",
        )
        .with_recovery(
            "Set ATM_DAEMON_SOCKET to a full socket path or ensure ATM_HOME resolves to a writable daemon socket location.",
        ));
    }
    let Some(parent_dir) = socket_path.parent() else {
        return Err(AtmError::daemon_unavailable(
            "daemon socket path must include a parent directory",
        )
        .with_recovery(
            "Set ATM_DAEMON_SOCKET or ATM_HOME so atm-daemon resolves a socket path inside a real directory.",
        ));
    };
    std::fs::create_dir_all(parent_dir).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to create daemon socket parent directory at {}",
            parent_dir.display()
        ))
        .with_recovery(
            "Choose a writable ATM_DAEMON_SOCKET parent directory or adjust ATM_HOME before starting atm-daemon.",
        )
        .with_source(source)
    })?;
    let probe_path = parent_dir.join(format!(".atm-daemon-write-probe-{}", std::process::id()));
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe_path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "daemon socket parent directory is not writable at {}",
                parent_dir.display()
            ))
            .with_recovery(
                "Grant write access to the daemon socket parent directory or point ATM_DAEMON_SOCKET at a writable location before retrying.",
            )
            .with_source(source)
        })?;
    if let Err(error) = std::fs::remove_file(&probe_path) {
        tracing::warn!(
            path = %probe_path.display(),
            %error,
            "failed to remove daemon socket write probe file"
        );
    }
    Ok(())
}

#[cfg(unix)]
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

#[cfg(not(unix))]
fn validate_runtime_socket_path() -> Result<(), AtmError> {
    Err(AtmError::daemon_unavailable(
        "atm-daemon socket transport requires a Unix platform",
    ))
}

#[cfg(not(unix))]
fn validate_runtime_home_dir(_home_dir: &std::path::Path) -> Result<(), AtmError> {
    Err(AtmError::daemon_unavailable(
        "atm-daemon home directory validation requires a Unix platform",
    ))
}

pub(crate) fn compose_runtime() -> Result<RuntimeComposition, AtmError> {
    validate_runtime_socket_path()?;
    let home_dir = atm_core::home::atm_home()?;
    validate_runtime_home_dir(&home_dir)?;
    Ok(RuntimeComposition::new(home_dir))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use atm_core::boundary::ServerTransport;
    use tempfile::TempDir;

    use super::{RuntimeComposition, RuntimeLifecycle, RuntimeLifecycleState};

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
    fn runtime_composition_failed_startup_returns_to_stopped() {
        let tempdir = TempDir::new().expect("tempdir");
        let parent_file = tempdir.path().join("not-a-dir");
        std::fs::write(&parent_file, "x").expect("parent file");
        let socket_path = parent_file.join("atm.sock");
        let runtime = RuntimeComposition::new(tempdir.path().to_path_buf());

        let error = runtime
            .start_with_socket_path_for_test(socket_path)
            .expect_err("startup should fail");

        assert!(error.is_daemon_unavailable());
        assert_eq!(runtime.lifecycle_state(), RuntimeLifecycleState::Stopped);
    }

    #[cfg(unix)]
    #[test]
    fn server_transport_cannot_bootstrap_outside_runtime_composition_start() {
        let tempdir = TempDir::new().expect("tempdir");
        let runtime = RuntimeComposition::new(tempdir.path().to_path_buf());

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
}
