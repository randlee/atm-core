use crate::{
    DaemonConfigIngress, DaemonInboxExport, DaemonInboxIngress, DaemonNotificationSink,
    DaemonReconcileCoordinator, DaemonRequestDispatcher, DaemonStatusSource, FileWatchEventSource,
    LocalSocketServerTransport, PeerClientTransport,
};
use atm_core::{
    boundary::{
        ClientTransport, ConfigIngress, InboxExport, InboxIngress, NotificationSink,
        ReconcileCoordinator, RequestDispatcher, StatusSource, WatchEventSource,
    },
    error::AtmError,
};
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RuntimeLifecycleState {
    Starting,
    Running,
    Draining,
    #[default]
    Stopped,
}

/// Serializes legal daemon runtime ownership transitions.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct RuntimeLifecycle {
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
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct RuntimeComposition {
    lifecycle: Arc<RuntimeLifecycle>,
    server_transport: LocalSocketServerTransport,
    request_dispatcher: Arc<DaemonRequestDispatcher>,
    notification_sink: DaemonNotificationSink,
    status_source: DaemonStatusSource,
    watch_event_source: FileWatchEventSource,
    reconcile_coordinator: DaemonReconcileCoordinator,
    config_ingress: DaemonConfigIngress,
    inbox_ingress: DaemonInboxIngress,
    inbox_export: DaemonInboxExport,
    peer_client_transport: PeerClientTransport,
}

#[allow(dead_code)]
impl RuntimeComposition {
    fn new(home_dir: PathBuf) -> Self {
        Self {
            lifecycle: Arc::new(RuntimeLifecycle::new()),
            server_transport: LocalSocketServerTransport::new(),
            request_dispatcher: Arc::new(DaemonRequestDispatcher::new(home_dir)),
            notification_sink: DaemonNotificationSink::new(),
            status_source: DaemonStatusSource::new(),
            watch_event_source: FileWatchEventSource::new(),
            reconcile_coordinator: DaemonReconcileCoordinator::new(),
            config_ingress: DaemonConfigIngress::new(),
            inbox_ingress: DaemonInboxIngress::new(),
            inbox_export: DaemonInboxExport::new(),
            peer_client_transport: PeerClientTransport::new(),
        }
    }

    fn notification_sink(&self) -> &dyn NotificationSink {
        &self.notification_sink
    }

    fn request_dispatcher(&self) -> Arc<dyn RequestDispatcher + Send + Sync> {
        self.request_dispatcher.clone()
    }

    fn status_source(&self) -> &dyn StatusSource {
        &self.status_source
    }

    fn watch_event_source(&self) -> &dyn WatchEventSource {
        &self.watch_event_source
    }

    fn reconcile_coordinator(&self) -> &dyn ReconcileCoordinator {
        &self.reconcile_coordinator
    }

    fn config_ingress(&self) -> &dyn ConfigIngress {
        &self.config_ingress
    }

    fn inbox_ingress(&self) -> &dyn InboxIngress {
        &self.inbox_ingress
    }

    fn inbox_export(&self) -> &dyn InboxExport {
        &self.inbox_export
    }

    fn peer_client_transport(&self) -> &dyn ClientTransport {
        &self.peer_client_transport
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        self.lifecycle.transition(RuntimeLifecycleState::Starting)?;
        let runtime = match self.server_transport.prepare_runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                self.lifecycle.force_stopped()?;
                return Err(error);
            }
        };
        self.lifecycle.transition(RuntimeLifecycleState::Running)?;
        let result = runtime.serve(self.request_dispatcher());
        self.finish_runtime(result)
    }

    #[cfg(test)]
    pub(crate) fn start_with_socket_path_for_test(
        &self,
        socket_path: PathBuf,
    ) -> Result<(), AtmError> {
        self.lifecycle.transition(RuntimeLifecycleState::Starting)?;
        let runtime = match self
            .server_transport
            .prepare_runtime_at_socket_path(socket_path)
        {
            Ok(runtime) => runtime,
            Err(error) => {
                self.lifecycle.force_stopped()?;
                return Err(error);
            }
        };
        self.lifecycle.transition(RuntimeLifecycleState::Running)?;
        let result = runtime.serve(self.request_dispatcher());
        self.finish_runtime(result)
    }

    #[cfg(test)]
    pub(crate) fn lifecycle_state(&self) -> RuntimeLifecycleState {
        self.lifecycle.state()
    }

    fn finish_runtime(&self, result: Result<(), AtmError>) -> Result<(), AtmError> {
        // `Draining` is a lifecycle closure marker for the composed runtime
        // owner, not an extra timed phase inside this type. The actual grace
        // period lives down in the prepared server shutdown loop.
        let state_result = self
            .lifecycle
            .transition(RuntimeLifecycleState::Draining)
            .and_then(|_| self.lifecycle.transition(RuntimeLifecycleState::Stopped))
            .map(|_| ());
        if state_result.is_err() {
            self.lifecycle.force_stopped()?;
        }
        result
    }
}

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
        tracing::debug!(
            path = %probe_path.display(),
            %error,
            "failed to remove daemon socket write probe file"
        );
    }
    Ok(())
}

pub(crate) fn compose_runtime() -> Result<RuntimeComposition, AtmError> {
    validate_runtime_socket_path()?;
    Ok(RuntimeComposition::new(atm_core::home::atm_home()?))
}

#[cfg(test)]
mod tests {
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
    fn runtime_lifecycle_rejects_illegal_transitions() {
        let lifecycle = RuntimeLifecycle::new();
        let error = lifecycle
            .transition(RuntimeLifecycleState::Running)
            .expect_err("illegal transition");
        assert!(
            error
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
