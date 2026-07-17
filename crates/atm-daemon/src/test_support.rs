use atm_core::LocalFileNonClaudeOutbound;
use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::{DoctorEnvironmentVisibility, DoctorReport, DoctorStatus, DoctorSummary};
#[cfg(test)]
use atm_core::error::AtmError;
use atm_core::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_runtime::{RuntimeAssembly, RuntimeAssemblyInputs, assemble_sqlite_runtime};

use interprocess::local_socket::Name as LocalSocketName;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::runtime_sqlite_observer::DaemonRuntimeSqliteObserver;
const TEST_LOCAL_IPC_CONNECT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
const TEST_LOCAL_IPC_REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
const TEST_LOCAL_IPC_CONNECT_RETRY_INITIAL_DELAY: std::time::Duration =
    std::time::Duration::from_millis(1);
const TEST_LOCAL_IPC_CONNECT_RETRY_MAX_DELAY: std::time::Duration =
    std::time::Duration::from_millis(25);

pub(crate) struct LifecycleFlagResetGuard {
    lifecycle: LifecycleControlSourceAdapter,
}

impl LifecycleFlagResetGuard {
    pub(crate) fn install(lifecycle: LifecycleControlSourceAdapter) -> Self {
        lifecycle.set_terminate_for_test(false);
        lifecycle.set_reload_for_test(false);
        Self { lifecycle }
    }
}

impl Drop for LifecycleFlagResetGuard {
    fn drop(&mut self) {
        self.lifecycle.set_terminate_for_test(false);
        self.lifecycle.set_reload_for_test(false);
        if let Err(error) = self.lifecycle.reset_shared_state_for_test() {
            tracing::warn!(
                subsystem = "test_support",
                action = "reset_shared_state_for_test",
                outcome = "drain_failed",
                %error,
                "failed to drain shared lifecycle worker during test reset"
            );
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DoctorOnlyDispatcher;

impl atm_core::boundary::sealed::Sealed for DoctorOnlyDispatcher {}

impl RequestDispatcher for DoctorOnlyDispatcher {
    fn dispatch(
        &self,
        request: RequestEnvelope,
        _peer_origin: Option<&str>,
    ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
        match request {
            RequestEnvelope::Doctor(_) => Ok(ResponseEnvelope::Doctor(Box::new(DoctorReport {
                summary: DoctorSummary {
                    status: DoctorStatus::Healthy,
                    message: "ok".to_string(),
                    info_count: 0,
                    warning_count: 0,
                    error_count: 0,
                },
                findings: Vec::new(),
                recommendations: Vec::new(),
                environment: DoctorEnvironmentVisibility {
                    atm_home: None,
                    atm_team: None,
                    atm_identity: None,
                    team_override: None,
                },
                client_context: atm_core::doctor::DoctorExecutionContext::default(),
                daemon_context: None,
                member_roster: None,
                observability: AtmObservabilityHealth {
                    active_log_path: None,
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
                    detail: None,
                },
                post_send: atm_core::doctor::PostSendDoctorReport::default(),
                config: atm_core::boundary::ConfigDoctorReport::default(),
                mail_store: atm_core::boundary::MailStoreDoctorReport::default(),
                roster_store: atm_core::boundary::RosterStoreDoctorReport::default(),
                cross_host: None,
                daemon_runtime: None,
                drift_findings: Vec::new(),
                runtime_status: None,
                bootstrap_trace: None,
            }))),
            other => panic!("unexpected request in DoctorOnlyDispatcher: {other:?}"),
        }
    }
}

#[cfg(test)]
struct PanicUnwindSignal(Option<std::sync::mpsc::SyncSender<()>>);

#[cfg(test)]
impl Drop for PanicUnwindSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct PanicDispatcherWithUnwindSignal {
    unwind_tx: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
}

#[cfg(test)]
impl PanicDispatcherWithUnwindSignal {
    pub(crate) fn new(unwind_tx: std::sync::mpsc::SyncSender<()>) -> Self {
        Self {
            unwind_tx: std::sync::Mutex::new(Some(unwind_tx)),
        }
    }
}

#[cfg(test)]
impl atm_core::boundary::sealed::Sealed for PanicDispatcherWithUnwindSignal {}

#[cfg(test)]
impl RequestDispatcher for PanicDispatcherWithUnwindSignal {
    fn dispatch(
        &self,
        request: RequestEnvelope,
        _peer_origin: Option<&str>,
    ) -> Result<ResponseEnvelope, AtmError> {
        let unwind_signal = PanicUnwindSignal(
            self.unwind_tx
                .lock()
                .expect("panic dispatcher unwind signal lock")
                .take(),
        );
        let _keep_unwind_signal_until_panic_unwinds = unwind_signal;
        panic!("intentional dispatcher panic for test: {request:?}");
    }
}

pub(crate) fn sqlite_runtime_assembly_for_test(db_path: &std::path::Path) -> RuntimeAssembly {
    let config_current_dir = std::env::current_dir().unwrap_or_else(|_| {
        db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    });
    let log_dir = db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("daemon-test-logs");
    let observability = std::sync::Arc::new(
        crate::test_observability::TestDaemonObservability::new(log_dir)
            .unwrap_or_else(|error| panic!("failed to build daemon test observability: {error}")),
    );
    assemble_sqlite_runtime(RuntimeAssemblyInputs {
        sqlite_db_path: db_path.to_path_buf(),
        config_current_dir,
        sqlite_observer: std::sync::Arc::new(DaemonRuntimeSqliteObserver::new(observability)),
        non_claude_outbound: std::sync::Arc::new(LocalFileNonClaudeOutbound::new()),
    })
    .unwrap_or_else(|error| {
        panic!(
            "failed to assemble sqlite runtime for daemon test support at {}: {error}",
            db_path.display()
        )
    })
}

pub(crate) fn connect_daemon_local_ipc_until_ready(
    endpoint_path: &std::path::Path,
    ready_rx: std::sync::mpsc::Receiver<()>,
) -> LocalSocketStream {
    match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("timed out waiting for daemon local ipc ready signal")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("daemon local ipc ready signal sender dropped before readiness")
        }
    }
    let ipc_name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)
        .expect("ipc name")
        .into_owned();
    let deadline = std::time::Instant::now() + TEST_LOCAL_IPC_CONNECT_DEADLINE;
    let mut attempts = 0usize;
    let mut retry_delay = TEST_LOCAL_IPC_CONNECT_RETRY_INITIAL_DELAY;
    let mut last_error = None;
    while std::time::Instant::now() < deadline {
        match connect_local_ipc_with_timeout(ipc_name.clone(), TEST_LOCAL_IPC_CONNECT_DEADLINE) {
            Ok(stream) => return stream,
            Err(error) => {
                attempts += 1;
                last_error = Some(error);
                // The ready signal is the structural synchronization point; this retry only
                // covers the residual OS socket publication race after readiness.
                std::thread::sleep(retry_delay);
                retry_delay = std::cmp::min(
                    retry_delay.saturating_mul(2),
                    TEST_LOCAL_IPC_CONNECT_RETRY_MAX_DELAY,
                );
            }
        }
    }
    panic!(
        "connect daemon local ipc after ready signal failed after {attempts} attempts: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown connect error".to_string())
    )
}

pub(crate) fn connect_local_ipc_with_timeout(
    ipc_name: LocalSocketName<'static>,
    timeout: std::time::Duration,
) -> std::io::Result<LocalSocketStream> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("test-local-ipc-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(ipc_name));
        })
        .expect("spawn bounded local IPC connect helper");
    match result_rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "bounded local IPC connect timed out",
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "bounded local IPC connect worker disconnected",
        )),
    }
}

pub(crate) fn configure_test_local_ipc_timeouts(stream: &LocalSocketStream) {
    apply_test_deadline(
        stream.set_send_timeout(Some(TEST_LOCAL_IPC_REQUEST_DEADLINE)),
        "set send timeout",
    );
    apply_test_deadline(
        stream.set_recv_timeout(Some(TEST_LOCAL_IPC_REQUEST_DEADLINE)),
        "set recv timeout",
    );
}

fn apply_test_deadline(result: std::io::Result<()>, context: &str) {
    if let Err(error) = result {
        #[cfg(windows)]
        {
            if error.kind() == std::io::ErrorKind::Unsupported {
                return;
            }
        }
        panic!("{context}: {error}");
    }
}
